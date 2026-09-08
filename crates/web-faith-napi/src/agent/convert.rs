//! Reading a JavaScript `AgentOptions` into the options the client validates.

#[cfg(feature = "cache")]
use http_cache_reqwest::CacheMode;
use std::{net::IpAddr, str::FromStr as _, time::Duration};

use napi::{Either, bindgen_prelude::Buffer};

use web_faith::{FaithError, FaithErrorKind, client::RedirectPolicy, options};

#[cfg(feature = "cookies")]
use web_faith_cookies::CookieLimits;

use crate::agent::AgentOptions;

#[cfg(feature = "http3")]
use crate::agent::Http3Congestion;

#[cfg(feature = "cache")]
use crate::agent::CacheStore;

/// Read the JavaScript options object into the shape the client validates.
///
/// A field-for-field mapping wherever the two agree, which is most of them; what differs is where
/// JavaScript expresses a choice as a union or a string that Rust has a type for.
///
/// Every option arrives as an `Option`, so the `maybe_` setters are what this reaches for: absent on
/// the JavaScript side means absent here, and the client settles the default. A group whose fields
/// sit behind a Cargo feature is filled in a `#[cfg]`-gated `let`, the builder's type changing with
/// each setter being what rules out a conditional call mid-chain.
impl TryFrom<AgentOptions> for options::AgentOptions {
	type Error = FaithError;

	fn try_from(opts: AgentOptions) -> Result<Self, Self::Error> {
		let builder = options::AgentOptions::builder()
			// JavaScript spells an address as a string; the client's option is an `IpAddr`, so this
			// is where a malformed one is refused rather than several frames later.
			.maybe_local_address(
				opts.local_address
					.as_deref()
					.map(|addr| {
						IpAddr::from_str(addr).map_err(|err| {
							FaithError::new(
								FaithErrorKind::AddressParse,
								Some(format!("{addr:?}: {err}")),
							)
						})
					})
					.transpose()?,
			)
			.maybe_redirect(opts.redirect.map(RedirectPolicy::from))
			.maybe_user_agent(opts.user_agent)
			.maybe_headers(opts.headers.map(|headers| {
				headers
					.into_iter()
					.map(|header| {
						options::Header::builder()
							.name(header.name)
							.value(header.value)
							.maybe_sensitive(header.sensitive)
							.build()
					})
					.collect::<Vec<_>>()
			}))
			.maybe_dns(opts.dns.map(|dns| move |_| dns_options(dns)))
			.maybe_flow_control(opts.flow_control.map(|flow| {
				move |_| {
					options::FlowControlOptions::builder()
						.maybe_stream_window(flow.stream_window)
						.maybe_connection_window(flow.connection_window)
						.build()
				}
			}))
			.maybe_http2(opts.http2.map(|http2| {
				move |_| {
					options::Http2Options::builder()
						.maybe_stream_window(http2.stream_window)
						.maybe_connection_window(http2.connection_window)
						.maybe_adaptive_window(http2.adaptive_window)
						.build()
				}
			}))
			.maybe_pool(opts.pool.map(|pool| {
				move |_| {
					options::PoolOptions::builder()
						.maybe_idle_timeout(
							pool.idle_timeout.map(|s| Duration::from_secs(s.into())),
						)
						.maybe_max_idle_per_host(pool.max_idle_per_host)
						.build()
				}
			}))
			.maybe_quirks(opts.quirks.map(|quirks| {
				move |_| {
					options::QuirksOptions::builder()
						.maybe_h1_request_streaming(quirks.h1_request_streaming)
						.build()
				}
			}))
			.maybe_timeout(opts.timeout.map(|timeout| {
				move |_| {
					options::TimeoutOptions::builder()
						.maybe_connect(timeout.connect.map(|ms| Duration::from_millis(ms.into())))
						.maybe_read(timeout.read.map(|ms| Duration::from_millis(ms.into())))
						.maybe_total(timeout.total.map(|ms| Duration::from_millis(ms.into())))
						.build()
				}
			}))
			.maybe_tls(opts.tls.map(|tls| {
				move |_| {
					options::TlsOptions::builder()
						.maybe_early_data(tls.early_data)
						.maybe_required(tls.required)
						// Either spelling of a PEM is the same bytes to the client.
						.maybe_identity(tls.identity.map(|pem| pem_bytes(&pem)))
						.maybe_extra_roots(
							tls.extra_roots
								.map(|roots| roots.iter().map(pem_bytes).collect::<Vec<_>>()),
						)
						.build()
				}
			}));

		#[cfg(feature = "cache")]
		let builder = builder.maybe_cache(opts.cache.map(|cache| {
			move |_| {
				options::CacheOptions::builder()
					.maybe_store(cache.store.map(|store| match store {
						CacheStore::Disk => options::CacheStore::Disk,
						CacheStore::Memory => options::CacheStore::Memory,
					}))
					.maybe_capacity(cache.capacity)
					.maybe_mode(cache.mode.map(CacheMode::from))
					.maybe_path(cache.path)
					.maybe_shared(cache.shared)
					.build()
			}
		}));

		// `false` and an absent value both mean no jar; `true` means one with default limits.
		#[cfg(feature = "cookies")]
		let builder = builder.maybe_cookies(match opts.cookies {
			None | Some(Either::A(false)) => None,
			Some(Either::A(true)) => Some(CookieLimits::default()),
			Some(Either::B(cookies)) => Some((&cookies).into()),
		});

		#[cfg(feature = "http3")]
		let builder = builder.maybe_http3(opts.http3.map(|http3| {
			move |_| {
				options::Http3Options::builder()
					.maybe_congestion(http3.congestion.map(|c| match c {
						Http3Congestion::Cubic => options::Http3Congestion::Cubic,
						Http3Congestion::Bbr1 => options::Http3Congestion::Bbr1,
					}))
					.maybe_max_idle_timeout(
						http3
							.max_idle_timeout
							.map(|s| Duration::from_secs(s.into())),
					)
					.maybe_upgrade_enabled(http3.upgrade_enabled)
					.maybe_upgrade_probe(http3.upgrade_probe)
					.maybe_upgrade_probe_timeout(
						http3
							.upgrade_probe_timeout
							.map(|ms| Duration::from_millis(ms.into())),
					)
					.maybe_upgrade_slow_factor(http3.upgrade_slow_factor)
					.maybe_upgrade_slow_ttl(
						http3
							.upgrade_slow_ttl
							.map(|s| Duration::from_secs(s.into())),
					)
					.maybe_upgrade_advertised_ttl(
						http3
							.upgrade_advertised_ttl
							.map(|s| Duration::from_secs(s.into())),
					)
					.maybe_upgrade_confirmed_ttl(
						http3
							.upgrade_confirmed_ttl
							.map(|s| Duration::from_secs(s.into())),
					)
					.maybe_upgrade_failed_ttl(
						http3
							.upgrade_failed_ttl
							.map(|s| Duration::from_secs(s.into())),
					)
					.maybe_upgrade_failed_max_ttl(
						http3
							.upgrade_failed_max_ttl
							.map(|s| Duration::from_secs(s.into())),
					)
					.maybe_upgrade_cancel_strikes(http3.upgrade_cancel_strikes)
					.maybe_upgrade_attempt_timeout(
						http3
							.upgrade_attempt_timeout
							.map(|ms| Duration::from_millis(ms.into())),
					)
					.maybe_upgrade_follow_advertised_port(http3.upgrade_follow_advertised_port)
					.maybe_upgrade_cache_capacity(http3.upgrade_cache_capacity)
					.maybe_hints(http3.hints.map(|hints| {
						hints
							.into_iter()
							.map(|hint| {
								options::Http3Hint::builder()
									.host(hint.host)
									.port(hint.port)
									.build()
							})
							.collect::<Vec<_>>()
					}))
					.maybe_stream_window(http3.stream_window)
					.maybe_connection_window(http3.connection_window)
					.maybe_send_window(http3.send_window)
					.build()
			}
		}));

		Ok(builder.into_options())
	}
}

/// The `dns` group, whose resolver settings sit behind the `dns` feature while `overrides` reaches
/// reqwest and so is honoured either way.
///
/// A group setter takes a closure over that group's builder, which suits a Rust caller filling one
/// by hand. This converter instead has the whole group in front of it, so every group here builds
/// directly and ignores the builder it is handed.
fn dns_options(dns: crate::agent::AgentDnsOptions) -> options::DnsOptions {
	let builder = options::DnsOptions::builder().maybe_overrides(dns.overrides.map(|overrides| {
		overrides
			.into_iter()
			.map(|o| {
				options::DnsOverride::builder()
					.domain(o.domain)
					.addresses(o.addresses)
					.build()
			})
			.collect::<Vec<_>>()
	}));

	#[cfg(feature = "dns")]
	let builder = builder
		.maybe_system(dns.system)
		.maybe_servers(dns.servers)
		.maybe_timeout(dns.timeout.map(|ms| Duration::from_millis(ms.into())))
		.maybe_search_domains(dns.search_domains)
		.maybe_ndots(dns.ndots)
		.maybe_hosts_file(dns.hosts_file)
		.maybe_exempt_domains(dns.exempt_domains)
		.maybe_serve_stale(dns.serve_stale)
		.maybe_max_stale(dns.max_stale.map(|ms| Duration::from_millis(ms.into())));

	builder.build()
}

/// PEM input arrives as a buffer or a string; both are just the bytes.
fn pem_bytes(pem: &Either<Buffer, String>) -> Vec<u8> {
	match pem {
		Either::A(buf) => buf.to_vec(),
		Either::B(string) => string.as_bytes().to_vec(),
	}
}
