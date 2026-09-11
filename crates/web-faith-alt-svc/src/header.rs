//! `Alt-Svc` header parsing.
use std::{fmt, str::FromStr, time::Duration};

use crate::cache::AltSvcAdvertisement;

/// The header advertised no HTTP/3 alternative service.
///
/// Covers a `clear` header, one advertising only other protocols, and one that does not parse:
/// none of them gives an origin to act on, and the header is a hint, so nothing distinguishes
/// them in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoHttp3Alternative;

impl fmt::Display for NoHttp3Alternative {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("no HTTP/3 alternative service advertised")
	}
}

impl std::error::Error for NoHttp3Alternative {}

impl FromStr for AltSvcAdvertisement {
	type Err = NoHttp3Alternative;

	/// Read the first HTTP/3 alternative service out of an `Alt-Svc` header value.
	fn from_str(value: &str) -> Result<Self, Self::Err> {
		parse(value).ok_or(NoHttp3Alternative)
	}
}

fn parse(value: &str) -> Option<AltSvcAdvertisement> {
	if value == "clear" {
		return None;
	}

	for service in value.split(',') {
		let service = service.trim();
		if service.is_empty() {
			continue;
		}

		let mut protocol_id: Option<&str> = None;
		let mut host: Option<&str> = None;
		let mut port: Option<u16> = None;
		let mut max_age: Option<Duration> = None;

		for param in service.split(';') {
			let param = param.trim();
			if param.is_empty() {
				continue;
			}

			let Some((key, value)) = param.split_once('=') else {
				continue;
			};

			let key = key.trim();
			let value = value.trim().trim_matches('"');

			match key {
				"ma" => {
					if let Ok(secs) = value.parse::<u64>() {
						max_age = Some(Duration::from_secs(secs));
					}
				}
				_ if key.starts_with("h3") => {
					protocol_id = Some(key);
					// The alt-authority is `[host]:port`, where an omitted host means
					// the origin's own. Keep the host: acting on an advertisement for
					// a different host would be the same unsupported inference as
					// acting on one for a different port.
					//
					// Split on the *last* colon so a bracketed IPv6 literal survives,
					// and keep it exactly as written — brackets included. That is the
					// form `Url::host_str` also returns for IPv6, so comparing the two
					// needs no normalising on either side.
					if let Some((alt_host, port_str)) = value.rsplit_once(':') {
						host = Some(alt_host);
						if let Ok(p) = port_str.parse::<u16>() {
							port = Some(p);
						}
					}
				}
				_ => {}
			}
		}

		if protocol_id.is_some() && port.is_some() {
			return Some(AltSvcAdvertisement {
				host: host.unwrap_or_default().to_owned(),
				port: port.unwrap(),
				max_age,
			});
		}
	}

	None
}

#[cfg(test)]
mod tests;
