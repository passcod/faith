# React Native support

Work split out of the React Native investigation, to be picked up once the Android example is proven.

## Run the React Native example on an iOS simulator

Extend the React Native example to Apple platforms: build the `faith.apple.node` XCFramework for `aarch64-apple-ios` and `aarch64-apple-ios-sim` through ferric, and run the same test in an iOS simulator in CI.

Apple is the easier half of the two in one respect and the harder in another. Easier, because `rustls-platform-verifier` reaches SecTrust with no app-side wiring, so the Android trust-store problem has no counterpart, and because `react-native-node-api` publishes a prebuilt Hermes as a release asset keyed by the pinned commit, so `pod install` downloads it instead of compiling the engine. Harder, because the artifact is an XCFramework of `.framework` bundles rather than a flat directory of shared objects, and because CocoaPods and Xcode sit between the prebuild and the app.

Decide at the time whether Apple ships alongside Android in one package or separately, and whether the binary carries the same feature set.

## Widen the React Native build beyond a TLS backend

The first React Native build is the slim one: HTTP/1.1 and HTTP/2 over TLS with the platform trust store, and nothing else. Work through the remaining features and decide which belong in a mobile binary, measuring what each costs in size across the four Android ABIs.

Content decoding is the first candidate, because an undecoded response body is the loss a mobile connection feels soonest. The HTTP cache and the revocation machinery both want a writable directory, and `directories::BaseDirs` does not give a usable one inside an Android app sandbox, so each needs its path supplied by the caller before it can be turned on. Faith's own DNS client is the one to leave off longest: the system resolver is what consults Android's real configuration, and the DNS client would fall back to Google Public DNS instead. HTTP/3 and the cookie jar have no known obstacle beyond size.

## Publish the React Native package to npm

Nothing publishes `@passcod/faith-react-native`. The publish workflow builds and publishes the Node package and its platform packages, and knows nothing about the React Native one, so the prebuilt `.node` directory it needs is never built outside the test workflow and never shipped.

Two things have to be settled. The prebuild has to be produced for all four Android ABIs in the release build and carried into the package, which the test workflow already does for one ABI and can be extended to do for the rest. And the package depends on a version of `@passcod/faith` that carries `wrapper-factory.js`, so the two have to be released in step, with the dependency pinned to the version actually published rather than to a floor.
