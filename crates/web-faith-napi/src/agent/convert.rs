//! Reading a JavaScript `AgentOptions` into the options the client validates.

#[cfg(feature = "cache")]
use http_cache_reqwest::CacheMode;
use napi::{Either, bindgen_prelude::Buffer};

use web_faith::{client::RedirectPolicy, options};

#[cfg(feature = "cookies")]
use web_faith_cookies::CookieLimits;

use crate::agent::{AgentOptions, Http3Congestion};

#[cfg(feature = "cache")]
use crate::agent::CacheStore;

/// Read the JavaScript options object into the shape the client validates.
///
/// A field-for-field mapping wherever the two agree, which is most of them; what differs is where
/// JavaScript expresses a choice as a union or a string that Rust has a type for.
impl From<AgentOptions> for options::AgentOptions {
	fn from(opts: AgentOptions) -> Self {
		Self {
			#[cfg(feature = "cache")]
			cache: opts.cache.map(|cache| options::CacheOptions {
				store: cache.store.map(|store| match store {
					CacheStore::Disk => options::CacheStore::Disk,
					CacheStore::Memory => options::CacheStore::Memory,
				}),
				capacity: cache.capacity,
				mode: cache.mode.map(CacheMode::from),
				path: cache.path,
				shared: cache.shared,
			}),
			// `false` and an absent value both mean no jar; `true` means one with default limits.
			#[cfg(feature = "cookies")]
			cookies: match opts.cookies {
				None | Some(Either::A(false)) => None,
				Some(Either::A(true)) => Some(CookieLimits::default()),
				Some(Either::B(cookies)) => Some((&cookies).into()),
			},
			dns: opts.dns.map(|dns| options::DnsOptions {
				#[cfg(feature = "dns")]
				system: dns.system,
				overrides: dns.overrides.map(|overrides| {
					overrides
						.into_iter()
						.map(|o| options::DnsOverride {
							domain: o.domain,
							addresses: o.addresses,
						})
						.collect()
				}),
				#[cfg(feature = "dns")]
				servers: dns.servers,
				#[cfg(feature = "dns")]
				timeout: dns.timeout,
				#[cfg(feature = "dns")]
				search_domains: dns.search_domains,
				#[cfg(feature = "dns")]
				ndots: dns.ndots,
				#[cfg(feature = "dns")]
				hosts_file: dns.hosts_file,
				#[cfg(feature = "dns")]
				exempt_domains: dns.exempt_domains,
				#[cfg(feature = "dns")]
				serve_stale: dns.serve_stale,
				#[cfg(feature = "dns")]
				max_stale: dns.max_stale,
			}),
			flow_control: opts.flow_control.map(|flow| options::FlowControlOptions {
				stream_window: flow.stream_window,
				connection_window: flow.connection_window,
			}),
			headers: opts.headers.map(|headers| {
				headers
					.into_iter()
					.map(|header| options::Header {
						name: header.name,
						value: header.value,
						sensitive: header.sensitive,
					})
					.collect()
			}),
			http2: opts.http2.map(|http2| options::Http2Options {
				stream_window: http2.stream_window,
				connection_window: http2.connection_window,
				adaptive_window: http2.adaptive_window,
			}),
			http3: opts.http3.map(|http3| options::Http3Options {
				congestion: http3.congestion.map(|c| match c {
					Http3Congestion::Cubic => options::Http3Congestion::Cubic,
					Http3Congestion::Bbr1 => options::Http3Congestion::Bbr1,
				}),
				max_idle_timeout: http3.max_idle_timeout,
				upgrade_enabled: http3.upgrade_enabled,
				upgrade_probe: http3.upgrade_probe,
				upgrade_probe_timeout: http3.upgrade_probe_timeout,
				upgrade_slow_factor: http3.upgrade_slow_factor,
				upgrade_slow_ttl: http3.upgrade_slow_ttl,
				upgrade_advertised_ttl: http3.upgrade_advertised_ttl,
				upgrade_confirmed_ttl: http3.upgrade_confirmed_ttl,
				upgrade_failed_ttl: http3.upgrade_failed_ttl,
				upgrade_failed_max_ttl: http3.upgrade_failed_max_ttl,
				upgrade_cancel_strikes: http3.upgrade_cancel_strikes,
				upgrade_attempt_timeout: http3.upgrade_attempt_timeout,
				upgrade_follow_advertised_port: http3.upgrade_follow_advertised_port,
				upgrade_cache_capacity: http3.upgrade_cache_capacity,
				hints: http3.hints.map(|hints| {
					hints
						.into_iter()
						.map(|hint| options::Http3Hint {
							host: hint.host,
							port: hint.port,
						})
						.collect()
				}),
				stream_window: http3.stream_window,
				connection_window: http3.connection_window,
				send_window: http3.send_window,
			}),
			local_address: opts.local_address,
			pool: opts.pool.map(|pool| options::PoolOptions {
				idle_timeout: pool.idle_timeout,
				max_idle_per_host: pool.max_idle_per_host,
			}),
			quirks: opts.quirks.map(|quirks| options::QuirksOptions {
				h1_request_streaming: quirks.h1_request_streaming,
			}),
			redirect: opts.redirect.map(RedirectPolicy::from),
			timeout: opts.timeout.map(|timeout| options::TimeoutOptions {
				connect: timeout.connect,
				read: timeout.read,
				total: timeout.total,
			}),
			tls: opts.tls.map(|tls| options::TlsOptions {
				early_data: tls.early_data,
				// Either spelling of a PEM is the same bytes to the client.
				identity: tls.identity.map(|pem| pem_bytes(&pem)),
				required: tls.required,
				extra_roots: tls
					.extra_roots
					.map(|roots| roots.iter().map(pem_bytes).collect()),
			}),
			user_agent: opts.user_agent,
		}
	}
}

/// PEM input arrives as a buffer or a string; both are just the bytes.
fn pem_bytes(pem: &Either<Buffer, String>) -> Vec<u8> {
	match pem {
		Either::A(buf) => buf.to_vec(),
		Either::B(string) => string.as_bytes().to_vec(),
	}
}
