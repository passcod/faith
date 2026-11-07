# Faith for React Native

[Faith](https://github.com/passcod/faith) is a fetch implementation backed by a Rust
networking stack. This package is the React Native build of it, for Android.

> [!WARNING]
> React Native has not shipped a Hermes with Node-API, so this rests on
> [`react-native-node-api`](https://github.com/callstackincubator/react-native-node-api)
> vendoring one from a pinned commit, and on the single React Native nightly that package
> builds against. Treat it as experimental until React Native ships Node-API itself.

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

Published releases carry it. To build from a checkout, with `ANDROID_HOME` set and the NDK
installed:

```sh
npm run build:android                        # every architecture
npm run build:android -- x86_64-linux-android # just one
```

## Licence

Apache-2.0 OR MIT, as Faith.
