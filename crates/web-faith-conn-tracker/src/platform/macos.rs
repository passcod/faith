use std::collections::HashMap;
use std::io;
use std::mem::{self, MaybeUninit};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::os::fd::RawFd;

use libc::{
	AF_INET, AF_INET6, IPPROTO_TCP, c_int, c_void, getpeername, getsockname, getsockopt, pid_t,
	proc_pidinfo, sockaddr, sockaddr_in, sockaddr_in6, socklen_t,
};

use super::{ConnectionKey, TcpStats};

const PROC_PIDLISTFDS: c_int = 1;
const PROX_FDTYPE_SOCKET: u32 = 2;
const TCP_CONNECTION_INFO: c_int = 0x106;

#[repr(C)]
struct ProcFdInfo {
	proc_fd: i32,
	proc_fdtype: u32,
}

#[repr(C)]
#[derive(Debug)]
struct TcpConnectionInfo {
	tcpi_state: u8,
	tcpi_snd_wscale: u8,
	tcpi_rcv_wscale: u8,
	_pad: u8,
	tcpi_options: u32,
	tcpi_flags: u32,
	tcpi_rttcur: u32,
	tcpi_rttvar: u32,
	tcpi_snd_ssthresh: u32,
	tcpi_snd_cwnd: u32,
	tcpi_rcv_space: u32,
	tcpi_snd_wnd: u32,
	tcpi_snd_nxt: u32,
	tcpi_rcv_nxt: u32,
	tcpi_last_outif: u32,
	tcpi_snd_sbbytes: u32,
	tcpi_txpackets: u64,
	tcpi_txbytes: u64,
	tcpi_txretransmitbytes: u64,
	tcpi_rxpackets: u64,
	tcpi_rxbytes: u64,
	tcpi_rxoutoforderbytes: u64,
	tcpi_txretransmitpackets: u64,
}

pub fn query_tcp_stats(keys: &[ConnectionKey]) -> io::Result<Vec<(ConnectionKey, TcpStats)>> {
	if keys.is_empty() {
		return Ok(Vec::new());
	}

	let key_map: HashMap<ConnectionKey, ()> = keys.iter().map(|k| (*k, ())).collect();
	let mut results = Vec::new();

	let pid = unsafe { libc::getpid() };
	let fds = list_fds(pid)?;

	for fd_info in fds {
		if fd_info.proc_fdtype != PROX_FDTYPE_SOCKET {
			continue;
		}

		let fd = fd_info.proc_fd;

		if let Some((local_addr, remote_addr)) = get_socket_addrs(fd) {
			let key = ConnectionKey {
				local_addr,
				remote_addr,
			};

			if key_map.contains_key(&key) {
				if let Some(stats) = get_tcp_connection_info(fd) {
					results.push((key, stats));
				}
			}
		}
	}

	Ok(results)
}

fn list_fds(pid: pid_t) -> io::Result<Vec<ProcFdInfo>> {
	let buf_size = unsafe { proc_pidinfo(pid, PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };

	if buf_size <= 0 {
		return Err(io::Error::last_os_error());
	}

	let count = buf_size as usize / mem::size_of::<ProcFdInfo>();
	let mut fds: Vec<ProcFdInfo> = Vec::with_capacity(count);

	let result = unsafe {
		proc_pidinfo(
			pid,
			PROC_PIDLISTFDS,
			0,
			fds.as_mut_ptr() as *mut c_void,
			buf_size,
		)
	};

	if result <= 0 {
		return Err(io::Error::last_os_error());
	}

	let actual_count = result as usize / mem::size_of::<ProcFdInfo>();
	unsafe { fds.set_len(actual_count) };

	Ok(fds)
}

fn get_socket_addrs(fd: RawFd) -> Option<(SocketAddr, SocketAddr)> {
	let mut local_addr: MaybeUninit<sockaddr_in6> = MaybeUninit::uninit();
	let mut local_len: socklen_t = mem::size_of::<sockaddr_in6>() as socklen_t;

	let ret = unsafe { getsockname(fd, local_addr.as_mut_ptr() as *mut sockaddr, &mut local_len) };
	if ret != 0 {
		return None;
	}

	let mut remote_addr: MaybeUninit<sockaddr_in6> = MaybeUninit::uninit();
	let mut remote_len: socklen_t = mem::size_of::<sockaddr_in6>() as socklen_t;

	let ret = unsafe {
		getpeername(
			fd,
			remote_addr.as_mut_ptr() as *mut sockaddr,
			&mut remote_len,
		)
	};
	if ret != 0 {
		return None;
	}

	let local = unsafe { sockaddr_to_socketaddr(local_addr.as_ptr() as *const sockaddr)? };
	let remote = unsafe { sockaddr_to_socketaddr(remote_addr.as_ptr() as *const sockaddr)? };

	Some((local, remote))
}

unsafe fn sockaddr_to_socketaddr(addr: *const sockaddr) -> Option<SocketAddr> {
	let family = (*addr).sa_family as c_int;

	match family {
		AF_INET => {
			let addr_in = addr as *const sockaddr_in;
			let ip = Ipv4Addr::from(u32::from_be((*addr_in).sin_addr.s_addr));
			let port = u16::from_be((*addr_in).sin_port);
			Some(SocketAddr::new(IpAddr::V4(ip), port))
		}
		AF_INET6 => {
			let addr_in6 = addr as *const sockaddr_in6;
			let ip = Ipv6Addr::from((*addr_in6).sin6_addr.s6_addr);
			let port = u16::from_be((*addr_in6).sin6_port);
			Some(SocketAddr::new(IpAddr::V6(ip), port))
		}
		_ => None,
	}
}

fn get_tcp_connection_info(fd: RawFd) -> Option<TcpStats> {
	let mut info: MaybeUninit<TcpConnectionInfo> = MaybeUninit::uninit();
	let mut len: socklen_t = mem::size_of::<TcpConnectionInfo>() as socklen_t;

	let ret = unsafe {
		getsockopt(
			fd,
			IPPROTO_TCP,
			TCP_CONNECTION_INFO,
			info.as_mut_ptr() as *mut c_void,
			&mut len,
		)
	};

	if ret != 0 {
		return None;
	}

	let info = unsafe { info.assume_init() };

	Some(TcpStats {
		rtt_us: info.tcpi_rttcur * 1000,
		rtt_var_us: info.tcpi_rttvar * 1000,
		lost: None,
		retrans: info.tcpi_txretransmitpackets as u32,
		total_retrans: info.tcpi_txretransmitpackets as u32,
		cwnd: info.tcpi_snd_cwnd,
		delivery_rate: None,
	})
}
