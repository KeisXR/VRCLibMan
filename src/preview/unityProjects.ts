import type { InstallVerdict, ItemView, PackageInstall, ProjectContents, UnityProject, UnityProjectList } from "../types";

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
  /** 見本の商品の題・サムネイル・カテゴリ。無い商品は undefined。 */
  productRef?: (boothId: string) => { title: string; thumbnailUrl: string | null; category: string | null } | undefined;
  /** 見本の商品カードの中身。無い商品は undefined。 */
  itemView?: (boothId: string) => ItemView | undefined;
}

/** 見本の「まだ入っていない対応アセット」。プロジェクトごとに、入っていないと分かった商品。 */
const SAMPLE_MISSING: Record<string, string[]> = {
  a1b2c3d4e5f60001: ["8800130"],
};

/** 見本の段階 3: Plum-Main に渡すとき Avatar Optimizer が無いと出す商品。 */
const SAMPLE_NEEDS: string[] = ["8800130"];

/** 見本の「調べた結果」。プロジェクトごとに（商品 ID、判定、パッケージ）。 */
type SampleRow = [string, InstallVerdict, Omit<PackageInstall, "sourcePath">[]];
const pkg = (name: string, verdict: InstallVerdict, matched: number, total: number, extra: Partial<PackageInstall> = {}) => ({
  name,
  verdict,
  uniqueMatched: matched,
  uniqueTotal: total,
  closest: false,
  ...extra,
});
const SAMPLE_CONTENTS: Record<string, SampleRow[]> = {
  a1b2c3d4e5f60001: [
    ["7770415", "installed", [pkg("Plum_v.1.0.1.unitypackage", "installed", 163, 163), pkg("lilToon_1.4.0.unitypackage", "bundled", 0, 350, { bundledWith: "jp.lilxyzw.liltoon" })]],
    ["8800101", "installed", [pkg("OnePiece_v2.unitypackage", "installed", 40, 42, { closest: true }), pkg("OnePiece_v1.unitypackage", "installed", 38, 42)]],
    ["8800120", "partial", [pkg("EyeTextures.unitypackage", "partial", 12, 30)]],
    ["8800130", "missing", [pkg("SmallItem.unitypackage", "missing", 0, 8)]],
    ["8800111", "unknown", [pkg("Ribbon.unitypackage", "unknown", 0, 0)]],
  ],
  a1b2c3d4e5f60003: [["8901001", "installed", [pkg("WorldProp.unitypackage", "installed", 19, 19)]]],
};
/** 調べた日時。調べるまで無い（見本では Plum-Main だけ調べ済み）。 */
const scannedAt: Record<string, number> = { a1b2c3d4e5f60001: NOW - 1800 };

/** 見本の VPM パッケージの読みやすい名前（どこかのプロジェクトで歩いたもの）。 */
const PACKAGE_NAMES: Record<string, string> = {
  "com.vrchat.avatars": "VRChat SDK - Avatars",
  "com.vrchat.base": "VRChat SDK - Base",
  "com.vrchat.worlds": "VRChat SDK - Worlds",
  "jp.lilxyzw.liltoon": "lilToon",
  "nadena.dev.modular-avatar": "Modular Avatar",
};

function contents(projectId: string, context: PreviewProjectContext): ProjectContents {
  const at = scannedAt[projectId];
  if (at === undefined) return { scannedAt: null, complete: false, stale: false, products: [], packageNames: PACKAGE_NAMES };
  const rows = SAMPLE_CONTENTS[projectId] ?? [];
  return {
    scannedAt: at,
    complete: true,
    stale: false,
    packageNames: PACKAGE_NAMES,
    products: rows.flatMap(([boothId, verdict, packages]) => {
      const product = context.productRef?.(boothId);
      if (!product) return [];
      const kind = product.category === "world" ? "world" : product.category === "avatar" ? "avatar" : "asset";
      const avatarId = kind === "avatar" ? (context.productAvatarIds(boothId)[0] ?? null) : null;
      return [
        {
          boothId,
          title: product.title,
          thumbnailPath: null,
          thumbnailUrl: product.thumbnailUrl,
          category: product.category,
          link: product.category && (kind !== "avatar" || avatarId) ? { kind, avatarId } : null,
          verdict,
          packages: packages.map((item) => ({ ...item, sourcePath: `D:\\Library\\${boothId}\\${item.name}` })),
        },
      ];
    }),
  };
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
    case "unity_project_contents":
      find(args.projectId);
      return { value: contents(String(args.projectId), context) };
    case "scan_unity_project": {
      const project = find(args.projectId);
      if (project.state !== "ok") throw new Error("プロジェクトのフォルダを読めません");
      scannedAt[project.id] = Math.floor(Date.now() / 1000);
      return { value: contents(project.id, context) };
    }
    case "product_unity_installs": {
      const boothId = String(args.boothId ?? "");
      const places = Object.entries(SAMPLE_CONTENTS).flatMap(([projectId, rows]) => {
        const project = projects.find((item) => item.id === projectId);
        const row = rows.find(([id]) => id === boothId);
        if (!project || project.hidden || scannedAt[projectId] === undefined || !row) return [];
        if (row[1] !== "installed" && row[1] !== "partial") return [];
        return [{ projectId, name: project.name, verdict: row[1] }];
      });
      return { value: places };
    }
    case "unity_project_suggestions": {
      const project = find(args.projectId);
      if (scannedAt[project.id] === undefined) return { value: { scanned: false, items: [] } };
      const items = (SAMPLE_MISSING[project.id] ?? [])
        .map((boothId) => context.itemView?.(boothId))
        .filter((item): item is ItemView => item !== undefined);
      return { value: { scanned: true, items } };
    }
    case "unity_package_check": {
      // 見本: 開いているのは Plum-Main（渡す先が決まるとき）。そこに入っている商品は「入っています」、
      // SAMPLE_NEEDS の商品は Avatar Optimizer が無い、名前に lilToon を含むパッケージは同梱版とする。
      const boothId = String(args.boothId ?? "");
      const entries = Array.isArray(args.entries) ? args.entries.map(String) : [];
      const plum = projects.find((item) => item.id === "a1b2c3d4e5f60001");
      if (!plum || plum.running !== "open") return { value: null };
      const row = (SAMPLE_CONTENTS.a1b2c3d4e5f60001 ?? []).find(([id]) => id === boothId);
      const installed = Boolean(row && row[1] === "installed");
      const missing = SAMPLE_NEEDS.includes(boothId)
        ? [{ kind: "vpm" as const, name: "AAO: Avatar Optimizer", package: "com.anatawa12.avatar-optimizer" }]
        : [];
      const bundled = entries
        .filter((entry) => /liltoon/i.test(entry))
        .map((entry) => ({ entry, name: "lilToon" }));
      if (!installed && missing.length === 0 && bundled.length === 0) return { value: null };
      return { value: { projectId: plum.id, projectName: plum.name, installed, missing, bundled } };
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
