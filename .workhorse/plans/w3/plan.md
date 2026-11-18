# React Native support

Investigation of whether Faith can run under React Native, and what a tested-in-CI example would cost.
Findings below were read off the sources on 2026-11-07 and corrected where a build disproved them; re-derive them before relying on them, because every component involved is young and pinned.

## The route exists: react-native-node-api

React Native does not run Node, so a Node-API addon has no host to link against.
`react-native-node-api` (Callstack, actively developed, last commit 2026-11-02) supplies one: it vendors Hermes from a pinned commit on its `static_h` branch, which carries Hermes' first-party Node-API implementation, and adds a TurboModule that dlopens prebuilt `.node` artifacts and registers them per `jsi::Runtime`.
Its sibling `ferric-cli` wraps Cargo and napi-rs to produce those artifacts from an annotated Rust crate, which is exactly Faith's shape.

Two consequences follow from the pinning.
React Native itself has not shipped a Hermes with Node-API, so the host package vendors one from a fork of Hermes that carries it.
`vendor-hermes` reads `sdks/.hermesversion` out of the installed React Native and clones the fork at the tag `node-api-<that version>`, so the React Native releases this works with are exactly the ones the fork has tagged.
The host package lists them in its `react-native` peer dependency: at 1.2.2 that is 0.79.1 through 0.81.5, stable releases rather than nightlies.
The example pins 0.81.5, the newest of them. The host package is explicitly "use at your own risk".

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
React Native Node-API loads the library with `dlopen` rather than `System.loadLibrary`, and its own `JNI_OnLoad` takes a `JavaVM` only to register its TurboModule and discards it, so neither is a route to the handles.
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
- The job is path-filtered: it runs when the React Native example, `web-faith-napi`, or the workflow itself changes, and not otherwise. The repository is public, so runner minutes are free and the filter is about keeping a long job off unrelated pull requests rather than about cost.
- Upstream drift is consequently not covered. A React Native release dropping out of the fork's tags, or the host package narrowing its peer dependency, will surface the next time one of the filtered paths changes, not when it happens. Accepted knowingly.
- Apple platforms are out of scope here and carried by their own card, to be picked up once Android is proven.
- The React Native build is the slim one: a TLS backend and nothing else, as `build:slim` already produces. Widening it is carded.
- The example carries its own test harness, whatever suits running inside an app, and the tape suite under `test/` is left alone. The two have different hosts and no reason to share a runner.
- The React Native build keeps the system trust store rather than falling back to static roots, so Android devices honour user-installed and enterprise roots and the OS's own distrust decisions.
- Integrating that is Faith's job, not the consuming app's, as far as it can be taken. React Native autolinking adds a package's `android/` Gradle module to the app build with no action from the consumer, which is what makes the Maven dependency on the Kotlin component and the Proguard keep rule something Faith can ship rather than document.

## Handing the JVM to the verifier

The Gradle module is needed whichever way the handles arrive, because the `org.rustls.platformverifier` classes have to be in the APK for the verifier to call at all.
What is open is how Faith obtains an `Env` and a `Context` at runtime, given that its library is dlopened by a TurboModule rather than loaded by the class loader.

Faith ships its own autolinked Android module and takes the integration on whole, so a consuming app adds the package and nothing else.
The prebuilt layout is jniLibs-shaped, so Faith's library is packed into the APK's native library directory like any other, and the module's Kotlin component loads it with `SoLoader` at app startup.
That runs `JNI_OnLoad` with the `JavaVM` in hand, before anything requires the addon from JavaScript and triggers the TurboModule's `dlopen`, and a native init declared on the Kotlin side passes the `Context` through.
Because the JNI entry point lives in Faith's own library alongside the napi one, no second shared object is needed.

Two things to prove against a real build: that the `SoLoader` load and the TurboModule's later `dlopen` coexist, and that the name the prebuild packs the library under is one `SoLoader` can ask for.

This machinery would shrink if `react-native-node-api` kept the `JavaVM` its own `JNI_OnLoad` already receives and offered it to addons, which is a few lines there and would serve every Rust addon reaching an Android system API.
Worth raising upstream once Faith's own integration proves what the interface needs to be.

## What slim leaves out, and what it buys

The slim build is HTTP/1.1 and HTTP/2 over TLS with the platform trust store, and no cookie jar, content decoding, HTTP cache, HTTP/3, revocation checking or Faith's own DNS client.
Dropping content decoding is the loss that will be felt first on a mobile connection.

Dropping the DNS client is a gain rather than a compromise here: resolution falls to the system resolver, which on Android consults the device's real configuration, so the silent fall back to Google Public DNS does not arise.
Connection tracking is absent from the slim build anyway, and would report nothing if it were present, because the tracker has no Android backend and leaves its statistics empty.

## ferric cannot express the slim build

`ferric build` invokes cargo with the target and the configuration and nothing else, so there is no way to pass `--no-default-features` or `--features` through it.
The packaging it does afterwards is not ferric's own: `createAndroidLibsDirectory` and its siblings are exported from the host package, which is what ferric itself imports.

So the prebuild is driven by a small script of Faith's own that calls cargo with the slim features and the NDK environment, then hands the artifacts to those same exported helpers.
Faith does not need ferric's other job either, generating TypeScript declarations, because the Node surface is already declared by hand.
A `--features` passthrough is worth raising upstream alongside the `JavaVM` one.

That script also has to set `CARGO_ENCODED_RUSTFLAGS` to link `weak-node-api`, which overrides the repository's `.cargo/config.toml` and so drops `--cfg reqwest_unstable`.
The slim build has no HTTP/3 and does not need it.

## Build steps

- [x] Split `dyn-symbols` out of `web-faith-napi`'s napi dependency into a feature of its own, inside `default`, so the React Native build can leave it off.
- [x] Expose a JNI entry point on the napi crate that hands an `Env` and a `Context` to `rustls-platform-verifier`. Kept on the napi crate rather than `web-faith`: the symbol has to be in the cdylib the JVM loads, and exposing `jni` types from a published crate's API would tie its semver to theirs. Not yet compiled, for want of an NDK.
- [x] Add the `react-native` package: the prebuild script, the JavaScript entrypoint, and `wrapper-factory.js`, which takes the binding and its environment as parameters so one wrapper serves both packages.
- [x] Add the autolinked Android module: the Gradle build with the Maven dependency on the verifier's Kotlin component, the consumer Proguard rule, and the Kotlin that loads the library and calls the initialiser.
- [x] Add the example app and its test harness, under `react-native/example`, reporting through mocha-remote as `react-native-node-api`'s own test app does.
- [x] Add the path-filtered workflow: the emulator run, a cross-compile job for the three architectures the emulator does not cover, and a check that the Gradle module's pinned verifier version matches `Cargo.lock`.

## Verifying the Android side

Everything above the Gradle module is verified: the three feature shapes resolve as intended, and the Node suite's 2229 tests pass over the wrapper that now takes its binding as a parameter.
Nothing below it is, because this machine carries no JDK, Android SDK or NDK, so the Rust Android target does not link, the Kotlin does not compile, and the example has never been bundled.

What a local loop needs, to be installed by hand:

- A JDK 17 distribution, which is what React Native's Gradle build expects.
- The Android SDK command-line tools, then through `sdkmanager`: `platform-tools`, `platforms;android-35`, `build-tools;35.0.0`, `ndk;27.1.12297006`, `cmake;3.22.1`, `emulator`, and `system-images;android-29;default;x86_64`.
- `ANDROID_HOME` exported to the SDK directory.
- An AVD on API 29 x86_64, and membership of the `kvm` group for hardware acceleration.
- The remaining Rust targets: `rustup target add x86_64-linux-android i686-linux-android armv7-linux-androideabi`.

The NDK version is the one the React Native community template pins, which the prebuild script defaults to and the workflow sets; a different one needs `FAITH_NDK_VERSION` and the workflow's `NDK_VERSION` moved together.

One local wrinkle to expect: this machine runs cargo under an `sccache` wrapper, and a first attempt at an Android target failed because sccache could not find the NDK compiler on `PATH`.
The prebuild script puts the NDK's bin directory on `PATH` and passes absolute compiler paths, which should settle it, but it is the first thing to suspect if a target fails to build locally and builds in CI.

## How the example resolves Faith

The React Native package needs `wrapper-factory.js`, which the published `@passcod/faith` does not carry: 1.0.1 is the latest release and predates the split. The dependency therefore names `>=1.0.2 <2` rather than a version that exists, so a consumer's install fails loudly instead of bundling a package with no factory in it, and both packages carry an npm `overrides` entry pointing at this checkout, which npm honours only for the package being installed as the root. That is what makes a plain `npm install` work in the package and in the example without reaching the registry for Faith.

Metro cannot see a path outside the project root unless it is in `watchFolders`, and the Node package is the repository root, so the root is watched. Cargo's `target` directory is inside it and runs to several gigabytes, so the directories that hold no JavaScript are in Metro's block list. Without that, every Metro start crawls the Rust build output.

## Where libfaith.so ends up

`react-native-node-api`'s own Gradle module is what adds the prebuild to the app, not Faith's. A `linkNodeApiModules` task runs its CLI over the project, collects every `*.android.node` directory carrying the marker file, and appends the results to its own `jniLibs.srcDirs`; the same module adds `weak-node-api`'s prebuilt libraries. So `libfaith.so` and `libweak-node-api.so` are both in the APK's library directory, which is what lets `System.loadLibrary("faith")` find it from the content provider, and what satisfies the `DT_NEEDED` entry the `--no-as-needed` link flag records.

Faith's own Gradle module therefore carries no native build at all. It is there for the Kotlin, the manifest entry, the Maven dependency on the verifier's Kotlin component, and the Proguard rule.

## How the example gets Gradle

Neither of the usual answers was right. Committing `gradle-wrapper.jar` puts a binary in a repository that has none, and asking everyone who touches the example to install Gradle means tracking whatever version React Native currently wants by hand.

So the example fetches Gradle when it needs it, in `scripts/ensure-gradle.mjs`, and writes the wrapper itself. The version is not pinned anywhere: `react-native-test-app` publishes the Gradle version each React Native release calls for, as a table its own Gradle code reads, and the script reads the same table against the installed React Native. The distribution is checked against the digest Gradle publishes beside it, and that digest is written into the generated wrapper, so every later `./gradlew` verifies what it downloads. Gradle itself is unpacked under `node_modules/.cache`, which is already ignored and goes with the install.

This replaces the workflow's `gradle wrapper` step. `gradle/actions/setup-gradle` stays, without a version, because what it is wanted for is caching Gradle's own dependency cache across runs.

> [!NOTE]
> The reason first given for not committing a wrapper was wrong, and worth recording so it is not reasoned from again: that the version it pinned would drift from what react-native-test-app wants. It would not. `configureGradleWrapper` rewrites `distributionUrl` in an existing wrapper on every `run-android`, which is why react-native-test-app's own example commits one. The objection that stands is only the binary.

## Why the package carries the wrapper rather than depending on it

The React Native package needs `wrapper-factory.js`, and the obvious way to get it is a dependency on the Node package. That cannot work here, for a reason that took a failed build to see: the Node package is the repository root, and the repository root contains the React Native package. `react-native-node-api` keys the app's dependencies by name and scans each one for prebuilds, so with both in the tree it finds `faith.android.node` twice, once under `@passcod/faith-react-native` and once under `@passcod/faith`, and refuses to link either with "Found conflicting library names". Nothing short of removing the dependency avoids it, since any `file:` reference to the root reproduces the containment.

So the prebuild script copies `wrapper-factory.js`, `wrapper.d.ts` and `index.d.ts` into the package beside the binary, and the package declares no dependencies at all. The copies are ignored by git, like the prebuild. This also ties the wrapper to the binary it was built against, and it removes the version problem that came with the dependency: no published Faith carries the factory, so the dependency could name no version that both existed and worked.

`index.d.ts` is the binding's generated declarations, which describe the whole binding rather than the slim build's subset. That was equally true when the types came from the Node package.

> [!NOTE]
> Two arrangements were tried and abandoned before this one, both defeated by npm. An `overrides` entry is refused when the same package is also a direct dependency, and a range of `>=1.0.2 <2` cannot be installed at all while no such version is published, which broke `npm install` in the package and so the prebuild step in CI.

## Known fragility in the emulator job

The job compiles React Native from source, which downloads Boost, Folly, glog, fmt and double-conversion from several hosts as part of the build. One such download failing takes the job with it: a run here failed on `archives.boost.io` not resolving, and succeeded on a retry minutes later with nothing changed. These are React Native's own Gradle tasks, so there is no retry to configure without patching them. A failure of this kind is worth re-running before being investigated.

## Two cmakes

Two things in this build compile native code and they do not agree on a cmake. React Native's own Android sources read `CMAKE_VERSION` and default to 3.30.5; `react-native-node-api` builds its host through the Android Gradle Plugin against the one the SDK carries, which is 3.22.1. A build with only 3.22.1 installed works, because Gradle fetches the other itself partway through, which is a download in the middle of an already long job. The workflow installs both up front instead.

Do not force `CMAKE_VERSION` to the older one to make them agree: React Native asks for the later version for its own reasons, and nothing here has a stake in which it uses.

## The architecture filter has to go through the environment

`react-native-test-app` pulls React Native in with `settings.includeBuild` when building it from source, and a project property set in the app's own `gradle.properties` does not cross into an included build. So `reactNativeArchitectures=x86_64` reached the app module and nothing else: React Native and Hermes both fell back to their default of all four architectures. A build here was seen configuring CMake for arm64-v8a, armeabi-v7a, x86 and x86_64 under hermes-engine while the app module configured x86_64 alone, which is the boundary showing itself.

That is Hermes compiled four times for an emulator that uses one, and on a job already budgeted at ninety minutes it is the largest single cost in it.

It is passed as `ORG_GRADLE_PROJECT_reactNativeArchitectures` instead, which every build in the composite reads from its own environment. A dry run with it set configures only x86_64. `-P` on the command line does not work for this, and neither does the properties file, which is why `--active-arch-only` does not cover it either: that flag passes `-P`, so it filters the app module and leaves React Native building everything.

The example's `android` script sets it, defaulting to x86_64 and overridable with `FAITH_ANDROID_ABI` for anyone running on a real device.

## Node-API 8 is the ceiling, and the reason is not arbitrary

The first build that reached the device failed to load it: `dlopen failed: cannot locate symbol "node_api_create_syntax_error"`. Faith references 66 Node-API symbols and weak-node-api defines 145, so this was one symbol out of 66, and worth measuring rather than guessing at.

The line it falls on is exact. Node-API functions named `node_api_` rather than `napi_` are Node's own extensions, which a host is not obliged to implement, and Node-API 9 is where they begin. The host defines all four symbols Node-API 8 adds and none of the four Node-API 9 adds. So 8 is not a cautious choice below some working value: it is the whole standard surface, and 9 is the first version that asks for something only Node has.

The version is therefore a feature of `web-faith-napi` rather than a setting of the workspace dependency, as `dyn-symbols` already was. One of them has to be on, because with no version feature only Node-API 1 exists and napi's own code does not compile against it.

It also reached Faith's code. `JsSyntaxError` is napi's wrapper over that very symbol, so below Node-API 9 the error is built by hand with its `name` set instead, which reads as a SyntaxError without being an instance of one. The error mapping already did this for the error kinds it calls named errors, so it was the existing path rather than a new one.

> [!NOTE]
> This is the one failure of the card that could not have been found by reading. The symbol is referenced from napi's generated bindings, the binary links cleanly because every Node-API symbol is undefined by design, and nothing resolves until the host loads it on a device.

## What actually blocks this: Hermes has no threadsafe functions

The addon loads. The log reads `Loaded addon`, then `Found napi_register_module_v1`, and then the process takes `SIGABRT` inside `napi_create_threadsafe_function` in `libweak-node-api.so`.

That library is a table of stubs, each of which aborts when the host has injected no implementation for it, with "called before it was injected". The host injects what it has. Hermes' Node-API defines 115 functions and `react-native-node-api`'s own host adds 17 more, including all five Buffer functions and the async-work queue, which Hermes lacks. Between them they still leave six that Faith calls:

- `napi_create_threadsafe_function`, `napi_call_threadsafe_function`, `napi_release_threadsafe_function`, `napi_unref_threadsafe_function`
- `napi_fatal_exception`
- `napi_add_env_cleanup_hook`

The four threadsafe functions are the whole of it. Faith's asynchrony is a Tokio runtime on its own threads settling promises on the JavaScript thread, and a threadsafe function is the only Node-API mechanism for calling into a JavaScript thread from a thread that does not own it. There is no way to express Faith's model without one, and no feature of Faith's that can be turned off to avoid it: a single `fetch` needs it. `napi_fatal_exception` and `napi_add_env_cleanup_hook` would be easier to live without, and are moot while the first four are missing.

So React Native support is blocked upstream, on Hermes implementing threadsafe functions or on the host package implementing them over Hermes' runtime as it already does for buffers and async work. Nothing in this repository can work around it.

> [!NOTE]
> Everything else on this card works, and the work stands: the prebuild builds for four architectures, the autolinked Gradle module puts Faith in an app with no consumer action, the content provider's library load and the host's later `dlopen` of the same library coexist, the wrapper bundles under Metro, and the app reaches the point of registering the module. The gap is one upstream capability, found at the last step before the tests would have run.
