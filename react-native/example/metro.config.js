const path = require("node:path");

const { makeMetroConfig } = require("@rnx-kit/metro-config");

// Faith is linked from the repository rather than installed from the registry, so Metro is
// told to watch it: a path outside the project root is otherwise invisible to it. Both the
// React Native package and the Node package it takes the wrapper from are linked, and the
// Node package is the repository root, so the root is watched too.
const faithPackage = path.resolve(__dirname, "..");
const repositoryRoot = path.resolve(faithPackage, "..");

/** Escape a literal string for use inside a regular expression. */
function quote(value) {
	return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// Watching the repository root puts Cargo's `target` directory in Metro's path, and it runs
// to several gigabytes. Metro crawls every watched folder when it starts, so the
// directories that hold no JavaScript are excluded by name rather than left to be walked.
const blockList = ["target", "bench", ".git", ".jj", ".workhorse"].map(
	(directory) => new RegExp(`^${quote(path.join(repositoryRoot, directory) + path.sep)}`),
);

module.exports = makeMetroConfig({
	watchFolders: [faithPackage, repositoryRoot],
	resolver: { blockList },
});
