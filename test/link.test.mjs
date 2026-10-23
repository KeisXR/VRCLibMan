import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const port = "17321";
const rust = readFileSync("src-tauri/src/local_import.rs", "utf8");
const popup = readFileSync("extension/popup.js", "utf8");
const manifest = JSON.parse(readFileSync("extension/manifest.json", "utf8"));
const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));

test("extension and app share the fixed loopback port", () => {
  assert.match(rust, new RegExp(`LOCAL_IMPORT_PORT: u16 = ${port}`));
  assert.match(rust, /127\.0\.0\.1:\{LOCAL_IMPORT_PORT\}/);
  assert.match(popup, new RegExp(`http://127\\.0\\.0\\.1:\\$\\{LOCAL_PORT\\}`));
  assert.equal(popup.match(/const LOCAL_PORT = (\d+)/)[1], port);
  assert.deepEqual(manifest.host_permissions.filter((host) => host.startsWith("http://127.0.0.1")), [
    `http://127.0.0.1:${port}/*`,
  ]);
});

test("popup sends the export to the app and can wake it", () => {
  assert.match(popup, /credentials:\s*"omit"/);
  assert.match(popup, /vrclibman:\/\/import/);
  assert.match(popup, /https:\/\/accounts\.booth\.pm\/library/);
  assert.equal(popup.includes("chrome.cookies"), false);
  assert.equal(manifest.permissions.includes("cookies"), false);
  assert.equal(manifest.version, "0.1.7");
  assert.deepEqual(config.plugins["deep-link"].desktop.schemes, ["vrclibman"]);
});
