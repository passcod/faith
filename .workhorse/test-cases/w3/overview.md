# React Native support

Scenarios that verify Faith builds for Android and works inside a React Native app.
The emulator cases run in CI; `*.node` is gitignored, so the prebuild is built before any of them.

## The napi feature split

- [x] The Node default build resolves `napi-sys` with `dyn-symbols` enabled.
- [x] The React Native build resolves `napi-sys` without `dyn-symbols`, which is what `ferric build` refuses and the host cannot link.
- [x] The React Native build resolves no `napiN` feature above 10, the highest the host implements.
- [x] The slim Node build keeps `dyn-symbols`, so `test:slim` still has a host to resolve against.

## The wrapper over two bindings

- [x] The Node suite passes with the wrapper taking its binding as a parameter rather than requiring it.
- [ ] The React Native entrypoint reaches no Node builtin, so Metro resolves the bundle without a polyfill.
- [ ] `toFile()` accepts a `file:` URL under React Native and refuses one carrying an encoded separator.

## The prebuild

- [x] The build script produces a `faith.android.node` directory holding one `libfaith.so` per requested ABI.
- [x] The directory carries the `react-native-node-api-module` marker, without which the host does not auto-link it.
- [x] The library is named `libfaith.so`, which is the name `System.loadLibrary("faith")` asks for.
- [x] Building for one ABI alone succeeds, which is what the emulator job does.

## The Android trust store

- [x] The Gradle module's pinned `rustls-platform-verifier` version matches the `rustls-platform-verifier-android` crate in `Cargo.lock`. Verifies spec: TLS#trust-roots
- [ ] An app that lists Faith as a dependency gets the content provider through manifest merging, with nothing added to the app.
- [ ] `System.loadLibrary` at provider creation and the host's later `dlopen` of the same library coexist.
- [ ] An HTTPS request to a host trusted by the device's own store succeeds, so the verifier received its handles. Verifies spec: TLS#trust-roots
- [ ] An HTTPS request to a host whose certificate the device does not trust is refused rather than accepted.

## Running in the app

- [ ] A plain GET resolves and `text()` returns the body, which is the Tokio-to-JS-thread path the card exists to prove.
- [ ] Several concurrent requests all resolve, so the threadsafe-function path is not serialised by accident.
- [ ] An aborted request rejects with the abort error rather than hanging.
- [ ] A request to an unreachable host rejects with a Faith error carrying a `code`.
