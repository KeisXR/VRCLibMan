import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

// Dummy data only. No real product names, folders or thumbnails.
const list = (names) => ({
  path: "C:\\Lib\\1234567\\pack.zip",
  kind: "zip",
  truncated: false,
  packages: names.map((name) => ({ entry: `pack/${name}`, name, size: 10 })),
});

test("Unityで開く: plan, selection and paths", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const unity = await server.ssrLoadModule("/src/unityOpen.ts");
    assert.equal(unity.isUnityOpenableName("a.unitypackage"), true);
    assert.equal(unity.isUnityOpenableName("A.ZIP"), true);
    assert.equal(unity.isUnityOpenableName("a.txt"), false);

    assert.equal(unity.unityEditorReady(null), false);
    assert.equal(unity.unityEditorReady({ selectedPath: "", selectedMissing: false }), false);
    assert.equal(unity.unityEditorReady({ selectedPath: "C:\\U\\Unity.exe", selectedMissing: true }), false);
    assert.equal(unity.unityEditorReady({ selectedPath: "C:\\U\\Unity.exe", selectedMissing: false }), true);

    assert.deepEqual(unity.unityOpenPlan(list([]), true), { kind: "none" });
    assert.deepEqual(unity.unityOpenPlan(list(["A.unitypackage"]), true), { kind: "direct", entries: ["pack/A.unitypackage"] });
    assert.deepEqual(unity.unityOpenPlan(list(["A.unitypackage"]), false), { kind: "setup", entries: ["pack/A.unitypackage"] });
    assert.deepEqual(unity.unityOpenPlan(list(["A.unitypackage", "B.unitypackage"]), true), { kind: "choose" });

    const many = list(["A.unitypackage", "B.unitypackage", "C.unitypackage"]);
    assert.deepEqual(unity.selectedEntries(many, new Set(["pack/C.unitypackage", "pack/A.unitypackage"])), [
      "pack/A.unitypackage",
      "pack/C.unitypackage",
    ]);
    assert.deepEqual(unity.onlyEntry(many, "pack/B.unitypackage").packages.map((item) => item.name), ["B.unitypackage"]);
    assert.equal(unity.onlyEntry(many, "pack/Z.unitypackage"), null);

    assert.equal(unity.isInsideRoot("C:\\Lib\\1\\a.zip", "c:/lib"), true);
    assert.equal(unity.isInsideRoot("C:\\Library\\a.zip", "C:\\Lib"), false);
    assert.equal(unity.isInsideRoot("C:\\Lib", "C:\\Lib"), false);
    assert.equal(unity.isInsideRoot("\\\\nas\\share\\a.zip", "\\\\nas\\share"), true);
    assert.equal(unity.isInsideRoot("C:\\nas\\share\\a.zip", "\\\\nas\\share"), false);
    assert.equal(unity.isInsideRoot("C:\\Lib\\a.zip", ""), false);

    assert.equal(unity.boothIdFromPath("C:\\Lib\\1234567\\sub\\a.zip"), "1234567");
    assert.equal(unity.boothIdFromPath("C:\\Lib\\Shop\\a.zip"), null);
    assert.equal(unity.boothIdFromPath("C:\\Lib\\Shop\\123\\a.zip"), null);
    assert.match(unity.launchedMessage(2, "unity"), /Unity に 2 件/);
    assert.match(unity.launchedMessage(1, "system"), /関連付けで 1 件/);
  } finally {
    await server.close();
  }
});

test("Unityで開く: preview mode lists and fakes extraction without leaving the library", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const files = await server.ssrLoadModule("/src/preview/files.ts");
    const zip = files.previewUnityPackages("C:\\Assets\\1000001\\Sample_Outfit_v1.2.zip", "C:\\Assets", "\\\\nas\\vrclibman", () => null);
    assert.equal(zip.kind, "zip");
    assert.equal(zip.packages.length, 2);
    const prepared = files.previewPrepareUnityPackages(zip, [zip.packages[1].entry], "1000001");
    assert.equal(prepared.length, 1);
    assert.match(prepared[0].path, /unity-import\\1000001\\/);
    assert.throws(() => files.previewUnityPackages("D:\\Else\\a.zip", "C:\\Assets", "", () => null), /ライブラリ/);
    assert.throws(() => files.previewUnityPackages("C:\\Assets\\a.txt", "C:\\Assets", "", () => null), /zip か/);
    const single = files.previewUnityPackages("C:\\Assets\\1000002\\Sample_Hair.unitypackage", "C:\\Assets", "", () => 5);
    assert.equal(single.kind, "package");
    assert.deepEqual(files.previewPrepareUnityPackages(single, [], null).map((item) => item.path), [single.path]);
  } finally {
    await server.close();
  }
});

test("Unityで開く: launching never goes through a shell or -importPackage", () => {
  const rust = readFileSync(new URL("../src-tauri/src/unity_import.rs", import.meta.url), "utf8");
  const code = rust.split("#[cfg(test)]")[0];
  assert.match(code, /"-openfile"/);
  assert.doesNotMatch(code, /"-importPackage"/);
  assert.doesNotMatch(code, /Command::new\("(cmd|sh|bash|powershell)/);
  assert.doesNotMatch(code, /log::|println!|eprintln!/);
  const tab = readFileSync(new URL("../src/fileBrowser.ts", import.meta.url), "utf8");
  assert.match(tab, /UNITY_OPEN_AVAILABLE = true/);
});
