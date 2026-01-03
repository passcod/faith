#!/usr/bin/env -S cargo +nightly -Zscript
---cargo
[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "=0.12.25", features = [
    "brotli",
    "cookies",
    "deflate",
    "gzip",
    "hickory-dns",
    "http2",
    "json",
    "rustls-tls-native-roots-no-provider",
    "rustls-tls-webpki-roots",
    "stream",
    "system-proxy",
    "zstd",
], git = "https://github.com/passcod/reqwest", branch = "v0.12.25-sslkeylogfile" }

[profile.dev]
opt-level = 3
debug-assertions = false
overflow-checks = false
debug = false
lto = true
incremental = false
codegen-units = 16
---

#[tokio::main]
async fn main() {
	let Ok(target) = std::env::var("TARGET") else {
		return;
	};

	let hits = std::env::var("HITS").unwrap();
	let hits: usize = hits.parse().unwrap();

	let client = reqwest::Client::builder()
		.tls_sslkeylogfile(true)
		.build()
		.unwrap();

	for n in 0..hits {
		match client.get(&target).send().await {
			Err(err) => {
				println!("{n}: {err}");
			}
			Ok(resp) => {
				let _ = resp.bytes_stream();
			}
		}
	}
}
