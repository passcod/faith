const test = require("tape");
const fs = require("fs");
const path = require("path");
const native = require("../index.js");

test("wrapper.d.ts ERROR_CODES definition is in sync", (t) => {
  const nativeErrorCodes = native.errorCodes();

  // Read the wrapper.d.ts file
  const dtsPath = path.join(__dirname, "..", "wrapper.d.ts");
  const dtsContent = fs.readFileSync(dtsPath, "utf-8");

  // Extract ERROR_CODES definition from the TypeScript file
  const errorCodesMatch = dtsContent.match(
    /export const ERROR_CODES: \{([^}]+)\}/s,
  );

  t.ok(errorCodesMatch, "Should find ERROR_CODES definition in wrapper.d.ts");

  if (errorCodesMatch) {
    const errorCodesBlock = errorCodesMatch[1];

    // Extract all readonly property definitions
    const propertyRegex = /readonly\s+(\w+):\s*"(\w+)"/g;
    const foundCodes = [];
    let match;

    while ((match = propertyRegex.exec(errorCodesBlock)) !== null) {
      const [, key, value] = match;
      foundCodes.push(key);

      // Verify const enum pattern (key === value)
      t.equal(
        key,
        value,
        `TypeScript definition should follow const enum pattern: ${key}: "${value}"`,
      );
    }

    t.equal(
      foundCodes.length,
      nativeErrorCodes.length,
      "wrapper.d.ts ERROR_CODES should have same number of entries as native errorCodes()",
    );

    // Check that every native error code exists in TypeScript definition
    for (const code of nativeErrorCodes) {
      t.ok(
        foundCodes.includes(code),
        `wrapper.d.ts should have readonly ${code}: "${code}"`,
      );
    }

    // Check that TypeScript definition doesn't have extra codes
    for (const code of foundCodes) {
      t.ok(
        nativeErrorCodes.includes(code),
        `wrapper.d.ts ERROR_CODES entry "${code}" should exist in native errorCodes()`,
      );
    }
  }

  t.end();
});
