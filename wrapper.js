/**
 * Faith Fetch API Wrapper (Node.js)
 *
 * This wrapper provides a spec-compliant Fetch API interface on top of
 * the native Rust bindings. The main difference is that `body` is exposed
 * as a property/getter instead of a method, and the class is named `Response`
 * instead of `FetchResponse`.
 */

const native = require("./index.js");
const { createWrapper } = require("./wrapper-factory.js");

// Create the wrapper using the factory
const { Response, fetch, native: nativeBindings } = createWrapper(native);

// Export the wrapper
module.exports = {
  Response,
  fetch,
  // Also expose native bindings for advanced use
  native: nativeBindings,
};
