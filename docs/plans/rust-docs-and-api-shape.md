# Rust docs and public API shape

The Rust crates' doc comments are LLM-generated and read like it. Cleaning them up is
inseparable from deciding what the public API actually is, so this covers both.

`web-faith` 1.0.0 is published and live on crates.io. Everything below is a breaking change to
a released API, so it ships as 1.0.1 with 1.0.0 yanked.

## House style

What a doc comment does:

- Says what the thing **is**, and where it sits. Extends a metaphor the reader can hold
  ("an agent is the browser instance").
- Sets the usage expectation ("a typical application has a single `Agent`"), which usually
  implies the mechanical facts for free.
- A small narrative title at the **main entry point only**. Everywhere else is reference tone;
  repeating the device gets gimmicky.

What it does not do:

- Explain what the signature and attributes already say: that a handle is cheap to clone, why
  `#[non_exhaustive]` is there, how Cargo features work.
- Narrate the reasoning that reached the design, or the alternatives rejected.
- Exist at all on an internal item with a clear name. `const MAX_REPLAYS: usize = 5;` is done.
- Use "X rather than Y", "not just A: it's B", elaborating appositives, "which is precisely why".

Load-bearing facts stay: measurements, RFC references, non-obvious constraints, and anything
a caller would otherwise get wrong.

## Phase 1 — public API shape (`web-faith`)

`lib.rs` exports thirteen modules, most of them plumbing. The crate root exports `FaithError`,
`FaithErrorKind`, `error_codes`, `USER_AGENT` — but not `Agent`, the entry point.

Everything `web-faith-napi` consumes must stay reachable. That is the whole constraint list:
`agent::Agent`, `client::RedirectPolicy`, `options::AgentOptions`, `stats::AgentStats`,
`timing::RequestTiming`, `request::{self, send, Credentials, RequestBody, RequestOptions}`,
`response::{FileDestination, FileProgress, FileWritten, Response, Trailers}`, `error_codes`,
`FaithErrorKind`, `USER_AGENT`.

**Re-export at the crate root:** `Agent`, `Response`, `Request`, `RequestBuilder`,
`FetchBuilder`. These are what a caller reaches for.

### Two new features

Both are **visibility switches, not compilation switches**. Every item below is always compiled
and always used internally; the feature only decides whether it is `pub` or `pub(crate)`.

**`raw-client`** (off by default, on for docs.rs) exposes `Agent::client()` and
`Agent::raw_client()`, which hand out the underlying `reqwest` client and the middleware-wrapped
one. Both are used internally regardless. Added to `package.metadata.docs.rs.features` so the
methods still render.

**`internals`** (off by default, *not* on for docs.rs) exposes what only `web-faith-napi` uses.
Documented as unstable and exempt from semver. `web-faith-napi` turns it on. Not in the docs.rs
feature list, so none of it renders there.

`internals` exposes: the `options` module, `AgentOptions` and its `into_options` terminal,
`Agent::from_options`, and `request::{send, RequestBody, RequestOptions}`.

### Mechanism

For a **module**, two cfg'd declarations of the same file:

```rust
#[cfg(feature = "internals")]
pub mod options;
#[cfg(not(feature = "internals"))]
mod options;
```

For an **inherent method**, keep the real body on a `pub(crate)` impl and add a cfg'd public
wrapper, so there is one body and one extra line:

```rust
#[cfg(feature = "internals")]
impl Agent {
    pub fn from_options(options: AgentOptions) -> Result<Self, FaithError> {
        Self::from_options_impl(options)
    }
}
```

For a **free item** (`request::send`) and a **type** (`RequestBody`, `RequestOptions`), a cfg'd
`pub use` beside a `pub(crate)` definition.

`options` is the awkward one regardless of mechanism. It cannot simply become private: `bon`
generates `AgentOptionsBuilder` from `AgentOptions`, and the builder's setters take
`CacheOptionsBuilder` and return `CacheOptions`, so every group type and group builder is
load-bearing for the ordinary `Agent::builder()` path and must stay publicly nameable whether or
not `internals` is on. So `builder` re-exports those — `AgentOptionsBuilder`, each
`*Options`/`*OptionsBuilder` pair, and the leaf types (`CacheStore`, `Http3Congestion`, `Header`,
`DnsOverride`, `Http3Hint`, `RedirectPolicy`) — and `internals` exposes the module path on top,
for napi's direct struct construction.

**Modules to make private:**

| Module | Disposition |
| --- | --- |
| `body` | Private. `Body`, `BodyHolder`, `DynStream`, `drain_body_inner` are pure plumbing; `ResponseBody` already lives in `response`. |
| `retry` | Private. `StaleAddressRetry` is internal middleware. |
| `warm_up` | Private. Argument-parsing helpers. |
| `integrity` | Private. SRI is applied through the `integrity` request option; nothing calls these directly. |
| `client` | Private. `RedirectPolicy` moves to `options` (and is re-exported from `builder`), which was the only thing keeping the module public. `install_https_sink`, `ClientRecipe`, `NodeEnv`, `DEFAULT_STREAM_WINDOW`, `DEFAULT_CONNECTION_WINDOW` go with it. |
| `options` | Module path public only under `internals`; its types stay reachable through `builder`. |

**Modules to keep public, with items demoted:**

| Module | Demote |
| --- | --- |
| `stats` | `InnerAgentStats` → private. It is `pub` and *not* `#[non_exhaustive]`, so adding a counter is a breaking change today — which cancels out `AgentStats` being non-exhaustive. |
| `timing` | Keep `RequestTiming`. Demote `TimingSlot`, `ArrivalStamp` and the stamp helpers. |
| `response` | Keep `Response`, `PeerInformation`, `FileDestination`, `FileProgress`, `FileWritten`, `Trailers`, `ResponseBody`. Demote `TrailersSlot`, `open_destination`, `classify_open_error`, `PROGRESS_INTERVAL`, and on `Response`: `check_stream_disturbed`, `ensure_stream`, `gather`, `gather_contiguous`. |
| `request` | Keep `Request`, `RequestBuilder`, `FetchBuilder`, `Priority`, `Credentials`, `Target`. `send`, `RequestBody`, `RequestOptions` public only under `internals`. Demote `QUERY` and `PRIORITY`. |
| `builder` | Becomes the home of the option types, per the re-export scheme. |
| `agent` | Keep. `client`/`raw_client` public only under `raw-client`; `from_options` only under `internals`. Decide per item on `dns_resolver`, `mark_warm`, `is_closed`. |

`web-faith-napi` gains `features = ["internals"]` on its `web-faith` dependency, and its imports
move from `web_faith::client::RedirectPolicy` to wherever that lands.

## Phase 2 — rewrite the public docs

Rewrite in house style, item by item, across what survives Phase 1. `Agent` gets the narrative
title; everything else is reference tone. Expected direction of travel: most items get shorter,
several lose their second paragraph entirely.

## Phase 3 — strip the internals

Delete doc comments on private items that only restate the name. Keep the ones carrying a real
fact (the `MAX_REPLAYS` measurement, the moka TTL-refresh behaviour in the Alt-Svc strike window,
the weak-reference cycle note in `H3HttpsSink`). Ordinary `//` comments explaining non-obvious
code stay — this is about doc comments on self-evident items.

## Phase 4 — component crates

The five are already shaped as standalone libraries and need less. Per crate:

- `web-faith-conn-tracker`: `query_tcp_stats` and `TrackedConnection` look internal; check.
- `web-faith-alt-svc`: the `record_*` surface is the store's API and stays, but the docs on
  `AltSvcCache` carry a lot of design narration.
- `web-faith-cookies`, `web-faith-dns`, `web-faith-encoding`: docs pass only, no shape changes
  expected.

Each keeps its crate-level `//!` as its landing page, since none of them ships a README.

## Not in scope

- `web-faith-napi`: its public docs are largely hand-written and MDN-derived on purpose.
- The main README and `crates/web-faith/README.md`: hand-written, already as intended.
- Any behaviour change. Visibility and docs only.
