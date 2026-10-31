import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

// Dummy data only. No real product names, folders or thumbnails.
test("file tab helpers handle paths, kinds and zip folders", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const files = await server.ssrLoadModule("/src/fileBrowser.ts");
    assert.equal(files.fileKind("a.ZIP", "file"), "zip");
    assert.equal(files.fileKind("b.unitypackage", "file"), "unitypackage");
    assert.equal(files.fileKind("c.txt", "file"), "file");
    assert.equal(files.fileKind("d.zip", "dir"), "dir");
    assert.equal(files.fileKind("e.zip", "link"), "link");
    assert.equal(files.extensionOf(".hidden"), "");

    assert.equal(files.joinPath("C:\\Assets", "x"), "C:\\Assets\\x");
    assert.equal(files.joinPath("C:\\", "x"), "C:\\x");
    assert.equal(files.joinPath("/home/test", "x"), "/home/test/x");
    assert.equal(files.parentOf("C:\\Assets\\1000001"), "C:\\Assets");
    assert.equal(files.parentOf("C:\\Assets"), "C:\\");
    assert.equal(files.parentOf("C:\\"), null);
    assert.equal(files.parentOf("/home/test"), "/home");
    assert.equal(files.parentOf("/home"), "/");
    assert.equal(files.parentOf("/"), null);
    assert.equal(files.parentOf("\\\\nas\\share"), null);
    assert.equal(files.baseName("C:\\Assets\\a.zip"), "a.zip");
    assert.equal(files.isZipPath('"C:\\Assets\\a.zip"'), true);
    assert.equal(files.isZipPath("C:\\Assets\\zip"), false);

    const entries = [
      { path: "Pack/A.unitypackage", dir: false, size: 10 },
      { path: "Pack/Tex/b.png", dir: false, size: 2 },
      { path: "readme.txt", dir: false, size: 1 },
      { path: "Empty/", dir: true, size: null },
    ];
    const root = files.zipChildren(entries, "");
    assert.deepEqual(root.map((child) => [child.name, child.dir]), [["Empty", true], ["Pack", true], ["readme.txt", false]]);
    const pack = files.zipChildren(entries, "Pack");
    assert.deepEqual(pack.map((child) => [child.path, child.dir, child.size]), [["Pack/Tex", true, null], ["Pack/A.unitypackage", false, 10]]);
    assert.equal(files.zipParent("Pack/Tex"), "Pack");
    assert.equal(files.zipParent("Pack"), "");
    assert.equal(files.zipParent(""), null);
    assert.deepEqual(files.zipUnityPackages(entries).map((entry) => entry.path), ["Pack/A.unitypackage"]);
    assert.equal(files.UNITY_OPEN_AVAILABLE, false);
  } finally {
    await server.close();
  }
});

test("preview fake lists folders and zips with the app's wording", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const fake = await server.ssrLoadModule("/src/preview/files.ts");
    const library = "C:\\Assets";
    const home = fake.previewListDirectory("", library, "");
    assert.equal(home.path, library);
    assert.equal(home.insideLibrary, true);
    assert.equal(home.parent, "C:\\");
    assert.ok(home.entries.length > 0);
    const outside = fake.previewListDirectory("C:\\Users", library, "");
    assert.equal(outside.insideLibrary, false);
    assert.throws(() => fake.previewListDirectory("C:\\Nope", library, ""), /このパスは見つかりません/);
    assert.throws(() => fake.previewListDirectory("C:\\Assets\\..\\Users", library, ""), /「..」/);
    const zipName = home.entries.length && fake.previewListDirectory("C:\\Assets\\1000001", library, "").entries.find((e) => /\.zip$/i.test(e.name));
    assert.ok(zipName);
    const zip = fake.previewListZip(`C:\\Assets\\1000001\\${zipName.name}`);
    assert.ok(zip.entries.some((entry) => entry.path.endsWith(".unitypackage")));
    assert.throws(() => fake.previewListZip("C:\\Assets\\readme.txt"), /zip ファイルではありません/);
  } finally {
    await server.close();
  }
});

test("the file tab stays read-only and keeps the Unity slot behind a flag", () => {
  const tab = readFileSync("src/components/FilesTab.tsx", "utf8");
  const rust = readFileSync("src-tauri/src/file_browser.rs", "utf8");
  assert.match(tab, /UNITY_OPEN_AVAILABLE/);
  assert.match(tab, /revealInFolder/);
  assert.doesNotMatch(tab, /deleteFile|removeFile|renameFile|writeFile/);
  assert.doesNotMatch(rust.split("#[cfg(test)]")[0], /fs::(remove|rename|write|create_dir|copy)/);
});
