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
| --- | --- | --- |
| `cache` | on | The HTTP cache, its store, and the per-request cache mode. |
| `connection-tracking` | on | Per-connection kernel counters, and the agent verb that reports them. |
| `cookies` | on | The cookie jar, and the agent option and handle that reach it. |
| `dns` | on | Faith's own caching resolver. Without it, names resolve through the platform. |
| `encoding` | on | Content codings: negotiating and decoding a response body, and compressing a request one. |
| `tls-aws-lc-rs` | on | The rustls crypto provider. `tls-ring` selects ring instead. |
| `http3` | off | Transparent HTTP/3, and the Alt-Svc machinery that upgrades an origin to it. Needs the cfg flag above. |

## Component crates

- [`web-faith-cookies`](https://docs.rs/web-faith-cookies)
- [`web-faith-dns`](https://docs.rs/web-faith-dns)
- [`web-faith-conn-tracker`](https://docs.rs/web-faith-conn-tracker):
- [`web-faith-alt-svc`](https://docs.rs/web-faith-alt-svc)
- [`web-faith-encoding`](https://docs.rs/web-faith-encoding)

The same stack ships to Node.js as [`@passcod/faith`](https://www.npmjs.com/package/@passcod/faith).

## Minimum supported Rust version

1.96, built and tested in CI alongside stable.

## Licence

Apache-2.0 OR MIT, at your option.
