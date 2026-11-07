/**
 * Faith for React Native.
 *
 * The same wrapper the Node package uses, over the prebuilt addon the React Native host
 * loads. `react-native-node-api`'s Babel plugin rewrites the require below into a
 * `requireNodeAddon` call on its TurboModule, so the app must have that plugin in its
 * Babel configuration, as it must for any Node-API addon.
 */

const createFaith = require("@passcod/faith/wrapper-factory");

const native = require("./faith.android.node");

/**
 * Convert a `file:` URL to a path, for `response.toFile()`.
 *
 * Node's own conversion is not reachable here, and the platforms React Native runs on are
 * POSIX, so there are no drive letters or UNC paths to account for. An encoded separator
 * is refused rather than decoded, as Node refuses it, because decoding it would silently
 * change which directory the path names.
 *
 * @param {URL} url
 * @returns {string}
 */
function fileURLToPath(url) {
	if (url.protocol !== "file:") {
		throw new TypeError("not a file: URL");
	}
	if (/%2f/i.test(url.pathname)) {
		throw new TypeError("must not include encoded / characters");
	}
	return decodeURIComponent(url.pathname);
}

module.exports = createFaith(native, { fileURLToPath });
