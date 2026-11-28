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

test("the sidebar, route and search index know the projects page", () => {
  const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  assert.match(app, /href: "#\/projects", label: "プロジェクト", icon: "projects"/);
  assert.match(app, /route\.page === "projects" && <ProjectsPage \/>/);
  const index = readFileSync(new URL("../src/appSearchIndex.ts", import.meta.url), "utf8");
  assert.match(index, /key: "nav:projects".*href: "#\/projects"/);
});
