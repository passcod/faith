# web-faith

A browser-shaped HTTP client.

[![crates.io](https://img.shields.io/crates/v/web-faith.svg)](https://crates.io/crates/web-faith)
[![docs.rs](https://docs.rs/web-faith/badge.svg)](https://docs.rs/web-faith)

Faith behaves like a browser ("faithfully") wherever that translates to a server-side runtime: transparent HTTP/2
and HTTP/3 upgrades, Happy Eyeballs across IPv4 and IPv6, DNS caching, an optional cookie jar, and HTTP
caching. We also publish the reusable components as separate crates.

## Usage

```toml
[dependencies]
web-faith = "1.0"
```

```rust
use web_faith::{FaithError, agent::Agent};

#[tokio::main]
async fn main() -> Result<(), FaithError> {
	let agent = Agent::new()?;
	let body = agent.fetch("https://example.com/").await?.text().await?;
	println!("{body}");
	Ok(())
}
```

## HTTP/3 is opt-in

Faith uses reqwest internally, and its HTTP/3 support is currently unstable. To enable HTTP/3 support, you will need to set the `http3` feature on Faith, and use the `reqwest_unstable` rustc cfg flag:

```toml
[dependencies]
web-faith = { version = "1.0", features = ["http3"] }
```

```toml
# .cargo/config.toml
[build]
rustflags = ["--cfg", "reqwest_unstable"]
```

## Features

| Feature | Default | What it adds |
| --- | :-: | --- |
| `cache` | ✓ | The HTTP cache. |
| `connection-tracking` | ✓ | Kernel connection counters. |
| `cookies` | ✓ | The cookie jar. |
| `dns` | ✓ | Faith's own caching resolver. Without it, names resolve through the platform. |
| `encoding` | ✓ | Content codings for request and response bodies. |
| `tls-aws-lc-rs` | ✓ | aws-lc-rs as the rustls crypto provider. |
| `tls-ring` |  | ring as the rustls crypto provider instead. |
| `http3` |  | Transparent HTTP/3, upgraded into via Alt-Svc. Needs the cfg flag above. |
| `raw-client` |  | Access to the reqwest client underneath. |
| `internals` |  | Faith's internals. Permanently unstable and exempt from semver. |

## Component crates

- [`web-faith-cookies`](https://docs.rs/web-faith-cookies)
- [`web-faith-dns`](https://docs.rs/web-faith-dns)
- [`web-faith-conn-tracker`](https://docs.rs/web-faith-conn-tracker)
- [`web-faith-alt-svc`](https://docs.rs/web-faith-alt-svc)
- [`web-faith-encoding`](https://docs.rs/web-faith-encoding)

## Elsewhere

Faith is also a Node.js module which lets you use this Rust networking stack as a `fetch` drop-in replacement: [`@passcod/faith`](https://www.npmjs.com/package/@passcod/faith).

## Licence

Apache-2.0 or MIT.
