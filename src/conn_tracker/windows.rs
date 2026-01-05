use std::collections::HashMap;
use std::io;
use std::mem;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

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
	if ret.is_err() {
		return Err(io::Error::from_raw_os_error(ret.0 as i32));
	}

	let table = unsafe { &*table };
	let entries =
		unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize) };

	for row in entries {
		if row.dwState != MIB_TCP_STATE_ESTAB {
			continue;
		}

		let local_ip = Ipv4Addr::from(u32::from_be(unsafe { row.dwLocalAddr.S_un.S_addr }));
		let local_port = (row.dwLocalPort & 0xFFFF) as u16;
		let local_port = u16::from_be(local_port);

		let remote_ip = Ipv4Addr::from(u32::from_be(unsafe { row.dwRemoteAddr.S_un.S_addr }));
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
		let _ = GetTcp6Table2(None, &mut size, false);
	}

	if size == 0 {
		return Ok(results);
	}

	let mut buffer = vec![0u8; size as usize];
	let table = buffer.as_mut_ptr() as *mut MIB_TCP6TABLE2;

	let ret = unsafe { GetTcp6Table2(Some(table), &mut size, false) };
	if ret.is_err() {
		return Err(io::Error::from_raw_os_error(ret.0 as i32));
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
			&rw_data as *const _ as *const u8,
			0,
			mem::size_of::<TCP_ESTATS_DATA_RW_v0>() as u32,
			0,
		);
		let _ = SetPerTcpConnectionEStats(
			row as *const MIB_TCPROW2 as *const _,
			TcpConnectionEstatsSndCong,
			&rw_snd as *const _ as *const u8,
			0,
			mem::size_of::<TCP_ESTATS_SND_CONG_RW_v0>() as u32,
			0,
		);
		let _ = SetPerTcpConnectionEStats(
			row as *const MIB_TCPROW2 as *const _,
			TcpConnectionEstatsBandwidth,
			&rw_bw as *const _ as *const u8,
			0,
			mem::size_of::<TCP_ESTATS_BANDWIDTH_RW_v0>() as u32,
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
			&rw_data as *const _ as *const u8,
			0,
			mem::size_of::<TCP_ESTATS_DATA_RW_v0>() as u32,
			0,
		);
		let _ = SetPerTcp6ConnectionEStats(
			row as *const MIB_TCP6ROW2 as *const _,
			TcpConnectionEstatsSndCong,
			&rw_snd as *const _ as *const u8,
			0,
			mem::size_of::<TCP_ESTATS_SND_CONG_RW_v0>() as u32,
			0,
		);
		let _ = SetPerTcp6ConnectionEStats(
			row as *const MIB_TCP6ROW2 as *const _,
			TcpConnectionEstatsBandwidth,
			&rw_bw as *const _ as *const u8,
			0,
			mem::size_of::<TCP_ESTATS_BANDWIDTH_RW_v0>() as u32,
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
			0,
			None,
			0,
			0,
			Some(&mut path_rod as *mut _ as *mut u8),
			0,
			mem::size_of::<TCP_ESTATS_PATH_ROD_v0>() as u32,
		)
		.is_ok()
	};

	let data_ok = unsafe {
		GetPerTcpConnectionEStats(
			row as *const MIB_TCPROW2 as *const _,
			TcpConnectionEstatsData,
			None,
			0,
			0,
			None,
			0,
			0,
			Some(&mut data_rod as *mut _ as *mut u8),
			0,
			mem::size_of::<TCP_ESTATS_DATA_ROD_v0>() as u32,
		)
		.is_ok()
	};

	let snd_ok = unsafe {
		GetPerTcpConnectionEStats(
			row as *const MIB_TCPROW2 as *const _,
			TcpConnectionEstatsSndCong,
			None,
			0,
			0,
			None,
			0,
			0,
			Some(&mut snd_rod as *mut _ as *mut u8),
			0,
			mem::size_of::<TCP_ESTATS_SND_CONG_ROD_v0>() as u32,
		)
		.is_ok()
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
		retrans: if data_ok {
			data_rod.DataSegsRetrans as u32
		} else {
			0
		},
		total_retrans: if data_ok {
			data_rod.DataSegsRetrans as u32
		} else {
			0
		},
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
			0,
			None,
			0,
			0,
			Some(&mut path_rod as *mut _ as *mut u8),
			0,
			mem::size_of::<TCP_ESTATS_PATH_ROD_v0>() as u32,
		)
		.is_ok()
	};

	let data_ok = unsafe {
		GetPerTcp6ConnectionEStats(
			row as *const MIB_TCP6ROW2 as *const _,
			TcpConnectionEstatsData,
			None,
			0,
			0,
			None,
			0,
			0,
			Some(&mut data_rod as *mut _ as *mut u8),
			0,
			mem::size_of::<TCP_ESTATS_DATA_ROD_v0>() as u32,
		)
		.is_ok()
	};

	let snd_ok = unsafe {
		GetPerTcp6ConnectionEStats(
			row as *const MIB_TCP6ROW2 as *const _,
			TcpConnectionEstatsSndCong,
			None,
			0,
			0,
			None,
			0,
			0,
			Some(&mut snd_rod as *mut _ as *mut u8),
			0,
			mem::size_of::<TCP_ESTATS_SND_CONG_ROD_v0>() as u32,
		)
		.is_ok()
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
		retrans: if data_ok {
			data_rod.DataSegsRetrans as u32
		} else {
			0
		},
		total_retrans: if data_ok {
			data_rod.DataSegsRetrans as u32
		} else {
			0
		},
		cwnd: if snd_ok { snd_rod.CurCwnd } else { 0 },
		delivery_rate: None,
	})
}
