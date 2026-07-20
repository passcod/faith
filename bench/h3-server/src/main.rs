//! Minimal HTTP/3 static server for the benchmark suite.
//!
//! Node has no HTTP/3 server, so the bench harness spawns this instead. It
//! serves the same routes as bench/lib/servers.mjs:
//!
//!   /payload/<bytes>              deterministic body of exactly <bytes>
//!   /delay/<ms>/payload/<bytes>   wait <ms> before responding
//!   /cc/<maxage>/payload/<bytes>  same, with Cache-Control: public, max-age
//!
//! Usage: h3-server --cert cert.pem --key key.pem [--port 0]
//! Prints "LISTENING <port>" on stdout once ready.

use std::{collections::HashMap, fs, net::SocketAddr, sync::Arc, time::Duration};

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use quinn::crypto::rustls::QuicServerConfig;

fn payload(bytes: usize, cache: &mut HashMap<usize, Bytes>) -> Bytes {
	cache
		.entry(bytes)
		.or_insert_with(|| {
			// same xorshift generator as bench/lib/servers.mjs, so h3 bodies
			// are byte-identical to the h1/h2 ones
			let mut buf = vec![0u8; bytes];
			let mut seed: u32 = 0x2545f491;
			for b in buf.iter_mut() {
				seed ^= seed << 13;
				seed ^= seed >> 17;
				seed ^= seed << 5;
				*b = (seed & 0xff) as u8;
			}
			Bytes::from(buf)
		})
		.clone()
}

struct Route {
	delay_ms: u64,
	bytes: usize,
	max_age: Option<u64>,
}

fn parse_route(path: &str) -> Option<Route> {
	let parts: Vec<&str> = path
		.split('?')
		.next()?
		.split('/')
		.filter(|p| !p.is_empty())
		.collect();
	let mut delay_ms = 0;
	let mut max_age = None;
	let mut i = 0;
	if parts.get(i) == Some(&"cc") {
		max_age = Some(parts.get(i + 1)?.parse().ok()?);
		i += 2;
	}
	if parts.get(i) == Some(&"delay") {
		delay_ms = parts.get(i + 1)?.parse().ok()?;
		i += 2;
	}
	if parts.get(i) != Some(&"payload") {
		return None;
	}
	let bytes: usize = parts.get(i + 1)?.parse().ok()?;
	if bytes > 512 * 1024 * 1024 {
		return None;
	}
	Some(Route {
		delay_ms,
		bytes,
		max_age,
	})
}

fn arg(name: &str) -> Option<String> {
	let mut args = std::env::args();
	while let Some(a) = args.next() {
		if a == name {
			return args.next();
		}
	}
	None
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let cert_path = arg("--cert").expect("--cert required");
	let key_path = arg("--key").expect("--key required");
	let port: u16 = arg("--port").map_or(0, |p| p.parse().expect("invalid --port"));
	let host = arg("--host").unwrap_or_else(|| "127.0.0.1".to_string());

	let certs = rustls_pemfile::certs(&mut fs::read(&cert_path)?.as_slice())
		.collect::<Result<Vec<_>, _>>()?;
	let key = rustls_pemfile::private_key(&mut fs::read(&key_path)?.as_slice())?
		.expect("no private key found");

	let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(
		rustls::crypto::aws_lc_rs::default_provider(),
	))
	.with_protocol_versions(&[&rustls::version::TLS13])?
	.with_no_client_auth()
	.with_single_cert(certs, key)?;
	tls.alpn_protocols = vec![b"h3".to_vec()];
	tls.max_early_data_size = u32::MAX;

	let server_config =
		quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(tls)?));
	let addr: SocketAddr = format!("{host}:{port}").parse()?;
	let endpoint = quinn::Endpoint::server(server_config, addr)?;

	println!("LISTENING {}", endpoint.local_addr()?.port());

	while let Some(incoming) = endpoint.accept().await {
		tokio::spawn(async move {
			let Ok(conn) = incoming.await else { return };
			let Ok(mut h3_conn) =
				h3::server::Connection::new(h3_quinn::Connection::new(conn)).await
			else {
				return;
			};
			let mut payloads = HashMap::new();
			loop {
				match h3_conn.accept().await {
					Ok(Some(resolver)) => {
						let Ok((req, stream)) = resolver.resolve_request().await else {
							continue;
						};
						let body = parse_route(req.uri().path())
							.map(|r| (payload(r.bytes, &mut payloads), r));
						tokio::spawn(handle(req, stream, body));
					}
					Ok(None) => break,
					Err(_) => break,
				}
			}
		});
	}

	Ok(())
}

async fn handle<S>(
	_req: Request<()>,
	mut stream: h3::server::RequestStream<S, Bytes>,
	body: Option<(Bytes, Route)>,
) where
	S: h3::quic::BidiStream<Bytes>,
{
	let result: Result<(), Box<dyn std::error::Error>> = async {
		match body {
			None => {
				let resp = Response::builder().status(StatusCode::NOT_FOUND).body(())?;
				stream.send_response(resp).await?;
			}
			Some((data, route)) => {
				if route.delay_ms > 0 {
					tokio::time::sleep(Duration::from_millis(route.delay_ms)).await;
				}
				let mut resp = Response::builder()
					.status(StatusCode::OK)
					.header("content-type", "application/octet-stream")
					.header("content-length", data.len());
				resp = if let Some(max_age) = route.max_age {
					resp.header("cache-control", format!("public, max-age={max_age}"))
				} else {
					resp.header("cache-control", "no-store")
				};
				stream.send_response(resp.body(())?).await?;
				stream.send_data(data).await?;
			}
		}
		stream.finish().await?;
		Ok(())
	}
	.await;
	let _ = result;
}
