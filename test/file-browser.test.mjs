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
    assert.equal(files.UNITY_OPEN_AVAILABLE, true);
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
    const zip = fake.previewListZip(`C:\\Assets\\1000001\\${zipName.name}`, library, "\\\\nas\\vrclibman");
    assert.equal(zip.library, library);
    assert.equal(zip.remote, "\\\\nas\\vrclibman");
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


test("reopening a zip restores library actions and follows newly reported roots", async () => {
  const { parseHTML } = await import("linkedom");
  const { window } = parseHTML('<html><body><div id="root"></div></body></html>');
  window.location = new URL("http://localhost:1420/");
  for (const name of ["window", "document", "location", "HTMLElement", "HTMLInputElement", "Element", "Event", "CustomEvent", "EventTarget"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test" } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const { act, createElement } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  let root;
  try {
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const { ShellProvider } = await server.ssrLoadModule("/src/App.tsx");
    const { FilesTab } = await server.ssrLoadModule("/src/components/FilesTab.tsx");
    const { UNITY_OPEN_AVAILABLE } = await server.ssrLoadModule("/src/fileBrowser.ts");
    let roots = { library: "C:\\Lib", remote: "\\\\nas\\lib" };
    let directoryReads = 0;
    api.listDirectory = async () => {
      directoryReads++;
      return { path: "C:\\Lib\\1000001", parent: "C:\\Lib", ...roots, insideLibrary: true, truncated: false, entries: [{ name: "pack.zip", kind: "file", size: 20 }] };
    };
    api.listZipEntries = async () => ({ path: "C:\\Lib\\1000001\\pack.zip", ...roots, truncated: false, entries: [{ path: "A.unitypackage", dir: false, size: 10 }] });
    const shell = { notify() {}, fail() {} };
    const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
    const render = () => act(async () => { root.render(createElement(ShellProvider, { value: shell }, createElement(FilesTab))); await tick(); });
    const unityButtons = () => [...document.querySelectorAll("button")].filter((node) => node.textContent === "Unityで開く").length;
    const remoteButton = () => [...document.querySelectorAll("button")].find((node) => node.textContent === "リモート");
    root = createRoot(document.getElementById("root"));
    await render();
    await act(async () => { document.querySelector("button.files-entry").dispatchEvent(new window.Event("click", { bubbles: true })); await tick(); });
    assert.equal(unityButtons(), UNITY_OPEN_AVAILABLE ? 2 : 0);
    assert.ok(remoteButton());
    await act(async () => root.render(null));
    await render();
    assert.ok(document.body.textContent.includes("A.unitypackage"));
    assert.equal(unityButtons(), UNITY_OPEN_AVAILABLE ? 2 : 0);
    assert.ok(remoteButton());
    assert.equal(directoryReads, 1, "zip restoration does not need to read a directory first");

    roots = { library: "C:\\Other", remote: null };
    await act(async () => root.render(null));
    await render();
    assert.equal(unityButtons(), 0, "the old library stops offering Unity actions");
    assert.equal(remoteButton(), undefined);
  } finally {
    if (root) await act(async () => root.unmount());
    await server.close();
  }
});
