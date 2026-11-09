// Put a Gradle wrapper in the example's android directory, fetching Gradle itself if the
// machine has none.
//
// The alternative to this is committing `gradle-wrapper.jar`, which every Android project
// does, or asking everyone who touches the example to install Gradle at whatever version
// React Native currently wants. Fetching on demand keeps the binary out of the repository
// and keeps the version right without anyone maintaining a pin: react-native-test-app
// publishes the Gradle version each React Native release needs, so the version is read
// from there rather than written down here.
//
// `unzip` is expected on the PATH, which holds for Linux and macOS. The example only
// targets Android from a Unix host so far.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const exampleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const androidDir = path.join(exampleRoot, "android");
const cacheRoot = path.join(exampleRoot, "node_modules", ".cache", "gradle");

const DISTRIBUTIONS = "https://services.gradle.org/distributions";

/** The Gradle version the installed React Native calls for. */
function requiredVersion() {
	const { GRADLE_VERSIONS } = require("react-native-test-app/android/gradle-wrapper.js");
	const { toVersionNumber } = require("react-native-test-app/scripts/helpers");
	const reactNative = toVersionNumber(require("react-native/package.json").version);
	// Sorted newest first, each row the React Native version it starts applying from.
	const row = GRADLE_VERSIONS.find(([from]) => reactNative >= from);
	assert(row, `react-native-test-app names no Gradle version for React Native ${reactNative}`);
	return row[1][1];
}

/** The version the wrapper already in place would use, if there is one. */
function installedVersion() {
	const properties = path.join(androidDir, "gradle", "wrapper", "gradle-wrapper.properties");
	if (!fs.existsSync(path.join(androidDir, "gradlew")) || !fs.existsSync(properties)) {
		return undefined;
	}
	return fs.readFileSync(properties, "utf8").match(/gradle-([.0-9]+)-bin\.zip/)?.[1];
}

async function download(url) {
	const response = await fetch(url);
	assert(response.ok, `${url} returned ${response.status}`);
	return Buffer.from(await response.arrayBuffer());
}

/**
 * Unpack a Gradle distribution into the cache, checked against the digest Gradle publishes
 * beside it, and return the path to its `gradle` binary.
 *
 * The digest comes from the same host as the archive, so it catches a truncated or
 * corrupted download rather than a compromised one. It is also written into the wrapper
 * below, where it guards every later run.
 */
async function fetchDistribution(version, sha256) {
	const target = path.join(cacheRoot, `gradle-${version}`);
	const binary = path.join(target, "bin", "gradle");
	if (fs.existsSync(binary)) {
		return binary;
	}

	console.log(`Fetching Gradle ${version}`);
	const archive = await download(`${DISTRIBUTIONS}/gradle-${version}-bin.zip`);
	const actual = createHash("sha256").update(archive).digest("hex");
	assert.equal(actual, sha256, `Gradle ${version} did not match its published digest`);

	fs.mkdirSync(cacheRoot, { recursive: true });
	const archivePath = path.join(cacheRoot, `gradle-${version}-bin.zip`);
	fs.writeFileSync(archivePath, archive);
	// Into a staging directory first, so an interrupted unpack does not leave something
	// that looks installed.
	const staging = `${target}.partial`;
	fs.rmSync(staging, { recursive: true, force: true });
	execFileSync("unzip", ["-q", archivePath, "-d", staging], { stdio: "inherit" });
	fs.renameSync(path.join(staging, `gradle-${version}`), target);
	fs.rmSync(staging, { recursive: true, force: true });
	fs.rmSync(archivePath);
	return binary;
}

const version = requiredVersion();
if (installedVersion() === version) {
	process.exit(0);
}

const sha256 = (await download(`${DISTRIBUTIONS}/gradle-${version}-bin.zip.sha256`))
	.toString("utf8")
	.trim();
const gradle = await fetchDistribution(version, sha256);

console.log(`Writing a Gradle ${version} wrapper into ${path.relative(exampleRoot, androidDir)}`);

// Generated in an empty directory and copied in, rather than run in `android` itself. The
// `wrapper` task belongs to the root project, so running it there would configure the whole
// build first: minutes of work, and it would need the vendored Hermes already in place.
// What it writes does not depend on the project at all.
const scratch = path.join(cacheRoot, "wrapper");
fs.rmSync(scratch, { recursive: true, force: true });
fs.mkdirSync(scratch, { recursive: true });
// Gradle refuses to run a task in a directory holding no build, and an empty settings file
// is enough of one for the `wrapper` task.
fs.writeFileSync(path.join(scratch, "settings.gradle"), "");
execFileSync(
	gradle,
	[
		"wrapper",
		"--gradle-version",
		version,
		// Pinned into the wrapper, so every later `./gradlew` verifies the distribution it
		// downloads instead of trusting the address alone.
		"--gradle-distribution-sha256-sum",
		sha256,
	],
	{ cwd: scratch, stdio: "inherit" },
);

fs.mkdirSync(path.join(androidDir, "gradle", "wrapper"), { recursive: true });
for (const entry of ["gradlew", "gradlew.bat", "gradle/wrapper/gradle-wrapper.jar", "gradle/wrapper/gradle-wrapper.properties"]) {
	fs.copyFileSync(path.join(scratch, entry), path.join(androidDir, entry));
}
fs.chmodSync(path.join(androidDir, "gradlew"), 0o755);
fs.rmSync(scratch, { recursive: true, force: true });
