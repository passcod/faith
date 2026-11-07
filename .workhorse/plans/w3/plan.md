# React Native support

Investigation of whether Faith can run under React Native, and what a tested-in-CI example would cost.
Findings below were read off the sources on 2026-11-07; re-derive them before relying on them, because every component involved is pre-1.0 or pinned to a nightly.

## The route exists: react-native-node-api

React Native does not run Node, so a Node-API addon has no host to link against.
`react-native-node-api` (Callstack, actively developed, last commit 2026-11-02) supplies one: it vendors Hermes from a pinned commit on its `static_h` branch, which carries Hermes' first-party Node-API implementation, and adds a TurboModule that dlopens prebuilt `.node` artifacts and registers them per `jsi::Runtime`.
Its sibling `ferric-cli` wraps Cargo and napi-rs to produce those artifacts from an annotated Rust crate, which is exactly Faith's shape.

Two consequences follow from the pinning.
React Native itself has not shipped a Hermes with Node-API, so the host package's `react-native` peer dependency is a single nightly (`0.88.0-nightly-20260809-db662caea` at time of reading).
Anything built on this is pinned to that nightly until React Native ships Node-API in a release, and the host package is explicitly "use at your own risk".

Prebuilt layout is not npm-platform-packages: Android wants a jniLibs-shaped `faith.android.node/` directory with a `react-native-node-api-module` marker file, Apple wants an XCFramework of `.framework` bundles named `faith.apple.node/`.
Supported targets cover the four Android ABIs, iOS device and simulator, and also macOS, tvOS and visionOS.

## What the napi binding has to change

`napi`'s default features are `napi4` and `dyn-symbols`.
`dyn-symbols` makes napi-rs resolve Node-API functions in the host process at runtime, which does not work under React Native; `ferric build` refuses to build a crate whose resolved `napi-sys` features include it.
The React Native build therefore needs `default-features = false` on `napi`, which means `web-faith-napi` carries a feature that re-enables `dyn-symbols` and sits in its own `default`, the way `build:slim` already opts features out.

The other check is a Node-API version ceiling of 10.
Faith asks for `napi9`, and `serde-json`, `tokio_rt` and `web_stream` each resolve no higher than `napi4`, so there is nothing to change there.

## Where Faith's stack meets the React Native runtime badly

**`ReadableStream` is not a Hermes global.**
napi-rs constructs a readable stream by reading `ReadableStream` off the JS global object, so `response.body` fails outright in a bare React Native runtime.
Every body accessor that does not stream (`text()`, `json()`, `arrayBuffer()`) is unaffected, so this bounds what an example can exercise rather than blocking one.
`wrapper.js` also reaches for `Blob`, `FormData` and `node:url`'s `fileURLToPath`, none of which React Native supplies in the form the wrapper expects.

**The Android trust store needs app-side wiring.**
`rustls-platform-verifier` calls into the JVM to reach Android's trust manager, which requires a Kotlin component added to the app's Gradle build, a Proguard keep rule, and a one-time `init_with_env` call handing over a `JavaVM` and a `Context`.
React Native Node-API loads the library with `dlopen` rather than `System.loadLibrary`, so `JNI_OnLoad` is not a route to the handles either.
A prebuilt Faith is therefore not drop-in on Android; either the consuming app does this setup, or the React Native Android build trades the system trust store for static roots.
Faith already refuses extra roots on Android for the related reason that the platform verifier has nowhere to put them.

**The system resolver is not readable on Android.**
Faith's DNS client configures itself from the system resolver configuration and falls back to Google Public DNS when that cannot be read.
Android has no `resolv.conf`, so that fallback is what an Android build would silently take, ignoring the carrier, Wi-Fi and VPN resolvers the device is actually configured with.
That is a correctness and privacy problem on exactly the platform where VPN and captive-portal resolvers matter most.

**Binary size is a product concern, not an engineering detail.**
An app ships four Android ABIs and a fat iOS binary, and the default feature set carries aws-lc-rs, quinn, hickory, four decompressors, a disk cache and the revocation machinery.
`--no-default-features --features tls-aws-lc-rs` already exists as `build:slim` and is the natural shape for a mobile build; it also sidesteps the resolver problem and the disk-cache path assumptions, though not the Android trust store, which lives in the TLS layer.

## The thing most worth proving

Faith is asynchronous to its core: a Tokio runtime on background threads resolving promises on the JS thread through napi-rs threadsafe functions.
Whether that holds up against Hermes' Node-API implementation and React Native's single JS thread is the question an example answers that reading cannot.
Compiling for the targets proves much less, and Android cross-compilation already passes in `publish.yml` today without anyone having run the result.

## The Android emulator route, as react-native-node-api runs it

The decided bar is that the example runs on an Android emulator, which is what proves the Tokio-to-JS-thread path.
`react-native-node-api`'s own Android job is the reference for what that costs, and it is instructive about where the cost sits.

The emulator needs one ABI only: their job builds `x86_64-linux-android` alone for the device, and the other three ABIs only to check they compile.

Hermes is the expensive part, and Android has no shortcut.
Apple platforms get a prebuilt Hermes published as a release asset keyed by the pinned commit, so `pod install` downloads it; there is no Android equivalent, so `vendor-hermes` shallow-clones Hermes at the pinned SHA and Gradle compiles it with the NDK as part of the app build.
Their emulator step carries a 75-minute timeout and the whole job is gated behind a pull request label rather than running on every change, with their self-hosted runner currently offline and the job disabled on `main`.
Caching the Gradle and `.cxx` output keyed on the pinned Hermes SHA and NDK version would make warm runs cheap; a cold run will not be.

The harness is `mocha-remote`: the app connects back over `adb reverse tcp:8090` and streams results to a host process that owns the exit code.
Faith's suite is tape, so the example either adopts mocha-remote for its own tests or reports results back over the same socket pattern with something smaller.

## Decided

- The example runs on an Android emulator, not merely cross-compiles. Compiling already passes in `publish.yml` for two Android ABIs and proves nothing about Hermes.

## Open

- How often the Android job runs, given the cold-build cost. See the discussion on the card.
