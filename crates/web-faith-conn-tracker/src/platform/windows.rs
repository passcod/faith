use std::collections::HashMap;
use std::io;
use std::mem;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::ptr;

use windows::Win32::NetworkManagement::IpHelper::{
	GetPerTcp6ConnectionEStats, GetPerTcpConnectionEStats, GetTcp6Table2, GetTcpTable2,
	MIB_TCP_STATE_ESTAB, MIB_TCP6ROW2, MIB_TCP6TABLE2, MIB_TCPROW2, MIB_TCPTABLE2,
	SetPerTcp6ConnectionEStats, SetPerTcpConnectionEStats, TCP_ESTATS_BANDWIDTH_RW_v0,
	TCP_ESTATS_DATA_ROD_v0, TCP_ESTATS_DATA_RW_v0, TCP_ESTATS_PATH_ROD_v0,
	TCP_ESTATS_SND_CONG_ROD_v0, TCP_ESTATS_SND_CONG_RW_v0, TcpBoolOptEnabled,
	TcpConnectionEstatsBandwidth, TcpConnectionEstatsData, TcpConnectionEstatsPath,
	TcpConnectionEstatsSndCong,
};

use super::{ConnectionKey, TcpStats};

const ERROR_SUCCESS: u32 = 0;

pub fn query_tcp_stats(keys: &[ConnectionKey]) -> io::Result<Vec<(ConnectionKey, TcpStats)>> {
	if keys.is_empty() {
		return Ok(Vec::new());
	}

	let key_map: HashMap<ConnectionKey, ()> = keys.iter().map(|k| (*k, ())).collect();
	let mut results = Vec::new();

	let has_v4 = keys.iter().any(|k| k.local_addr.is_ipv4());
	let has_v6 = keys.iter().any(|k| k.local_addr.is_ipv6());

	if has_v4 {
		if let Ok(v4_results) = query_tcp4_stats(&key_map) {
			results.extend(v4_results);
		}
	}

	if has_v6 {
		if let Ok(v6_results) = query_tcp6_stats(&key_map) {
			results.extend(v6_results);
		}
	}

	Ok(results)
}

fn query_tcp4_stats(
	key_map: &HashMap<ConnectionKey, ()>,
) -> io::Result<Vec<(ConnectionKey, TcpStats)>> {
	let mut results = Vec::new();

	let mut size: u32 = 0;
	unsafe {
		let _ = GetTcpTable2(None, &mut size, false);
	}

	if size == 0 {
		return Ok(results);
	}

	let mut buffer = vec![0u8; size as usize];
	let table = buffer.as_mut_ptr() as *mut MIB_TCPTABLE2;

	let ret = unsafe { GetTcpTable2(Some(table), &mut size, false) };
	if ret != ERROR_SUCCESS {
		return Err(io::Error::from_raw_os_error(ret as i32));
	}

	let table = unsafe { &*table };
	let entries =
		unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize) };

	for row in entries {
		if row.dwState != MIB_TCP_STATE_ESTAB.0 as u32 {
			continue;
		}

		let local_ip = Ipv4Addr::from(u32::from_be(row.dwLocalAddr));
		let local_port = (row.dwLocalPort & 0xFFFF) as u16;
		let local_port = u16::from_be(local_port);

		let remote_ip = Ipv4Addr::from(u32::from_be(row.dwRemoteAddr));
		let remote_port = (row.dwRemotePort & 0xFFFF) as u16;
		let remote_port = u16::from_be(remote_port);

		let key = ConnectionKey {
			local_addr: SocketAddr::new(IpAddr::V4(local_ip), local_port),
			remote_addr: SocketAddr::new(IpAddr::V4(remote_ip), remote_port),
		};

		if key_map.contains_key(&key) {
			if let Some(stats) = get_tcp4_estats(row) {
				results.push((key, stats));
			}
		}
	}

	Ok(results)
}

fn query_tcp6_stats(
	key_map: &HashMap<ConnectionKey, ()>,
) -> io::Result<Vec<(ConnectionKey, TcpStats)>> {
	let mut results = Vec::new();

	let mut size: u32 = 0;
	unsafe {
		let _ = GetTcp6Table2(ptr::null_mut(), &mut size, false);
	}

	if size == 0 {
		return Ok(results);
	}

	let mut buffer = vec![0u8; size as usize];
	let table = buffer.as_mut_ptr() as *mut MIB_TCP6TABLE2;

	let ret = unsafe { GetTcp6Table2(table, &mut size, false) };
	if ret != ERROR_SUCCESS {
		return Err(io::Error::from_raw_os_error(ret as i32));
	}

	let table = unsafe { &*table };
	let entries =
		unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize) };

	for row in entries {
		if row.State != MIB_TCP_STATE_ESTAB {
			continue;
		}

		let local_ip = Ipv6Addr::from(unsafe { row.LocalAddr.u.Byte });
		let local_port = (row.dwLocalPort & 0xFFFF) as u16;
		let local_port = u16::from_be(local_port);

		let remote_ip = Ipv6Addr::from(unsafe { row.RemoteAddr.u.Byte });
		let remote_port = (row.dwRemotePort & 0xFFFF) as u16;
		let remote_port = u16::from_be(remote_port);

		let key = ConnectionKey {
			local_addr: SocketAddr::new(IpAddr::V6(local_ip), local_port),
			remote_addr: SocketAddr::new(IpAddr::V6(remote_ip), remote_port),
		};

		if key_map.contains_key(&key) {
			if let Some(stats) = get_tcp6_estats(row) {
				results.push((key, stats));
			}
		}
	}

	Ok(results)
}

fn struct_as_bytes<T>(s: &T) -> &[u8] {
	unsafe { std::slice::from_raw_parts(s as *const T as *const u8, mem::size_of::<T>()) }
}

fn struct_as_bytes_mut<T>(s: &mut T) -> &mut [u8] {
	unsafe { std::slice::from_raw_parts_mut(s as *mut T as *mut u8, mem::size_of::<T>()) }
}

fn enable_estats_for_row4(row: &MIB_TCPROW2) {
	let rw_data = TCP_ESTATS_DATA_RW_v0 {
		EnableCollection: true,
	};
	let rw_snd = TCP_ESTATS_SND_CONG_RW_v0 {
		EnableCollection: true,
	};
	let rw_bw = TCP_ESTATS_BANDWIDTH_RW_v0 {
		EnableCollectionOutbound: TcpBoolOptEnabled,
		EnableCollectionInbound: TcpBoolOptEnabled,
	};

	unsafe {
		let _ = SetPerTcpConnectionEStats(
			row as *const MIB_TCPROW2 as *const _,
			TcpConnectionEstatsData,
			struct_as_bytes(&rw_data),
			0,
			0,
		);
		let _ = SetPerTcpConnectionEStats(
			row as *const MIB_TCPROW2 as *const _,
			TcpConnectionEstatsSndCong,
			struct_as_bytes(&rw_snd),
			0,
			0,
		);
		let _ = SetPerTcpConnectionEStats(
			row as *const MIB_TCPROW2 as *const _,
			TcpConnectionEstatsBandwidth,
			struct_as_bytes(&rw_bw),
			0,
			0,
		);
	}
}

fn enable_estats_for_row6(row: &MIB_TCP6ROW2) {
	let rw_data = TCP_ESTATS_DATA_RW_v0 {
		EnableCollection: true,
	};
	let rw_snd = TCP_ESTATS_SND_CONG_RW_v0 {
		EnableCollection: true,
	};
	let rw_bw = TCP_ESTATS_BANDWIDTH_RW_v0 {
		EnableCollectionOutbound: TcpBoolOptEnabled,
		EnableCollectionInbound: TcpBoolOptEnabled,
	};

	unsafe {
		let _ = SetPerTcp6ConnectionEStats(
			row as *const MIB_TCP6ROW2 as *const _,
			TcpConnectionEstatsData,
			struct_as_bytes(&rw_data),
			0,
			0,
		);
		let _ = SetPerTcp6ConnectionEStats(
			row as *const MIB_TCP6ROW2 as *const _,
			TcpConnectionEstatsSndCong,
			struct_as_bytes(&rw_snd),
			0,
			0,
		);
		let _ = SetPerTcp6ConnectionEStats(
			row as *const MIB_TCP6ROW2 as *const _,
			TcpConnectionEstatsBandwidth,
			struct_as_bytes(&rw_bw),
			0,
			0,
		);
	}
}

fn get_tcp4_estats(row: &MIB_TCPROW2) -> Option<TcpStats> {
	enable_estats_for_row4(row);

	let mut path_rod: TCP_ESTATS_PATH_ROD_v0 = unsafe { mem::zeroed() };
	let mut data_rod: TCP_ESTATS_DATA_ROD_v0 = unsafe { mem::zeroed() };
	let mut snd_rod: TCP_ESTATS_SND_CONG_ROD_v0 = unsafe { mem::zeroed() };

	let path_ok = unsafe {
		GetPerTcpConnectionEStats(
			row as *const MIB_TCPROW2 as *const _,
			TcpConnectionEstatsPath,
			None,
			0,
			None,
			0,
			Some(struct_as_bytes_mut(&mut path_rod)),
			0,
		) == ERROR_SUCCESS
	};

	let data_ok = unsafe {
		GetPerTcpConnectionEStats(
			row as *const MIB_TCPROW2 as *const _,
			TcpConnectionEstatsData,
			None,
			0,
			None,
			0,
			Some(struct_as_bytes_mut(&mut data_rod)),
			0,
		) == ERROR_SUCCESS
	};

	let snd_ok = unsafe {
		GetPerTcpConnectionEStats(
			row as *const MIB_TCPROW2 as *const _,
			TcpConnectionEstatsSndCong,
			None,
			0,
			None,
			0,
			Some(struct_as_bytes_mut(&mut snd_rod)),
			0,
		) == ERROR_SUCCESS
	};

	if !path_ok && !data_ok && !snd_ok {
		return None;
	}

	Some(TcpStats {
		rtt_us: if path_ok {
			path_rod.SmoothedRtt * 1000
		} else {
			0
		},
		rtt_var_us: if path_ok { path_rod.RttVar * 1000 } else { 0 },
		lost: None,
		retrans: if path_ok { path_rod.PktsRetrans } else { 0 },
		total_retrans: if path_ok { path_rod.PktsRetrans } else { 0 },
		cwnd: if snd_ok { snd_rod.CurCwnd } else { 0 },
		delivery_rate: None,
	})
}

fn get_tcp6_estats(row: &MIB_TCP6ROW2) -> Option<TcpStats> {
	enable_estats_for_row6(row);

	let mut path_rod: TCP_ESTATS_PATH_ROD_v0 = unsafe { mem::zeroed() };
	let mut data_rod: TCP_ESTATS_DATA_ROD_v0 = unsafe { mem::zeroed() };
	let mut snd_rod: TCP_ESTATS_SND_CONG_ROD_v0 = unsafe { mem::zeroed() };

	let path_ok = unsafe {
		GetPerTcp6ConnectionEStats(
			row as *const MIB_TCP6ROW2 as *const _,
			TcpConnectionEstatsPath,
			None,
			0,
			None,
			0,
			Some(struct_as_bytes_mut(&mut path_rod)),
			0,
		) == ERROR_SUCCESS
	};

	let data_ok = unsafe {
		GetPerTcp6ConnectionEStats(
			row as *const MIB_TCP6ROW2 as *const _,
			TcpConnectionEstatsData,
			None,
			0,
			None,
			0,
			Some(struct_as_bytes_mut(&mut data_rod)),
			0,
		) == ERROR_SUCCESS
	};

	let snd_ok = unsafe {
		GetPerTcp6ConnectionEStats(
			row as *const MIB_TCP6ROW2 as *const _,
			TcpConnectionEstatsSndCong,
			None,
			0,
			None,
			0,
			Some(struct_as_bytes_mut(&mut snd_rod)),
			0,
		) == ERROR_SUCCESS
	};

	if !path_ok && !data_ok && !snd_ok {
		return None;
	}

	Some(TcpStats {
		rtt_us: if path_ok {
			path_rod.SmoothedRtt * 1000
		} else {
			0
		},
		rtt_var_us: if path_ok { path_rod.RttVar * 1000 } else { 0 },
		lost: None,
		retrans: if path_ok { path_rod.PktsRetrans } else { 0 },
		total_retrans: if path_ok { path_rod.PktsRetrans } else { 0 },
		cwnd: if snd_ok { snd_rod.CurCwnd } else { 0 },
		delivery_rate: None,
	})
}
