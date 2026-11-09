module.exports = {
	presets: ["module:@react-native/babel-preset"],
	// Rewrites `require("./faith.android.node")` in the Faith package into a
	// `requireNodeAddon` call on the host's TurboModule. Every app loading a Node-API
	// addon needs this.
	plugins: ["module:react-native-node-api/babel-plugin"],
};
