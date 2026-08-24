use std::io;
use std::net::SocketAddr;

use netlink_packet_core::{
	NLM_F_DUMP, NLM_F_REQUEST, NetlinkHeader, NetlinkMessage, NetlinkPayload,
};

use netlink_packet_sock_diag::{
	SockDiagMessage,
	constants::*,
	inet::{ExtensionFlags, InetRequest, SocketId, StateFlags, nlas::Nla},
};
use netlink_sys::{Socket, SocketAddr as NetlinkSocketAddr, protocols::NETLINK_SOCK_DIAG};

use super::{ConnectionKey, TcpStats};

pub fn query_tcp_stats(keys: &[ConnectionKey]) -> io::Result<Vec<(ConnectionKey, TcpStats)>> {
	if keys.is_empty() {
		return Ok(Vec::new());
	}

	let mut socket = Socket::new(NETLINK_SOCK_DIAG)?;
	socket.bind_auto()?;
	socket.connect(&NetlinkSocketAddr::new(0, 0))?;

	let mut results = Vec::new();

	let has_v4 = keys.iter().any(|k| k.local_addr.is_ipv4());
	let has_v6 = keys.iter().any(|k| k.local_addr.is_ipv6());

	if has_v4 {
		results.extend(query_family(&socket, AF_INET, keys)?);
	}
	if has_v6 {
		results.extend(query_family(&socket, AF_INET6, keys)?);
	}

	Ok(results)
}

fn query_family(
	socket: &Socket,
	family: u8,
	keys: &[ConnectionKey],
) -> io::Result<Vec<(ConnectionKey, TcpStats)>> {
	let mut results = Vec::new();

	let socket_id = if family == AF_INET {
		SocketId::new_v4()
	} else {
		SocketId::new_v6()
	};

	let request = InetRequest {
		family,
		protocol: IPPROTO_TCP,
		extensions: ExtensionFlags::INFO,
		states: StateFlags::all(),
		socket_id,
	};

	let mut nl_hdr = NetlinkHeader::default();
	nl_hdr.flags = NLM_F_REQUEST | NLM_F_DUMP;

	let mut packet = NetlinkMessage::new(nl_hdr, SockDiagMessage::InetRequest(request).into());
	packet.finalize();

	let mut buf = vec![0u8; packet.header.length as usize];
	packet.serialize(&mut buf);
	socket.send(&buf, 0)?;

	let mut recv_buf = vec![0u8; 65536];

	loop {
		let size = socket.recv(&mut &mut recv_buf[..], 0)?;
		if size == 0 {
			break;
		}

		let mut offset = 0;
		loop {
			let bytes = &recv_buf[offset..];
			if bytes.is_empty() {
				break;
			}

			let msg = NetlinkMessage::<SockDiagMessage>::deserialize(bytes)
				.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

			let msg_len = msg.header.length as usize;
			if msg_len == 0 {
				break;
			}

			match msg.payload {
				NetlinkPayload::Done(_) => return Ok(results),
				NetlinkPayload::Error(e) => {
					if let Some(code) = e.code {
						return Err(io::Error::new(
							io::ErrorKind::Other,
							format!("netlink error code: {}", code),
						));
					}
				}
				NetlinkPayload::InnerMessage(SockDiagMessage::InetResponse(resp)) => {
					if let Some((key, stats)) = process_response(&resp, keys) {
						results.push((key, stats));
					}
				}
				_ => {}
			}

			offset += msg_len;
			if offset >= size {
				break;
			}
		}
	}

	Ok(results)
}

fn process_response(
	resp: &netlink_packet_sock_diag::inet::InetResponse,
	keys: &[ConnectionKey],
) -> Option<(ConnectionKey, TcpStats)> {
	let socket_id = &resp.header.socket_id;

	let local_addr = SocketAddr::new(socket_id.source_address, socket_id.source_port);
	let remote_addr = SocketAddr::new(socket_id.destination_address, socket_id.destination_port);

	let key = ConnectionKey {
		local_addr,
		remote_addr,
	};

	if !keys.contains(&key) {
		return None;
	}

	let mut stats = TcpStats::default();

	for nla in &resp.nlas {
		if let Nla::TcpInfo(info) = nla {
			stats.rtt_us = info.rtt;
			stats.rtt_var_us = info.rttvar;
			stats.lost = Some(info.lost);
			stats.retrans = info.retrans;
			stats.total_retrans = info.total_retrans;
			stats.cwnd = info.snd_cwnd;
			stats.delivery_rate = Some(info.delivery_rate);
		}
	}

	Some((key, stats))
}
