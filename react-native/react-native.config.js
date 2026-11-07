// Faith contributes no React Native module of its own: the Gradle module exists to put
// the platform verifier's Kotlin component on the app's classpath and to run Faith's
// own initialiser. Autolinking skips a package it finds no native module in, so the
// Android platform is declared here explicitly.
module.exports = {
	dependency: {
		platforms: {
			android: {},
		},
	},
};
