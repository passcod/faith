const path = require("node:path");

const { makeMetroConfig } = require("@rnx-kit/metro-config");

// Faith is linked from the repository rather than installed from the registry, so Metro
// is told to watch it: a path outside the project root is otherwise invisible to it.
const faithPackage = path.resolve(__dirname, "..");

module.exports = makeMetroConfig({
	watchFolders: [faithPackage, path.resolve(faithPackage, "..")],
});
