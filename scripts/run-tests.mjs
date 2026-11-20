// Runs every test/*.test.mjs with `node --test`, so a new test file is picked up without
// being added to a list. The files below need a real browser or a display and have their
// own npm scripts (see docs/development.md).
import { spawnSync } from "node:child_process";
import { readdirSync } from "node:fs";

const SEPARATE = new Set([
  "browser-panel-layout.test.mjs", // WebKitWebDriver and Xvfb: npm run test:browser-layout
  "extension-ui.test.mjs", // Chrome: npm run test:extension-ui
  "right-panel-tab-click.test.mjs", // Chrome: npm run test:tab-click
]);

const files = readdirSync("test")
  .filter((name) => name.endsWith(".test.mjs") && !SEPARATE.has(name))
  .sort()
  .map((name) => `test/${name}`);

const missing = [...SEPARATE].filter((name) => !readdirSync("test").includes(name));
if (missing.length) {
  console.error(`scripts/run-tests.mjs lists files that no longer exist: ${missing.join(", ")}`);
  process.exit(1);
}
if (!files.length) {
  console.error("No test files found. Run this from the repository root.");
  process.exit(1);
}

const result = spawnSync(process.execPath, ["--test", "--test-concurrency=1", ...process.argv.slice(2), ...files], {
  stdio: "inherit",
});
process.exit(result.status ?? 1);
