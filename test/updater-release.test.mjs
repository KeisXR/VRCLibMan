import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { updaterConfig } from "../scripts/configure-updater.mjs";
import { createUpdaterManifest } from "../scripts/create-updater-manifest.mjs";

const pubkey = readFileSync(
  new URL("../src-tauri/test-fixtures/updater/public-key.txt", import.meta.url),
  "utf8",
).trim();
const signature = readFileSync(
  new URL(
    "../src-tauri/test-fixtures/updater/payload.txt.sig",
    import.meta.url,
  ),
  "utf8",
);
const names = [
  "VRCLibMan_99.0.0_x64-setup.exe",
  "VRCLibMan_99.0.0_aarch64.app.tar.gz",
];
const assets = names
  .flatMap((name) => [name, `${name}.sig`])
  .map((name) => ({
    name,
    state: "uploaded",
    size: 100,
    browser_download_url: `https://github.com/KeisXR/VRCLibMan/releases/download/v99.0.0/${name}`,
  }));
const input = {
  version: "99.0.0",
  repository: "KeisXR/VRCLibMan",
  assets,
  signature: () => signature,
  notes: "変更点\nテスト",
  now: new Date("2026-10-25T00:00:00Z"),
};

test("release builds require a signing key and embed only the public key", () => {
  assert.throws(() => updaterConfig({}), /登録/);
  assert.throws(
    () => updaterConfig({ TAURI_UPDATER_PUBLIC_KEY: pubkey }),
    /登録/,
  );
  assert.throws(
    () =>
      updaterConfig({
        TAURI_SIGNING_PRIVATE_KEY: "secret",
        TAURI_UPDATER_PUBLIC_KEY: "bad key",
      }),
    /公開鍵/,
  );
  const config = updaterConfig({
    TAURI_SIGNING_PRIVATE_KEY: "never-embed-this-secret",
    TAURI_UPDATER_PUBLIC_KEY: pubkey,
  });
  assert.equal(config.plugins.updater.pubkey, pubkey);
  assert.equal(config.bundle.createUpdaterArtifacts, true);
  assert.ok(!JSON.stringify(config).includes("never-embed-this-secret"));
});

test("complete manifest uses public asset URLs and both exact target keys", () => {
  const manifest = createUpdaterManifest(input);
  assert.deepEqual(Object.keys(manifest.platforms), [
    "windows-x86_64",
    "darwin-aarch64",
  ]);
  assert.equal(manifest.version, "99.0.0");
  assert.equal(manifest.notes, "変更点\nテスト");
  assert.equal(manifest.pub_date, "2026-10-25T00:00:00.000Z");
  assert.equal(
    manifest.platforms["windows-x86_64"].url,
    assets[0].browser_download_url,
  );
  assert.equal(
    manifest.platforms["darwin-aarch64"].signature,
    signature.trim(),
  );
});

test("partial or mixed releases never produce a manifest", () => {
  for (const asset of assets) {
    assert.throws(
      () =>
        createUpdaterManifest({
          ...input,
          assets: assets.filter((item) => item !== asset),
        }),
      /ありません/,
    );
  }
  assert.throws(
    () => createUpdaterManifest({ ...input, version: "99.0.1" }),
    /ありません/,
  );
  assert.throws(
    () =>
      createUpdaterManifest({
        ...input,
        assets: assets.map((asset) => ({ ...asset, size: 0 })),
      }),
    /ありません/,
  );
  assert.throws(
    () => createUpdaterManifest({ ...input, signature: () => "" }),
    /署名/,
  );
  assert.throws(
    () =>
      createUpdaterManifest({
        ...input,
        assets: assets.map((asset) => ({
          ...asset,
          browser_download_url: "https://example.invalid/other.exe",
        })),
      }),
    /URL/,
  );
});
