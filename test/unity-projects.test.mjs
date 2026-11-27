import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

async function withModules(paths, run) {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    await run(...(await Promise.all(paths.map((path) => server.ssrLoadModule(path)))));
  } finally {
    await server.close();
  }
}

const project = (extra = {}) => ({
  id: "a1b2c3d4e5f60001",
  name: "Plum-Main",
  path: "D:\\ALCOM\\Plum-Main",
  state: "ok",
  version: "2022.3.22f1",
  kind: "avatar",
  packages: [],
  source: "alcom",
  hidden: false,
  running: "closed",
  editorAvailable: true,
  avatars: [{ id: "plum", name: "プラム" }],
  modified: null,
  ...extra,
});

test("projects are filtered by name, path and avatar, and hidden ones stay out", async () => {
  await withModules(["/src/unityProjects.ts"], async ({ filterProjects, linkableProjects }) => {
    const list = [
      project(),
      project({ id: "2", name: "ChillWorld", path: "D:\\ALCOM\\ChillWorld", kind: "world", avatars: [] }),
      project({ id: "3", name: "Backup", hidden: true, avatars: [] }),
    ];
    assert.deepEqual(filterProjects(list, "", false).map((item) => item.id), ["a1b2c3d4e5f60001", "2"]);
    assert.deepEqual(filterProjects(list, "", true).length, 3);
    assert.deepEqual(filterProjects(list, "プラム", false).map((item) => item.name), ["Plum-Main"]);
    assert.deepEqual(filterProjects(list, "alcom chill", false).map((item) => item.name), ["ChillWorld"]);
    // ワールドのプロジェクトはアバターの候補に出さない。
    assert.deepEqual(linkableProjects(list, "plum"), []);
    const lime = project({ id: "4", name: "Lime", avatars: [] });
    assert.deepEqual(linkableProjects([...list, lime], "plum").map((item) => item.id), ["4"]);
  });
});

test("the row note tells why a project cannot open", async () => {
  await withModules(["/src/unityProjects.ts"], async ({ projectStateNote, canOpenProject, modifiedLabel }) => {
    assert.equal(projectStateNote(project()), null);
    assert.equal(projectStateNote(project({ state: "missing" })), "フォルダが見つかりません");
    assert.equal(projectStateNote(project({ state: "timeout" })), "確認できません");
    assert.equal(projectStateNote(project({ running: "open" })), "開いています");
    assert.equal(projectStateNote(project({ editorAvailable: false })), "Unity 2022.3.22f1 が見つかりません");
    // 版の合う Unity が無くても押せる。押したときに Rust が理由を返す。
    assert.equal(canOpenProject(project({ editorAvailable: false })), true);
    assert.equal(canOpenProject(project({ state: "missing" })), false);
    const now = Date.UTC(2026, 10, 27, 12) ;
    assert.equal(modifiedLabel(null, now), "");
    assert.equal(modifiedLabel(now / 1000 - 60, now), "今日");
    assert.equal(modifiedLabel(now / 1000 - 86400 * 3, now), "3 日前");
  });
});

test("the not-running message is recognised", async () => {
  await withModules(["/src/unityOpen.ts"], async ({ isUnityNotRunning }) => {
    assert.equal(isUnityNotRunning("Unity が起動していません。設定した Unity でプロジェクトを開いてから、もう一度押してください"), true);
    assert.equal(isUnityNotRunning("起動中の Unity は、設定した Unity と別のバージョンです。"), false);
  });
  // Rust の文言と同じであることを確かめる。
  const rust = readFileSync(new URL("../src-tauri/src/unity_import.rs", import.meta.url), "utf8");
  assert.match(rust, /EDITOR_NOT_RUNNING: &str =\s*"Unity が起動していません/);
});

test("the preview keeps ALCOM projects and lets links change", async () => {
  await withModules(["/src/preview/unityProjects.ts"], async ({ previewUnityProjectDispatch }) => {
    const context = {
      avatarName: (id) => ({ plum: "プラム", lime: "ライム", manuka: "マヌカ" })[id],
      productAvatarIds: (boothId) => (boothId === "7770415" ? ["plum"] : []),
      productIsWorld: (boothId) => boothId === "1969082",
    };
    const call = (command, args = {}) => previewUnityProjectDispatch(command, args, context)?.value;
    const list = call("unity_projects");
    assert.equal(list.alcomFound, true);
    const lime = list.projects.find((item) => item.name === "Lime-Test");
    assert.throws(() => call("remove_unity_project", { projectId: lime.id }), /隠してください/);
    const linked = call("link_unity_project_avatar", { projectId: lime.id, avatarId: "lime", linked: true });
    assert.deepEqual(linked.projects.find((item) => item.id === lime.id).avatars, [{ id: "lime", name: "ライム" }]);
    assert.deepEqual(call("product_unity_projects", { boothId: "7770415" }).map((item) => item.name), ["Plum-Main"]);
    // ワールドの商品には、結び付けなしでワールドのプロジェクトが候補になる。
    assert.deepEqual(call("product_unity_projects", { boothId: "1969082" }).map((item) => item.name), ["ChillWorld"]);
    assert.deepEqual(call("product_unity_projects", { boothId: "9999" }), []);
    assert.equal(call("open_unity_project", { projectId: list.projects[0].id }).alreadyOpen, true);
    const old = list.projects.find((item) => item.name === "Manuka-Old");
    assert.throws(() => call("open_unity_project", { projectId: old.id }), /2019\.4\.31f1/);
    assert.equal(previewUnityProjectDispatch("list_avatars", {}, context), undefined);
  });
});

test("project contents are grouped and missing products are not listed", async () => {
  await withModules(["/src/unityProjects.ts"], async ({ projectSections, projectProductHref, scannedLabel, needsScan }) => {
    const product = (boothId, verdict, extra = {}) => ({
      boothId,
      title: `商品${boothId}`,
      thumbnailPath: null,
      thumbnailUrl: null,
      category: "outfit",
      link: { kind: "asset", avatarId: null },
      verdict,
      packages: [],
      ...extra,
    });
    const sections = projectSections({
      scannedAt: 1,
      complete: true,
      stale: false,
      products: [product("2", "installed"), product("1", "installed"), product("3", "missing"), product("4", "partial"), product("5", "bundled"), product("6", "unknown")],
    });
    assert.deepEqual(sections.installed.map((item) => item.boothId), ["1", "2"]);
    assert.deepEqual([sections.partial.length, sections.bundled.length, sections.unknown.length], [1, 1, 1]);
    assert.equal(Object.values(sections).flat().some((item) => item.verdict === "missing"), false);
    assert.equal(projectProductHref(product("9", "installed")), "#/assets/item/9");
    assert.equal(projectProductHref(product("9", "installed", { link: { kind: "world", avatarId: null } })), "#/worlds/item/9");
    assert.equal(projectProductHref(product("9", "installed", { link: { kind: "avatar", avatarId: "plum" } })), "#/avatar/plum");
    assert.equal(projectProductHref(product("9", "installed", { link: null })), null);
    const now = Date.UTC(2026, 10, 27, 12);
    assert.equal(scannedLabel(null, now), "まだ調べていません");
    assert.equal(scannedLabel(now / 1000 - 300, now), "5 分前に調べました");
    assert.equal(needsScan(null), true);
    assert.equal(needsScan({ scannedAt: null, complete: false, stale: false, products: [] }), true);
    assert.equal(needsScan({ scannedAt: 1, complete: true, stale: true, products: [] }), true);
    assert.equal(needsScan({ scannedAt: 1, complete: true, stale: false, products: [] }), false);
  });
});

test("the preview scans a project once asked and reports where a product is", async () => {
  await withModules(["/src/preview/unityProjects.ts"], async ({ previewUnityProjectDispatch }) => {
    const context = {
      avatarName: (id) => ({ plum: "プラム" })[id],
      productAvatarIds: () => [],
      productIsWorld: () => false,
      productRef: (boothId) => ({ "8901001": { title: "見本のワールド小物", thumbnailUrl: null, category: "world" } })[boothId],
    };
    const call = (command, args = {}) => previewUnityProjectDispatch(command, args, context)?.value;
    const world = "a1b2c3d4e5f60003";
    assert.equal(call("unity_project_contents", { projectId: world }).scannedAt, null);
    assert.deepEqual(call("product_unity_installs", { boothId: "8901001" }), []);
    const scanned = call("scan_unity_project", { projectId: world });
    assert.equal(scanned.products[0].link.kind, "world");
    assert.deepEqual(call("product_unity_installs", { boothId: "8901001" }), [{ projectId: world, name: "ChillWorld", verdict: "installed" }]);
  });
});

test("badges follow one linked project, and installed avatar bodies suggest a link", async () => {
  await withModules(["/src/unityProjects.ts"], async ({ installMarks, markProjectId, bodySuggestions }) => {
    const product = (boothId, verdict, link = { kind: "asset", avatarId: null }) => ({
      boothId, title: `商品${boothId}`, thumbnailPath: null, thumbnailUrl: null, category: "outfit", link, verdict, packages: [],
    });
    const contents = {
      scannedAt: 1,
      complete: true,
      stale: false,
      products: [
        product("1", "installed"),
        product("2", "partial"),
        product("3", "missing"),
        product("4", "bundled"),
        product("5", "installed", { kind: "avatar", avatarId: "plum" }),
        product("6", "installed", { kind: "avatar", avatarId: "lime" }),
      ],
    };
    assert.deepEqual([...installMarks(contents).entries()], [["1", "installed"], ["2", "partial"], ["5", "installed"], ["6", "installed"]]);
    // 調べていなければ印を出さない（入っていないとは言わない）。
    assert.equal(installMarks({ ...contents, scannedAt: null }).size, 0);
    // 調べたあとに索引が変わった（古い）結果からも出さない。
    assert.equal(installMarks({ ...contents, stale: true }).size, 0);
    assert.equal(installMarks(null).size, 0);
    const a = project({ id: "a" });
    const b = project({ id: "b" });
    assert.equal(markProjectId([a, b], null), "a");
    assert.equal(markProjectId([a, b], "b"), "b");
    assert.equal(markProjectId([a], "b"), "a");
    assert.equal(markProjectId([], "b"), null);
    // プラムは結び付け済み、ライムはまだ。
    assert.deepEqual(bodySuggestions(contents, project()).map((item) => item.boothId), ["6"]);
    assert.deepEqual(bodySuggestions(null, project()), []);
    // 古い結果からは言わない。
    assert.deepEqual(bodySuggestions({ ...contents, stale: true }, project()), []);
  });
});

test("the preview suggests missing compatible assets and warns before a second hand-off", async () => {
  await withModules(["/src/preview/unityProjects.ts"], async ({ previewUnityProjectDispatch }) => {
    const context = {
      avatarName: (id) => ({ plum: "プラム" })[id],
      productAvatarIds: () => [],
      productIsWorld: () => false,
      productRef: () => undefined,
      itemView: (boothId) => ({ boothId, title: `見本${boothId}` }),
    };
    const call = (command, args = {}) => previewUnityProjectDispatch(command, args, context)?.value;
    const suggestions = call("unity_project_suggestions", { projectId: "a1b2c3d4e5f60001" });
    assert.equal(suggestions.scanned, true);
    assert.deepEqual(suggestions.items.map((item) => item.boothId), ["8800130"]);
    // 調べていないプロジェクトには候補を出さない。
    assert.deepEqual(call("unity_project_suggestions", { projectId: "a1b2c3d4e5f60002" }), { scanned: false, items: [] });
    assert.equal(call("unity_install_check", { boothId: "7770415", target: null }).name, "Plum-Main");
    assert.equal(call("unity_install_check", { boothId: "8800120", target: null }), null);
  });
});

test("the screen listens to the event name the Rust side sends, and drops late scan results", () => {
  const rust = readFileSync(new URL("../src-tauri/src/unity_index_jobs.rs", import.meta.url), "utf8");
  const api = readFileSync(new URL("../src/api.ts", import.meta.url), "utf8");
  const name = rust.match(/CHANGED_EVENT: &str = "([^"]+)"/)?.[1];
  assert.equal(name, "unity-index-changed");
  assert.ok(api.includes(`listen("${name}"`));
  // 調べている間に別のプロジェクトへ移ったとき、前のプロジェクトの結果を表示しない。
  const page = readFileSync(new URL("../src/pages/ProjectDetailPage.tsx", import.meta.url), "utf8");
  assert.match(page, /if \(shown\.current === id\) setContents\(value\)/);
});

test("the sidebar, route and search index know the projects page", () => {
  const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  assert.match(app, /href: "#\/projects", label: "プロジェクト", icon: "projects"/);
  assert.match(app, /route\.page === "projects" && \(route\.id \? <ProjectDetailPage id=\{route\.id\} \/> : <ProjectsPage \/>\)/);
  assert.match(app, /parts\[0\] === "projects" && parts\[1\]/);
  const index = readFileSync(new URL("../src/appSearchIndex.ts", import.meta.url), "utf8");
  assert.match(index, /key: "nav:projects".*href: "#\/projects"/);
});
