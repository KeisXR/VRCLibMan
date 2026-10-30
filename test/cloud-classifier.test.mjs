import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createServer } from "vite";

const DUMMY_KEY = "sk-preview-dummy-TESTONLY-0000";

test("preview fake cloud provider keeps only the key status", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { dispatch } = await server.ssrLoadModule("/src/preview/api.ts");
    const initial = await dispatch("get_settings", {});
    assert.equal(initial.llmBackend, "ollama");
    assert.equal(initial.cloudBaseUrl, "https://api.openai.com/v1");

    const saved = await dispatch("update_settings", { patch: { llmBackend: "cloud", cloudBaseUrl: " https://api.x.ai/v1/ ", cloudModel: " grok-test " } });
    assert.equal(saved.llmBackend, "cloud");
    assert.equal(saved.cloudBaseUrl, "https://api.x.ai/v1");
    assert.equal(saved.cloudModel, "grok-test");
    await assert.rejects(dispatch("update_settings", { patch: { cloudBaseUrl: "http://api.example.com/v1" } }), /https/);
    await assert.rejects(dispatch("update_settings", { patch: { cloudBaseUrl: "https://u:p@api.example.com/v1" } }), /認証情報/);

    assert.equal(await dispatch("cloud_key_status", {}), true);
    const ok = await dispatch("test_cloud_classifier", {});
    assert.equal(ok.ok, true);
    assert.match(ok.message, /grok-test/);

    assert.equal(await dispatch("delete_cloud_api_key", {}), false);
    const missing = await dispatch("test_cloud_classifier", {});
    assert.equal(missing.ok, false);
    assert.equal(missing.kind, "キー未保存");
    await assert.rejects(dispatch("save_cloud_api_key", { key: "  " }), /APIキーを入力/);
    await assert.rejects(dispatch("save_cloud_api_key", { key: "sk abc" }), /使えない文字/);
    assert.equal(await dispatch("save_cloud_api_key", { key: DUMMY_KEY }), true);
    assert.equal(await dispatch("cloud_key_status", {}), true);

    const after = await dispatch("get_settings", {});
    assert.ok(!JSON.stringify(after).includes(DUMMY_KEY));
    const setup = await dispatch("setup_status", {});
    assert.equal(setup.backend, "cloud");
    await dispatch("update_settings", { patch: { llmBackend: "ollama" } });
  } finally {
    await server.close();
  }
});

test("cloud classifier commands match between Rust, api.ts and the settings UI", async () => {
  const [lib, commands, api, settingsUi] = await Promise.all([
    readFile(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/commands.rs", import.meta.url), "utf8"),
    readFile(new URL("../src/api.ts", import.meta.url), "utf8"),
    readFile(new URL("../src/components/CloudClassifierSettings.tsx", import.meta.url), "utf8"),
  ]);
  for (const name of ["cloud_key_status", "save_cloud_api_key", "delete_cloud_api_key", "test_cloud_classifier"]) {
    assert.ok(lib.includes(`commands::${name},`), name);
    assert.ok(commands.includes(`pub fn ${name}(`) || commands.includes(`pub async fn ${name}(`), name);
    assert.ok(api.includes(`"${name}"`), name);
  }
  assert.match(commands, /pub fn save_cloud_api_key\(app: AppHandle, key: String\)/);
  assert.match(api, /call<boolean>\("save_cloud_api_key", \{ key \}\)/);
  assert.match(settingsUi, /type="password"/);
  assert.match(settingsUi, /chat\/completions/);
  assert.match(settingsUi, /年齢制限（R-18）の商品はクラウドに送りません/);
  assert.ok(!/console\.(log|info|debug)/.test(settingsUi));
});
