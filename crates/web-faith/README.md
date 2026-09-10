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

An `Agent` owns the connection pool, resolver, cookie jar, and caches, and `Agent::builder`
configures one. Cloning an agent is cheap and every clone names the same one. `Agent::fetch`
returns a builder that sends when awaited, so there is no separate send step, and `Request`
prepares one without sending it.

Whichever layer a request fails in, the failure arrives as one `FaithError` whose `FaithErrorKind`
is the stable code to match on.

## HTTP/3 is opt-in

Everything else is on by default, so the snippet above builds as it stands. HTTP/3 is the exception:
it turns on reqwest's own `http3` feature, which reqwest treats as unstable and refuses to compile
unless the build sets a cfg flag. Enabling it takes both the feature and the flag.

```toml
[dependencies]
web-faith = { version = "1.0", features = ["http3"] }
```

```toml
# .cargo/config.toml
[build]
rustflags = ["--cfg", "reqwest_unstable"]
```

Or per invocation, `RUSTFLAGS='--cfg reqwest_unstable' cargo build`. Without the flag, reqwest stops
the build and says so. Requests still negotiate HTTP/2 without the feature; what it adds is HTTP/3
and the Alt-Svc machinery that upgrades an origin to it.

## Features

All on by default except `http3`.

| Feature | Default | What it adds |
| --- | --- | --- |
| `cache` | on | The HTTP cache, its store, and the per-request cache mode. |
| `connection-tracking` | on | Per-connection kernel counters, and the agent verb that reports them. |
| `cookies` | on | The cookie jar, and the agent option and handle that reach it. |
| `dns` | on | Faith's own caching resolver. Without it, names resolve through the platform. |
| `encoding` | on | Content codings: negotiating and decoding a response body, and compressing a request one. |
| `tls-aws-lc-rs` | on | The rustls crypto provider. `tls-ring` selects ring instead. |
| `http3` | off | Transparent HTTP/3, and the Alt-Svc machinery that upgrades an origin to it. Needs the cfg flag above. |

Turning one off drops the code behind it, and the parts of the API that only mean something with
that subsystem present go with it.

## The component crates

Each one is useful without the client above it, and none of them depends on `web-faith`:

- [`web-faith-cookies`](https://docs.rs/web-faith-cookies): the cookie jar, with the rules that
  hold outside a browser.
- [`web-faith-dns`](https://docs.rs/web-faith-dns): the resolver, its cache, the discovery ladder,
  the `HTTPS` record query, and Happy Eyeballs.
- [`web-faith-conn-tracker`](https://docs.rs/web-faith-conn-tracker): live per-connection
  statistics, read from the operating system.
- [`web-faith-alt-svc`](https://docs.rs/web-faith-alt-svc): the Alt-Svc store and the HTTP/3
  upgrade machinery.
- [`web-faith-encoding`](https://docs.rs/web-faith-encoding): content coding for request and
  response bodies.

The same stack ships to Node.js as [`@passcod/faith`](https://www.npmjs.com/package/@passcod/faith).

## Minimum supported Rust version

1.96, built and tested in CI alongside stable.

## Licence

Apache-2.0 OR MIT, at your option.
