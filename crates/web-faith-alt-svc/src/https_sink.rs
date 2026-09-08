use std::sync::{Arc, Weak};

use crate::{cache::AltSvcCache, prober::H3Prober};

/// Feeds `HTTPS` DNS records into the upgrade layer, so an origin advertising `alpn="h3"` is
/// probe-worthy from its first request rather than from the first TCP response carrying an
/// `Alt-Svc` header.
///
/// Installed on the resolver by the agent (see [`web_faith_dns::FaithResolver::set_https_sink`]),
/// which is the only place that holds all three: the resolver is built before the cache, and the
/// prober holds a client that holds the resolver, so nothing lower down can own this.
///
/// The record is read at the bare name, which per RFC 9460 is the record for the origin at the
/// default HTTPS port; the resolver sees only a hostname, so that is also the only origin it could
/// name. Recording it there is right whichever request triggered the lookup, because what the
/// record describes does not depend on who asked.
// spec:H3UP#advertisements-from-dns
// spec:DNS#https-records
pub struct H3HttpsSink {
	cache: Arc<AltSvcCache>,
	/// Weak, and load-bearingly so: the prober holds the client, the client holds the resolver,
	/// and the resolver holds this sink. A strong reference here would close that ring and leak
	/// the whole graph — connection pool included — past `Agent::close`, which works by dropping
	/// the client. The agent owns the only strong reference, so this lives exactly as long as the
	/// agent's prober does.
	///
	/// `None` rather than a dead handle when probing is off, where an advertisement is acted on
	/// inline by the next foreground request instead.
	// spec:PROBE
	prober: Option<Weak<H3Prober>>,
}

impl std::fmt::Debug for H3HttpsSink {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("H3HttpsSink")
			.field("probing", &self.prober.is_some())
			.finish()
	}
}

impl H3HttpsSink {
	pub fn new(cache: Arc<AltSvcCache>, prober: Option<&Arc<H3Prober>>) -> Self {
		Self {
			cache,
			prober: prober.map(Arc::downgrade),
		}
	}

	/// The origin a record at `host` describes: the default HTTPS port, which is the port whose
	/// record lives at the bare name.
	fn origin_url(host: &str) -> Option<reqwest::Url> {
		reqwest::Url::parse(&format!("https://{host}")).ok()
	}
}

impl web_faith_dns::HttpsSink for H3HttpsSink {
	fn wants(&self, host: &str) -> bool {
		Self::origin_url(host).is_some_and(|url| self.cache.wants_https_record(&url))
	}

	fn record(&self, host: &str, advertisement: web_faith_dns::HttpsAdvertisement) {
		let Some(url) = Self::origin_url(host) else {
			return;
		};

		self.cache
			.record_https_record(&url, advertisement.port, advertisement.ttl);

		// Probe straight away rather than waiting for the request that triggered the lookup to
		// finish: the point of reading DNS is that the path can be verified while that request is
		// still on TCP, so the one after it upgrades.
		//
		// A prober that has gone means the agent was closed (or rebuilt) while this query was in
		// flight; the advertisement above is still worth keeping, but there is nothing left to
		// probe it with, and resurrecting a dropped client to try would be exactly wrong.
		if let Some(prober) = self.prober.as_ref().and_then(Weak::upgrade) {
			prober.maybe_probe(&url);
		}
	}
}
