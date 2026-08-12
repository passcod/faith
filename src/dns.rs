//! Fáith's own DNS resolver.
//!
//! `reqwest`'s built-in hickory resolver and its in-memory cache are `pub(crate)`, so the only
//! way to warm that cache is to make a request through it — which is `preconnect`'s job, not
//! `prefetchDns`'s (that verb must not touch the origin). To let `prefetchDns` populate the cache
//! a later request reads, Fáith owns the resolver instead: this type is installed on the `reqwest`
//! client with `ClientBuilder::dns_resolver`, so reqwest routes every lookup through it, and
//! `prefetch` calls the same resolver directly. Both share one `TokioResolver`, so a name warmed
//! by `prefetchDns` is already cached when a request looks it up.
//!
//! The resolver mirrors reqwest's own `new_resolver`: system configuration where available, Google
//! DNS as a fallback, and `Ipv4AndIpv6` so Happy Eyeballs races both families. `dns.overrides` are
//! not this type's concern — reqwest layers them on top of any custom resolver.
//!
//! spec:WARM

use std::{net::SocketAddr, sync::Arc};

use hickory_resolver::{
	TokioResolver,
	config::{GOOGLE, LookupIpStrategy, ResolverConfig},
	net::{NetError, runtime::TokioRuntimeProvider},
};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use tokio::sync::OnceCell;

/// A hickory resolver Fáith owns, shared between reqwest's request path and `prefetchDns`.
#[derive(Clone, Default)]
pub struct FaithResolver {
	/// Built lazily inside a tokio runtime (hickory needs one), then shared: reqwest's
	/// `Resolve::resolve` and [`FaithResolver::prefetch`] both read this cell, so they resolve
	/// against — and cache into — the same resolver.
	state: Arc<OnceCell<TokioResolver>>,
}

impl std::fmt::Debug for FaithResolver {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("FaithResolver").finish_non_exhaustive()
	}
}

impl FaithResolver {
	async fn resolver(&self) -> Result<&TokioResolver, NetError> {
		self.state
			.get_or_try_init(|| async { new_resolver() })
			.await
	}

	/// Resolve `host` and leave the answer in the shared cache, so a later request skips the
	/// lookup. Any failure is swallowed: the warm-up is advisory (spec:WARM).
	pub async fn prefetch(&self, host: &str) {
		if let Ok(resolver) = self.resolver().await {
			let _ = resolver.lookup_ip(host).await;
		}
	}
}

impl Resolve for FaithResolver {
	fn resolve(&self, name: Name) -> Resolving {
		let this = self.clone();
		Box::pin(async move {
			let resolver = this.resolver().await?;
			let lookup = resolver.lookup_ip(name.as_str()).await?;
			// Collect into an owned iterator: the lookup borrows the resolver, but the returned
			// `Addrs` has to be `'static`. Port `0` is a placeholder reqwest fills from the URL.
			let addrs: Vec<SocketAddr> = lookup.iter().map(|ip| SocketAddr::new(ip, 0)).collect();
			Ok(Box::new(addrs.into_iter()) as Addrs)
		})
	}
}

/// Mirror of reqwest's `new_resolver`: read the system configuration, fall back to Google DNS if
/// that fails, and look up both address families for Happy Eyeballs.
fn new_resolver() -> Result<TokioResolver, NetError> {
	let mut builder = TokioResolver::builder_tokio().unwrap_or_else(|_| {
		TokioResolver::builder_with_config(
			ResolverConfig::udp_and_tcp(&GOOGLE),
			TokioRuntimeProvider::default(),
		)
	});
	builder.options_mut().ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
	builder.build()
}
