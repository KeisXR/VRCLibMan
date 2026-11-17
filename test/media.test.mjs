import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));

function directives(csp) {
  return Object.fromEntries(
    csp.split(";").map((part) => part.trim().split(/\s+/)).filter((words) => words[0]).map(([name, ...sources]) => [name, sources]),
  );
}

// A product's thumbnail URL comes from imported data. Only BOOTH's image servers are contacted.
test("images that are not saved yet load only from BOOTH's image servers", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { remoteImageUrl } = await server.ssrLoadModule("/src/media.ts");
    for (const url of [
      "https://booth.pximg.net/c/300x300_a2_g5/abc/i/123456/def_base_resized.jpg",
      "https://booth.pximg.net/abc/i/123456/def.png?v=2",
      "https://s2.booth.pm/static-images/thumbnail.png",
      "https://booth.pm/static-images/thumbnail.png",
      "https://i.pximg.net/user-profile/img/a.jpg",
    ]) {
      assert.equal(remoteImageUrl(url), url);
    }
    for (const url of [
      null,
      "",
      "not a url",
      "http://booth.pximg.net/a.jpg",
      "https://tracker.example.test/pixel.gif?user=1",
      "http://192.168.0.1/admin.png",
      "http://127.0.0.1:17321/health",
      "https://evilbooth.pm/a.jpg",
      "https://booth.pm.example.test/a.jpg",
      "https://pximg.net.example.test/a.jpg",
      "https://user@booth.pximg.net/a.jpg",
      "https://booth.pximg.net:8443/a.jpg",
      "file:///C:/Users/kei/secret.png",
      "javascript:alert(1)",
      "data:image/png;base64,AAAA",
      "asset://localhost/etc/passwd",
    ]) {
      assert.equal(remoteImageUrl(url), null, String(url));
    }
  } finally {
    await server.close();
  }
});

// The main window's policy: only the app's own scripts run, and images come from the app,
// its saved image folders and the same BOOTH image servers as above.
test("the main window has a content security policy that matches what the app loads", () => {
  const csp = config.app.security.csp;
  assert.equal(typeof csp, "string");
  const policy = directives(csp);
  assert.deepEqual(policy["default-src"], ["'self'"]);
  assert.deepEqual(policy["script-src"], ["'self'"], "no inline script, no eval, no remote script");
  assert.deepEqual(policy["object-src"], ["'none'"]);
  assert.deepEqual(policy["frame-src"], ["'none'"]);
  assert.deepEqual(policy["base-uri"], ["'self'"]);
  // Tauri's IPC: ipc: on macOS and Linux, http://ipc.localhost on Windows.
  assert.deepEqual(policy["connect-src"], ["'self'", "ipc:", "http://ipc.localhost"]);
  // The asset protocol: asset: on macOS and Linux, http://asset.localhost on Windows.
  assert.deepEqual(policy["img-src"], [
    "'self'",
    "asset:",
    "http://asset.localhost",
    "data:",
    "https://booth.pm",
    "https://*.booth.pm",
    "https://pximg.net",
    "https://*.pximg.net",
  ]);
  assert.equal(/unsafe-eval|\*\s|\shttps:(\s|;|$)|\shttp:(\s|;|$)/.test(csp), false, "no scheme-wide or wildcard source");
  assert.equal(policy["style-src"].includes("'unsafe-inline'"), true, "styles only; scripts never get it");
});
