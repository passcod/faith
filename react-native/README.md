# Faith for React Native

[Faith](https://github.com/passcod/faith) is a fetch implementation backed by a Rust
networking stack. This package is the React Native build of it, for Android.

> [!CAUTION]
> This does not work yet, and cannot be made to work from here. Hermes' Node-API has no
> threadsafe functions, and a threadsafe function is the only way Node-API offers to call
> into the JavaScript thread from a thread that does not own it. Faith answers every request
> from a Tokio runtime on its own threads, so a single `fetch` needs one. The app builds and
> starts, the addon loads, and the process aborts inside `napi_create_threadsafe_function`
> the moment the module registers.
>
> What unblocks it is Hermes implementing those functions, or
> [`react-native-node-api`](https://github.com/callstackincubator/react-native-node-api)
> implementing them over Hermes' runtime as it already does for buffers and async work.
> Everything else here is built and working, and is waiting on that.
>
> This package is not on npm, and will not be published before it runs.

> [!NOTE]
> React Native has not shipped a Hermes with Node-API at all, so this rests on the host
> package vendoring one from a fork that carries it. That fork publishes a patched tag per
> React Native release, so the versions this builds against are the ones it has tagged,
> which the host package lists in its peer dependency.

## Installing

```sh
npm install @passcod/faith-react-native react-native-node-api
```

Set `react-native-node-api` up as its own README describes, which includes adding its Babel
plugin, needed by any Node-API addon:

```js
// babel.config.js
module.exports = {
	presets: ["module:@react-native/babel-preset"],
	plugins: ["module:react-native-node-api/babel-plugin"],
};
```

Faith itself needs nothing else. Its Gradle module is auto-linked, which puts the Android
trust manager's Kotlin component on the classpath, keeps Proguard from stripping it, and
runs the initialiser that hands the JVM handles to the certificate verifier.

## Using

```js
import { fetch } from "@passcod/faith-react-native";

const response = await fetch("https://example.com/");
console.log(await response.text());
```

The surface is the Node package's, so anything written against it reads the same here.

## What this build carries

HTTP/1.1 and HTTP/2 over TLS, verified against the device's own trust store, so
user-installed and enterprise roots and the operating system's distrust decisions all
apply.

Left out, to keep four architectures' worth of binary off an app: the cookie jar, content
decoding, the HTTP cache, HTTP/3, revocation checking, and Faith's own DNS client. Dropping
the DNS client is deliberate beyond size — resolution falls to the system resolver, which
is the one that reads the device's real configuration, including a VPN's.

`response.body` needs a `ReadableStream` in the global scope, which React Native does not
provide. `text()`, `json()`, `arrayBuffer()` and `toFile()` are unaffected.

## Building the prebuild

The binary is not in the repository. Build it with `ANDROID_HOME` set and the NDK
installed:

```sh
npm install
npm run build:android                         # every architecture
npm run build:android -- x86_64-linux-android # just one
```

That writes `faith.android.node`, which `react-native-node-api` finds and adds to the app's
native libraries. A prebuild directory names exactly the architectures it was built with, so
naming a subset replaces what was there rather than adding to it: to end up with all four,
build all four in one go. The build takes the wrapper from the Node package in the same checkout,
which is what the `overrides` entry in this package's manifest resolves.

## Licence

Apache-2.0 OR MIT, as Faith.
