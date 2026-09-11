//! `HTTPS` record lookups.
use std::time::Duration;

use hickory_resolver::proto::rr::{
	Name, RData,
	rdata::svcb::{SvcParamKey, SvcParamValue},
};

/// An origin's HTTP/3 support, as its `HTTPS` record advertised it.
// spec:DNS#https-records
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct HttpsAdvertisement {
	/// The record's `port` SvcParam, or `None` when it named none and the origin's own port
	/// applies.
	pub port: Option<u16>,
	/// The record's own DNS TTL, which is how long the advertisement it carries lives.
	pub ttl: Duration,
}

/// Where an `HTTPS` record's advertisement goes once the resolver has read one.
///
/// The resolver cannot own the HTTP/3 upgrade cache directly: that cache is built after the
/// resolver, and the prober holds a client which holds the resolver in turn. The caller installs
/// this afterwards instead (see
/// [`FaithResolver::set_https_sink`](crate::FaithResolver::set_https_sink)), which also keeps this
/// crate free of the upgrade layer's types.
pub trait HttpsSink: Send + Sync {
	/// Whether an `HTTPS` record for `host` is worth querying at all right now.
	///
	/// Asked before the query so an origin already confirmed, already failed, or already holding a
	/// live advertisement costs no DNS traffic to re-learn what is known.
	fn wants(&self, host: &str) -> bool;

	/// Fold a record's advertisement into the upgrade layer's knowledge of `host`.
	fn record(&self, host: &str, advertisement: HttpsAdvertisement);
}

/// Whether an ALPN token names a version of HTTP/3.
///
/// The same family test the `Alt-Svc` reader applies, so a draft token like `h3-29` counts here
/// exactly as it does in a header.
// spec:H3UP#reading-advertisements
pub(crate) fn is_h3_alpn(token: &str) -> bool {
	token == "h3" || token.starts_with("h3-")
}

/// Read the HTTP/3 advertisement out of an `HTTPS` answer for `name`, if it carries one.
///
/// Only ServiceMode records are considered: an AliasMode record (`svc_priority` 0) redirects to
/// another name rather than describing this one, and following that redirection is a resolution
/// step this does not take. Among the rest the lowest `svc_priority` wins, which is the preference
/// order RFC 9460 defines.
///
/// A record whose target is neither the root (which per RFC 9460 §2.5.2 means the owner name
/// itself) nor the queried name designates a *different* host, and Faith only upgrades to the
/// origin's own host, so such a record is not acted on.
/// `queried` is the name the answer was actually asked for rather than the host as written, since
/// the search list can requalify a name before it reaches a server; comparison ignores the trailing
/// root so the two are judged on identity rather than on how each was spelled.
// spec:H3UP#advertisements-from-dns
pub(crate) fn read_https_answer(
	queried: &Name,
	answers: &[hickory_resolver::proto::rr::Record],
) -> Option<HttpsAdvertisement> {
	let mut best: Option<(u16, HttpsAdvertisement)> = None;

	for record in answers {
		let RData::HTTPS(https) = &record.data else {
			continue;
		};

		if https.svc_priority == 0 {
			continue;
		}

		if !https.target_name.is_root() && !https.target_name.eq_ignore_root(queried) {
			continue;
		}

		let mut has_h3 = false;
		let mut port = None;
		for (key, value) in &https.svc_params {
			match (key, value) {
				(SvcParamKey::Alpn, SvcParamValue::Alpn(alpn)) => {
					has_h3 = alpn.0.iter().any(|token| is_h3_alpn(token));
				}
				(SvcParamKey::Port, SvcParamValue::Port(value)) => port = Some(*value),
				_ => {}
			}
		}

		if !has_h3 {
			continue;
		}

		let advertisement = HttpsAdvertisement {
			port,
			ttl: Duration::from_secs(record.ttl.into()),
		};
		if best
			.as_ref()
			.is_none_or(|(best, _)| https.svc_priority < *best)
		{
			best = Some((https.svc_priority, advertisement));
		}
	}

	best.map(|(_, advertisement)| advertisement)
}

#[cfg(test)]
mod tests;
