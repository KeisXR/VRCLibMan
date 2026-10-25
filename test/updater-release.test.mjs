import assert from "node:assert/strict";
import {
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { updaterConfig } from "../scripts/configure-updater.mjs";
import { createUpdaterManifest } from "../scripts/create-updater-manifest.mjs";
import { stageUpdaterArtifacts } from "../scripts/stage-updater-artifacts.mjs";
import { createReleaseNotes } from "../scripts/create-release-notes.mjs";
import { spawnSync } from "node:child_process";

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

test("release notes use only the selected version's changes", () => {
  const template = readFileSync(new URL("../.github/release-notes.md", import.meta.url), "utf8");
  const notes = createReleaseNotes({
    appVersion: "99.0.0",
    extensionVersion: "2.0.1",
    template,
    changes: "- この版だけの変更\n",
  });
  assert.match(notes, /## v99\.0\.0 の変更/);
  assert.match(notes, /vrclibman-booth-export-2\.0\.1\.zip/);
  assert.match(notes, /この版だけの変更/);
  assert.ok(!notes.includes("{{"));
  assert.ok(!notes.includes("v0.1.8 の変更"));
  assert.throws(() => createReleaseNotes({ appVersion: "99.0.0", extensionVersion: "1", template, changes: "  " }), /変更内容/);
});

test("a release without its own notes cannot reuse an older version's changes", () => {
  const directory = mkdtempSync(join(tmpdir(), "vrclibman-release-notes-"));
  try {
    const destination = join(directory, "notes.md");
    const result = spawnSync(process.execPath, ["scripts/create-release-notes.mjs", "99.0.0", "1.0", destination], { encoding: "utf8" });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /99\.0\.0\.md/);
    assert.deepEqual(readdirSync(directory), []);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("dry-run artifacts preserve package, signature and public key for both platforms", () => {
  const directory = mkdtempSync(join(tmpdir(), "vrclibman-updater-stage-"));
  try {
    for (const [target, name] of [
      ["windows-x64", names[0]],
      ["macos-aarch64", "VRCLibMan.app.tar.gz"],
    ]) {
      const artifact = join(directory, name);
      writeFileSync(artifact, "test updater bytes");
      writeFileSync(`${artifact}.sig`, signature);
      const destination = join(directory, target);
      stageUpdaterArtifacts({
        target,
        paths: [artifact, `${artifact}.sig`, "unrelated.dmg"],
        destination,
        pubkey,
      });
      assert.deepEqual(
        readdirSync(destination).sort(),
        [name, `${name}.sig`, "updater-public-key.txt"].sort(),
      );
      assert.equal(
        readFileSync(join(destination, name), "utf8"),
        "test updater bytes",
      );
      assert.equal(
        readFileSync(join(destination, `${name}.sig`), "utf8"),
        signature,
      );
      assert.equal(
        readFileSync(
          join(destination, "updater-public-key.txt"),
          "utf8",
        ).trim(),
        pubkey,
      );
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("dry-run artifacts reject missing signatures, keys, and empty packages", () => {
  const directory = mkdtempSync(join(tmpdir(), "vrclibman-updater-stage-"));
  try {
    const artifact = join(directory, names[0]);
    writeFileSync(artifact, "");
    writeFileSync(`${artifact}.sig`, signature);
    const input = {
      target: "windows-x64",
      paths: [artifact, `${artifact}.sig`],
      destination: join(directory, "out"),
      pubkey,
    };
    assert.throws(
      () => stageUpdaterArtifacts({ ...input, paths: [artifact] }),
      /署名/,
    );
    assert.throws(() => stageUpdaterArtifacts({ ...input, paths: [] }), /特定/);
    assert.throws(
      () => stageUpdaterArtifacts({ ...input, pubkey: "" }),
      /公開鍵/,
    );
    assert.throws(() => stageUpdaterArtifacts(input), /空/);
    assert.throws(
      () => stageUpdaterArtifacts({ ...input, target: "unsupported" }),
      /対象/,
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

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

test("draft assets produce their final published tag URLs", () => {
  const draftAssets = assets.map((asset) => ({
    ...asset,
    browser_download_url: asset.browser_download_url.replace(
      "/download/v99.0.0/",
      "/download/untagged-5efe83da0d55e6f01fb3/",
    ),
  }));
  const manifest = createUpdaterManifest({ ...input, assets: draftAssets });
  assert.equal(
    manifest.platforms["windows-x86_64"].url,
    assets[0].browser_download_url,
  );
  assert.equal(
    manifest.platforms["darwin-aarch64"].url,
    assets[2].browser_download_url,
  );
  assert.ok(!JSON.stringify(manifest).includes("untagged-"));
  for (const invalid of [
    "/download/untagged-not-an-id/",
    "/download/untagged-1234/../",
    "/download/v98.0.0/",
  ]) {
    assert.throws(
      () =>
        createUpdaterManifest({
          ...input,
          assets: draftAssets.map((asset) => ({
            ...asset,
            browser_download_url: asset.browser_download_url.replace(
              "/download/untagged-5efe83da0d55e6f01fb3/",
              invalid,
            ),
          })),
        }),
      /URL/,
    );
  }
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
