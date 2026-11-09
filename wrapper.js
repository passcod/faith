/**
 * Faith for Node.js: the shared wrapper over the platform binary `index.js` selects.
 */

const { fileURLToPath } = require("node:url");

const createFaith = require("./wrapper-factory.js");
const native = require("./index.js");

module.exports = createFaith(native, { fileURLToPath });
