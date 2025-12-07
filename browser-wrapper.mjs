/**
 * Faith Fetch API Browser Wrapper
 *
 * This wrapper provides a spec-compliant Fetch API interface for browser environments.
 * It imports the auto-generated browser.js ES module and uses the shared wrapper factory
 * to provide:
 * - `Response` class (not `FetchResponse`)
 * - `body` as a property/getter (not a method)
 * - Additional methods: `arrayBuffer()`, `json()`, `clone()`
 * - Proper body type conversion
 */

// Import the auto-generated browser module
// Note: This assumes the bundler will handle the WASM import
import * as nativeModule from "./browser.js";

// Extract the native bindings
const native = {
  FetchResponse: nativeModule.FetchResponse,
  fetch: nativeModule.fetch,
};

// Import the wrapper factory
// Since we're in an ES module context, we need to use dynamic import
// for the CommonJS factory module
const factoryModule = await import("./wrapper-factory.js");
const { createWrapper } = factoryModule;

// Create the wrapper using the factory
const { Response, fetch } = createWrapper(native);

// Export the wrapper
export { Response, fetch };

// Default export for convenience
export default { Response, fetch };
