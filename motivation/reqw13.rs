#!/usr/bin/env -S cargo +nightly -Zscript
---cargo
[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.13", features = ["hickory-dns", "stream", "http3"], git = "https://github.com/passcod/reqwest", rev = "8d07893" }

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
		.http3_prior_knowledge()
		.build()
		.unwrap();

	for n in 0..hits {
		let req = client.request(Default::default(), &target).version(reqwest::Version::HTTP_3);
		match req.send().await {
			Err(err) => {
				println!("{n}: {err:?}");
			}
			Ok(resp) => {
				println!("{:?}", resp.version());
				let _ = resp.bytes_stream();
			}
		}
	}
}
