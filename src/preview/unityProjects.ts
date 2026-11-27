import type { UnityProject, UnityProjectList } from "../types";

/** 見本の Unity プロジェクト。ALCOM の一覧を真似る。実在のフォルダは見ない。 */
interface PreviewProject {
  id: string;
  name: string;
  path: string;
  state: UnityProject["state"];
  version: string;
  kind: UnityProject["kind"];
  packages: UnityProject["packages"];
  source: UnityProject["source"];
  hidden: boolean;
  running: UnityProject["running"];
  editorAvailable: boolean;
  avatarIds: string[];
  modified: number | null;
}

const NOW = Math.floor(Date.now() / 1000);
const AVATAR_PACKAGES = [
  { name: "com.vrchat.avatars", version: "3.10.6" },
  { name: "com.vrchat.base", version: "3.10.6" },
  { name: "jp.lilxyzw.liltoon", version: "2.4.1" },
  { name: "nadena.dev.modular-avatar", version: "1.18.7" },
];

const projects: PreviewProject[] = [
  { id: "a1b2c3d4e5f60001", name: "Plum-Main", path: "D:\\ALCOM\\Plum-Main", state: "ok", version: "2022.3.22f1", kind: "avatar", packages: AVATAR_PACKAGES, source: "alcom", hidden: false, running: "open", editorAvailable: true, avatarIds: ["plum"], modified: NOW - 3600 },
  { id: "a1b2c3d4e5f60002", name: "Lime-Test", path: "D:\\ALCOM\\Lime-Test", state: "ok", version: "2022.3.22f1", kind: "avatar", packages: AVATAR_PACKAGES, source: "alcom", hidden: false, running: "closed", editorAvailable: true, avatarIds: [], modified: NOW - 86400 * 3 },
  { id: "a1b2c3d4e5f60003", name: "ChillWorld", path: "D:\\ALCOM\\ChillWorld", state: "ok", version: "2022.3.22f1", kind: "world", packages: [{ name: "com.vrchat.base", version: "3.10.6" }, { name: "com.vrchat.worlds", version: "3.10.6" }], source: "alcom", hidden: false, running: "closed", editorAvailable: true, avatarIds: [], modified: NOW - 86400 * 10 },
  { id: "a1b2c3d4e5f60004", name: "Manuka-Old", path: "D:\\ALCOM\\Manuka-Old", state: "ok", version: "2019.4.31f1", kind: "avatar", packages: [{ name: "com.vrchat.avatars", version: "3.1.13" }], source: "alcom", hidden: false, running: "closed", editorAvailable: false, avatarIds: ["manuka"], modified: NOW - 86400 * 400 },
  { id: "a1b2c3d4e5f60005", name: "Backup-Plum", path: "E:\\Backup\\Backup-Plum", state: "missing", version: "", kind: "unknown", packages: [], source: "alcom", hidden: true, running: "closed", editorAvailable: false, avatarIds: [], modified: null },
];

let added = 0;

export interface PreviewProjectContext {
  avatarName: (id: string) => string | undefined;
  productAvatarIds: (boothId: string) => string[];
  productIsWorld: (boothId: string) => boolean;
}

function view(project: PreviewProject, context: PreviewProjectContext): UnityProject {
  const { avatarIds, ...rest } = project;
  return {
    ...rest,
    packages: [...rest.packages],
    avatars: avatarIds
      .map((id) => ({ id, name: context.avatarName(id) }))
      .filter((avatar): avatar is { id: string; name: string } => avatar.name !== undefined)
      .sort((a, b) => a.name.localeCompare(b.name, "ja")),
  };
}

function list(context: PreviewProjectContext): UnityProjectList {
  const sorted = [...projects].sort((a, b) => (b.modified ?? -1) - (a.modified ?? -1));
  return {
    projects: sorted.map((project) => view(project, context)),
    alcomFound: true,
    alcomPath: "C:\\Users\\you\\AppData\\Local\\VRChatCreatorCompanion\\settings.json",
  };
}

function find(id: unknown): PreviewProject {
  const project = projects.find((item) => item.id === id);
  if (!project) throw new Error("プロジェクトが見つかりません。一覧を読み直してください");
  return project;
}

/** 見本の Unity プロジェクトのコマンド。扱わないコマンドは `undefined`。 */
export function previewUnityProjectDispatch(
  command: string,
  args: Record<string, unknown>,
  context: PreviewProjectContext,
): { value: unknown } | undefined {
  switch (command) {
    case "unity_projects":
      return { value: list(context) };
    case "product_unity_projects": {
      // Rust の `projects_for_product` と同じ: 対応アバターに結び付けたもの、ワールドの商品ならワールドのプロジェクト。
      const boothId = String(args.boothId ?? "");
      const avatars = new Set(context.productAvatarIds(boothId));
      const world = context.productIsWorld(boothId);
      return {
        value: list(context).projects.filter(
          (project) =>
            !project.hidden &&
            (project.avatars.some((avatar) => avatars.has(avatar.id)) || (world && project.kind === "world")),
        ),
      };
    }
    case "add_unity_project": {
      const path = String(args.path ?? "").trim();
      if (!path) throw new Error("プロジェクトのフォルダを選んでください");
      const name = path.split(/[\\/]/).filter(Boolean).pop() ?? path;
      added += 1;
      projects.push({ id: `b0b0b0b0b0b0${String(added).padStart(4, "0")}`, name, path, state: "ok", version: "2022.3.22f1", kind: "unknown", packages: [], source: "manual", hidden: false, running: "closed", editorAvailable: true, avatarIds: [], modified: NOW });
      return { value: list(context) };
    }
    case "remove_unity_project": {
      const project = find(args.projectId);
      if (project.source === "alcom") throw new Error("ALCOM の一覧にあるプロジェクトは、一覧から隠してください");
      projects.splice(projects.indexOf(project), 1);
      return { value: list(context) };
    }
    case "set_unity_project_hidden":
      find(args.projectId).hidden = Boolean(args.hidden);
      return { value: list(context) };
    case "link_unity_project_avatar": {
      const project = find(args.projectId);
      const avatarId = String(args.avatarId ?? "");
      if (args.linked) {
        if (context.avatarName(avatarId) === undefined) throw new Error("アバターが見つかりません");
        if (!project.avatarIds.includes(avatarId)) project.avatarIds.push(avatarId);
      } else {
        project.avatarIds = project.avatarIds.filter((id) => id !== avatarId);
      }
      return { value: list(context) };
    }
    case "open_unity_project": {
      const project = find(args.projectId);
      if (project.state === "missing") throw new Error("プロジェクトのフォルダが見つかりません");
      if (project.running === "open") return { value: { alreadyOpen: true } };
      if (!project.editorAvailable) {
        throw new Error(`このプロジェクトは Unity ${project.version} で作られています。同じ版の Unity が見つからないので、ALCOM か Unity Hub で開いてください`);
      }
      project.running = "open";
      return { value: { alreadyOpen: false } };
    }
    default:
      return undefined;
  }
}
