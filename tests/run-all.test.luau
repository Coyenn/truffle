-- Test runner that imports all test files
-- Run with: zune test tests/run-all.test.luau

-- Import all test files
require("./sync.test")
require("./highlights.test")
require("./convert.test")

print("All test files loaded successfully!")
