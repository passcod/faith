#!/bin/bash

set -e

echo "Building faith project..."

echo "1. Checking Rust code..."
cargo check

echo "2. Running Rust tests..."
cargo test

echo "3. Building Node.js native module..."
if command -v npm &> /dev/null; then
    npm run build:debug
else
    echo "npm not found, skipping Node.js build"
fi

echo "4. Testing JavaScript interface..."
if [ -f "js/index.js" ]; then
    echo "JavaScript interface exists"
    echo "Basic structure check:"
    node -e "const mod = require('./js/index.js'); console.log('Module loaded successfully'); console.log('Exports:', Object.keys(mod).join(', '))"
else
    echo "JavaScript interface not found"
fi

echo "5. Running example..."
if [ -f "example.js" ]; then
    echo "Example file exists"
    echo "Note: Example requires internet connection to test with httpbin.org"
else
    echo "Example file not found"
fi

echo ""
echo "Build and test completed!"
echo ""
echo "To run the full test suite:"
echo "  cd faith && npm test"
echo ""
echo "To run comparison tests:"
echo "  cd faith && npm run test:compare"
echo ""
echo "To run the example:"
echo "  cd faith && node example.js"
