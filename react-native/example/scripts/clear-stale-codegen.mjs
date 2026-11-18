// Remove the empty `@react-native/codegen` directory a successful Android build leaves
// inside the React Native package.
//
// react-native-test-app points the Gradle plugin's codegen directory at the first
// `node_modules/@react-native/codegen` it finds walking up from the React Native package,
// and it tests for the directory rather than for the CLI inside it. A build creates that
// directory there and leaves it empty, so the next build finds it, looks for the codegen
// CLI in it, and fails with "Cannot find module". The real package is hoisted to the app's
// own `node_modules`, which is where the search lands once the empty one is gone.
//
// Removing it is safe because it is empty: an npm install never puts anything there, since
// the lockfile places the package at the top level.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const exampleRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const stale = path.join(
	exampleRoot,
	"node_modules/react-native/node_modules/@react-native/codegen",
);

if (fs.existsSync(stale) && fs.readdirSync(stale).length === 0) {
	fs.rmdirSync(stale);
	console.log("Removed the empty codegen directory left by the last build");
}
