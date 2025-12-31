#!/usr/bin/env -S cargo +nightly -Zscript
---cargo
[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.13", features = ["hickory-dns"], git = "https://github.com/passcod/reqwest", branch = "feat-2676-sslkeylogfile" }
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
				let _ = resp.bytes().await;
			}
		}
	}
}
