// Build Faith's Android prebuild for React Native.
//
// This does what `ferric build` would, minus two things it cannot do. ferric invokes cargo
// with the target and the configuration and nothing else, so there is no way to ask it for
// the slim feature set a mobile binary wants; and Faith's TypeScript declarations are
// written by hand, so there is nothing to generate. What is left is the NDK environment and
// the prebuild layout, and the layout is assembled by the host package's own exported
// helpers, which is what ferric uses too.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { createAndroidLibsDirectory } from "react-native-node-api";
import { androidPrebuildPath } from "weak-node-api";

const packageRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = path.resolve(packageRoot, "..");

// Matching the version the React Native community template pins, as ferric does.
const NDK_VERSION = process.env.FAITH_NDK_VERSION ?? "27.1.12297006";
// The floor React Native itself supports.
const API_LEVEL = 24;

// The slim build: HTTP/1.1 and HTTP/2 over TLS with the platform trust store. The Node
// default set carries a disk cache, a DNS client and revocation machinery that each want
// something an Android app sandbox does not hand out, and four more megabytes per ABI.
const FEATURES = "tls-aws-lc-rs";

/**
 * Each Rust target, with the NDK's name for its compilers and the host package's name for
 * its ABI. The NDK names the arm32 toolchain `armv7a` where Rust names the target `armv7`,
 * so neither name can be derived from the other without a table.
 */
const TARGETS = {
	"aarch64-linux-android": { ndk: "aarch64-linux-android", triplet: "aarch64-linux-android" },
	"armv7-linux-androideabi": { ndk: "armv7a-linux-androideabi", triplet: "armv7a-linux-androideabi" },
	"i686-linux-android": { ndk: "i686-linux-android", triplet: "i686-linux-android" },
	"x86_64-linux-android": { ndk: "x86_64-linux-android", triplet: "x86_64-linux-android" },
};

const ABI_FOR_TRIPLET = {
	"aarch64-linux-android": "arm64-v8a",
	"armv7a-linux-androideabi": "armeabi-v7a",
	"i686-linux-android": "x86",
	"x86_64-linux-android": "x86_64",
};

function requestedTargets() {
	const requested = process.argv.slice(2);
	if (requested.length === 0) {
		return Object.keys(TARGETS);
	}
	for (const target of requested) {
		assert(target in TARGETS, `Not an Android target Faith builds for: ${target}`);
	}
	return requested;
}

function toolchainBinPath() {
	const sdk = process.env.ANDROID_HOME ?? process.env.ANDROID_SDK_ROOT;
	assert(sdk && fs.existsSync(sdk), "Set ANDROID_HOME to the Android SDK directory");
	const ndk = path.join(sdk, "ndk", NDK_VERSION);
	assert(
		fs.existsSync(ndk),
		`No NDK at ${ndk}. Install it with: sdkmanager --install "ndk;${NDK_VERSION}"`,
	);
	const prebuilt = path.join(ndk, "toolchains", "llvm", "prebuilt");
	const [host, ...rest] = fs.readdirSync(prebuilt);
	assert(host && rest.length === 0, `Expected one LLVM toolchain in ${prebuilt}`);
	return path.join(prebuilt, host, "bin");
}

/**
 * The environment one target's cargo build runs under.
 *
 * `CARGO_ENCODED_RUSTFLAGS` replaces the repository's own `.cargo/config.toml` rustflags
 * rather than adding to them, which drops `--cfg reqwest_unstable`. The slim build has no
 * HTTP/3 and so does not need it.
 *
 * `--no-as-needed` around `weak-node-api` keeps the linker from dropping a library nothing
 * references at link time. Nothing does: the host fills those symbols in when it loads the
 * addon.
 */
function environmentFor(target, binPath) {
	const { ndk, triplet } = TARGETS[target];
	const clang = path.join(binPath, `${ndk}${API_LEVEL}-clang`);
	const weakNodeApi = path.join(androidPrebuildPath, ABI_FOR_TRIPLET[triplet]);
	assert(fs.existsSync(clang), `Expected an NDK compiler at ${clang}`);
	assert(fs.existsSync(weakNodeApi), `Expected weak-node-api libraries at ${weakNodeApi}`);

	const linkerVariable = `CARGO_TARGET_${target.replaceAll("-", "_").toUpperCase()}_LINKER`;
	return {
		CARGO_ENCODED_RUSTFLAGS: [
			"-L",
			weakNodeApi,
			"-C",
			"link-arg=-Wl,--push-state,--no-as-needed,-lweak-node-api,--pop-state",
		].join("\u001f"),
		[linkerVariable]: clang,
		TARGET_CC: clang,
		TARGET_CXX: `${clang}++`,
		TARGET_AR: path.join(binPath, "llvm-ar"),
		TARGET_RANLIB: path.join(binPath, "llvm-ranlib"),
		ANDROID_NDK: path.resolve(binPath, "..", "..", "..", ".."),
		PATH: `${binPath}${path.delimiter}${process.env.PATH}`,
	};
}

function build(target, binPath) {
	console.log(`Building web-faith-napi for ${target}`);
	execFileSync(
		"cargo",
		[
			"build",
			"--release",
			"--package", "web-faith-napi",
			"--no-default-features",
			"--features", FEATURES,
			"--target", target,
		],
		{
			cwd: repoRoot,
			stdio: "inherit",
			env: { ...process.env, ...environmentFor(target, binPath) },
		},
	);
	const library = path.join(repoRoot, "target", target, "release", "libfaith.so");
	assert(fs.existsSync(library), `cargo produced no library at ${library}`);
	return library;
}

const binPath = toolchainBinPath();
const libraries = requestedTargets().map((target) => ({
	triplet: TARGETS[target].triplet,
	libraryPath: build(target, binPath),
}));

const outputPath = await createAndroidLibsDirectory({
	outputPath: path.join(packageRoot, "faith.android.node"),
	libraries,
	autoLink: true,
});
console.log(`Wrote ${path.relative(repoRoot, outputPath)}`);
