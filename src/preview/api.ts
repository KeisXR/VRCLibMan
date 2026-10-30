import { previewBrowserState, previewBrowserAction, previewSaveBookmark, previewDeleteBookmark, previewOpenBrowser, previewCloseBrowser } from "./browser";
import extensionManifest from "../../extension/manifest.json";
import { filePresence } from "../media";
import { GENRE_FREE, GENRE_UNSET, genresForKind } from "../types";
import type { ClassificationSnapshot, OwnedAvatarRestore, RegisterOwnedBodyResult, SnapshotLink } from "../types";
import type {
  AvatarCard,
  BrowserAction,
  IntegrationStatus,
  BoothImportMode,
  AvatarDetail,
  AvatarInput,
  AvatarRecord,
  BaseBody,
  Category,
  FileBrief,
  ExtensionLink,
  ImportReport,
  JobProgress,
  LibraryWalkReport,
  DownloadView,
  ItemView,
  Knowledge,
  RemoteProductProbe,
  LibraryStats,
  ProductUpdate,
  QueueFile,
  QueuePage,
  QueueProduct,
  Section,
  Settings,
  AppUpdateStatus,
} from "../types";

const excludedProducts = new Set<string>();

function activeProducts(): Product[] { return store.products.filter((p) => !excludedProducts.has(p.boothId)); }
function activeAvatars(): AvatarRecord[] {
  return store.avatars.map((avatar) => {
    const bodies = store.products.filter((p) => p.category === "avatar" && linksOf(p.boothId).includes(avatar.id));
    return { ...avatar, owned: avatar.owned && bodies.some((p) => !excludedProducts.has(p.boothId)) };
  });
}

interface Product {
  boothId: string;
  title: string;
  shop: string;
  descriptionSnippet: string;
  url: string;
  thumbnailUrl: string | null;
  thumbnailPath: string | null;
  category: Category | null;
  categoryLocked: boolean;
  genre?: string | null;
  genreLocked?: boolean;
  compatibleAvatars: string[];
  unresolvedNames: string[];
  isGeneric: boolean;
  confidence: number | null;
  linksLocked: boolean;
  userConfirmed: boolean;
  needsReview: boolean;
  updatedAt: string;
  isFree: boolean;
  isAdult: boolean;
  unpublished: boolean;
  classifierVersion?: number;
  boothCategoryId?: number | null;
  boothCategoryName?: string;
  boothCategoryParent?: string;
  boothTags?: string[];
}

interface LocalFile {
  path: string;
  filename: string;
  isDir: boolean;
  size: number;
  linkedProductId: string | null;
  candidateProductId: string | null;
  matchScore: number;
}

interface Store {
  products: Product[];
  files: LocalFile[];
  avatars: AvatarRecord[];
  baseBodies: BaseBody[];
  links: { productId: string; avatarId: string; source?: string }[];
  settings: Settings;
  setupComplete: boolean;
  variations: { boothId: string; name: string; owned: boolean }[];
  downloads: (DownloadView & { boothId: string })[];
  remoteReachable: boolean;
  remoteChecked: boolean;
}

// Sample genres only; the real classification lives in Rust (genre.rs).
const PREVIEW_GENRE: Record<string, string> = {
  outfit: "outfit",
  hair: "hair",
  accessory: "accessory",
  texture: "makeup_skin",
  gimmick: "gimmick",
  world: "world",
  other: "other",
};

function product(partial: Partial<Product> & Pick<Product, "boothId" | "title" | "category">): Product {
  return {
    genre: partial.category ? PREVIEW_GENRE[partial.category] ?? null : null,
    shop: "",
    descriptionSnippet: "",
    url: `https://booth.pm/ja/items/${partial.boothId}`,
    thumbnailUrl: null,
    thumbnailPath: null,
    categoryLocked: partial.category != null,
    compatibleAvatars: [],
    unresolvedNames: [],
    isGeneric: false,
    confidence: partial.category ? 0.9 : null,
    linksLocked: false,
    userConfirmed: false,
    needsReview: false,
    updatedAt: "2026-10-22T00:00:00Z",
    isFree: false,
    isAdult: false,
    unpublished: false,
    classifierVersion: partial.category ? 6 : 1,
    ...partial,
  };
}

const extensionLink: ExtensionLink = {
  token: "K7NP-4Q2M",
  port: 17321,
  listening: false,
  error: "見本では受信しません",
};

const store: Store = {
  settings: {
    ollamaBaseUrl: "http://127.0.0.1:11434",
    ollamaModel: "qwen3.5:9b",
    confidenceThreshold: 0.6,
    libraryFolder: "C:\\Assets",
    builtinBrowser: false,
    boothLinkTarget: "external",
    remoteAutoSync: false,
    remoteLibraryFolder: "\\\\nas\\vrclibman",
    routeDownloads: false,
    llmBackend: "ollama",
    cloudBaseUrl: "https://api.openai.com/v1",
    cloudModel: "gpt-4.1-mini",
    backgroundMode: false,
    launchAtLogin: false,
    unloadOllamaAfterClassification: true,
    classificationDismissedVersion: 0,
    uiRailWidth: 232,
    uiGenreWidth: 200,
    previewPane: false,
    previewPaneWidth: 300,
    checkUpdateOnStartup: false,
    uiUpdateDismissedVersion: "",
  },
  setupComplete: false,
  baseBodies: [
    {
      id: "komado",
      displayName: "あまとうさぎ共通素体",
      note: "作者こまど氏の商品説明で、ライム・プラム・シフォン・ショコラは共通素体で、衣装を着せ替えできると書かれている。",
    },
  ],
  avatars: [
    { id: "lime", displayName: "ライム", baseBody: "komado", aliases: ["Lime"], isBuiltin: true, owned: true, notes: "" },
    { id: "plum", displayName: "プラム", baseBody: "komado", aliases: ["Plum"], isBuiltin: true, owned: true, notes: "" },
    { id: "chiffon", displayName: "シフォン", baseBody: "komado", aliases: ["Chiffon"], isBuiltin: true, owned: false, notes: "" },
    { id: "chocolat", displayName: "ショコラ", baseBody: "komado", aliases: ["Chocolat"], isBuiltin: true, owned: false, notes: "" },
    { id: "manuka", displayName: "マヌカ", baseBody: null, aliases: ["Manuka", "MANUKA", "마누카"], isBuiltin: true, owned: true, notes: "" },
    { id: "shinano", displayName: "しなの", baseBody: null, aliases: ["Shinano"], isBuiltin: true, owned: false, notes: "" },
    { id: "celestia", displayName: "セレスティア", baseBody: null, aliases: ["Celestia", "Selestia"], isBuiltin: true, owned: false, notes: "" },
    { id: "milltina", displayName: "ミルティナ", baseBody: null, aliases: ["Milltina"], isBuiltin: true, owned: false, notes: "" },
    { id: "kikyo", displayName: "桔梗", baseBody: null, aliases: [], isBuiltin: true, owned: false, notes: "" },
    { id: "rurune", displayName: "ルルネ", baseBody: null, aliases: ["rurune", "Rurune"], isBuiltin: true, owned: false, notes: "" },
    { id: "owned-8800106", displayName: "架空ちゃん", baseBody: null, aliases: [], isBuiltin: false, owned: false, notes: "" },
  ],
  products: [
    product({
      boothId: "4876459",
      title: "ライム -Lime- / オリジナル3Dモデル",
      shop: "こまど",
      category: "avatar",
      descriptionSnippet: "共通素体のアバターです。",
    }),
    product({
      boothId: "7770415",
      title: "プラム -Plum- / オリジナル3Dモデル",
      shop: "こまど",
      category: "avatar",
    }),
    product({
      boothId: "5058077",
      title: "オリジナル3Dモデル「マヌカ」",
      shop: "STUDIO JINGO",
      category: "avatar",
    }),
    product({
      boothId: "8800101",
      title: "しなの・ライム・マヌカ対応 ワンピース",
      shop: "サンプル工房",
      thumbnailUrl: "/samples/ribbon.svg",
      category: "outfit",
      compatibleAvatars: ["しなの", "ライム", "マヌカ"],
    }),
    product({
      boothId: "8800111",
      title: "無料のリボン",
      shop: "サンプル工房",
      category: "accessory",
      isFree: true,
    }),
    product({
      boothId: "8800130",
      title: "手元にある無料の小物",
      shop: "見本工房",
      category: "accessory",
      isFree: true,
    }),
    product({
      boothId: "8800131",
      title: "手動で移した無料の小物",
      shop: "見本工房",
      category: "outfit",
      genre: "outfit",
      categoryLocked: true,
      genreLocked: true,
      isFree: true,
    }),
    product({
      boothId: "8800132",
      title: "非公開の見本アクセサリ",
      shop: "見本工房",
      category: "accessory",
      unpublished: true,
    }),
    product({
      boothId: "8800112",
      title: "年齢制限のアクセサリ",
      shop: "サンプル工房",
      thumbnailUrl: "/samples/seal.svg",
      category: "accessory",
      isAdult: true,
      descriptionSnippet: "",
    }),
    product({
      boothId: "8800102",
      title: "マヌカ用ヘア",
      shop: "サンプル工房",
      thumbnailUrl: "/samples/hair.svg",
      category: "hair",
      compatibleAvatars: ["マヌカ"],
    }),
    product({
      boothId: "8800103",
      title: "汎用ペンライト",
      shop: "サンプル工房",
      thumbnailUrl: "/samples/lamp.svg",
      category: "gimmick",
      isGeneric: true,
      descriptionSnippet: "どのアバターでも使えるギミックです。",
    }),
    product({
      boothId: "8800120",
      title: "瞳のテクスチャ集",
      shop: "サンプル工房",
      thumbnailUrl: "/samples/eye.svg",
      category: "texture",
      genre: "eye_texture",
      confidence: 1,
    }),
    product({
      boothId: "8800104",
      title: "架空ちゃん対応パーカー",
      shop: "サンプル工房",
      category: "outfit",
      confidence: 0.42,
      needsReview: true,
      compatibleAvatars: ["架空ちゃん"],
      unresolvedNames: ["架空ちゃん"],
      descriptionSnippet: "架空ちゃん専用と書かれていますが、対応表にありません。",
    }),
    product({
      boothId: "8800105",
      title: "説明がまだないアクセサリ",
      shop: "サンプル工房",
      category: null,
      categoryLocked: false,
      confidence: null,
      needsReview: true,
      descriptionSnippet: "",
    }),
    product({
      boothId: "8800106",
      title: "架空ちゃん / オリジナル3Dモデル",
      shop: "サンプル工房",
      category: "avatar",
      confidence: 0.7,
      needsReview: true,
    }),
    product({
      boothId: "8800201",
      title: "分類できなかった小物",
      shop: "サンプル工房",
      category: "other",
      genre: "other",
      categoryLocked: false,
      genreLocked: false,
      needsReview: false,
      descriptionSnippet: `用途の説明がありません。${"あ".repeat(450)}末尾マーカー`,
    }),
    product({
      boothId: "8800202",
      title: "手動で髪にしたリボン",
      shop: "サンプル工房",
      category: "hair",
      genre: "hair",
      categoryLocked: true,
      genreLocked: true,
      needsReview: false,
      descriptionSnippet: "小物として届きましたが、髪として使っています。",
    }),
  ],
  links: [
    { productId: "4876459", avatarId: "lime" },
    { productId: "7770415", avatarId: "plum" },
    { productId: "5058077", avatarId: "manuka" },
    { productId: "8800101", avatarId: "lime" },
    { productId: "8800101", avatarId: "shinano" },
    { productId: "8800101", avatarId: "manuka" },
    { productId: "8800111", avatarId: "lime" },
    { productId: "8800132", avatarId: "lime" },
    { productId: "8800112", avatarId: "lime" },
    { productId: "8800102", avatarId: "manuka" },
    { productId: "8800120", avatarId: "lime" },
    { productId: "8800106", avatarId: "owned-8800106" },
  ],
  files: [
    {
      path: "C:\\Assets\\Lime\\ワンピース.unitypackage",
      filename: "ワンピース.unitypackage",
      isDir: false,
      size: 48_000_000,
      linkedProductId: "8800101",
      candidateProductId: "8800101",
      matchScore: 0.94,
    },
    {
      path: "C:\\Assets\\Manuka\\hair_candidate.unitypackage",
      filename: "hair_candidate.unitypackage",
      isDir: false,
      size: 12_000_000,
      linkedProductId: null,
      candidateProductId: "8800102",
      matchScore: 0.62,
    },
    {
      path: "C:\\Assets\\Free\\small_free.unitypackage",
      filename: "small_free.unitypackage",
      isDir: false,
      size: 2048,
      linkedProductId: "8800130",
      candidateProductId: "8800130",
      matchScore: 1,
    },
  ],
  variations: [
    { boothId: "8800101", name: "通常", owned: true },
    { boothId: "8800101", name: "特典", owned: false },
  ],
  downloads: [
    {
      boothId: "4876459",
      variation: "",
      filename: "見本の本体.unitypackage",
      sizeBytes: 120_000_000,
      state: "local",
      localPath: "C:\\Assets\\Lime\\見本の本体.unitypackage",
      remotePath: null,
    },
    {
      boothId: "4876459",
      variation: "",
      filename: "見本の未取得.unitypackage",
      sizeBytes: 80_000_000,
      state: "missing",
      localPath: null,
      remotePath: null,
    },
    {
      boothId: "8800101",
      variation: "通常",
      filename: "ワンピース.unitypackage",
      sizeBytes: 48_000_000,
      state: "local",
      localPath: "C:\\Assets\\Lime\\ワンピース.unitypackage",
      remotePath: null,
    },
    {
      boothId: "8800101",
      variation: "通常",
      filename: "bonus.zip",
      sizeBytes: 1024,
      state: "remote",
      localPath: null,
      remotePath: "\\\\nas\\vrclibman\\8800101\\bonus.zip",
    },
    {
      boothId: "8800101",
      variation: "",
      filename: "extra.unitypackage",
      sizeBytes: null,
      state: "missing",
      localPath: null,
      remotePath: null,
    },
    {
      boothId: "8800120",
      variation: "",
      filename: "eye_base.zip",
      sizeBytes: 4096,
      state: "local",
      localPath: "C:\\Assets\\Lime\\eye_base.zip",
      remotePath: null,
    },
    {
      boothId: "8800130",
      variation: "",
      filename: "small_free.unitypackage",
      sizeBytes: 2048,
      state: "local",
      localPath: "C:\\Assets\\Free\\small_free.unitypackage",
      remotePath: null,
    },
    ...["8187247", "8187248", "8187253", "8187254", "8187260"].map((id) => ({
      boothId: "8800120",
      variation: "",
      filename: id,
      sizeBytes: null,
      state: "missing" as const,
      localPath: null,
      remotePath: null,
    })),
  ],
  remoteReachable: true,
  remoteChecked: false,
};

const stressPreview =
  typeof location !== "undefined" && new URLSearchParams(location.search).has("stress");

if (typeof location !== "undefined" && new URLSearchParams(location.search).get("remote") === "down") {
  store.remoteReachable = false;
}

type PreviewRemoteProbeOutcome = "ok" | "fail" | "timeout";
let previewRemoteProbeOutcome: PreviewRemoteProbeOutcome = "ok";
if (typeof location !== "undefined") {
  const mode = new URLSearchParams(location.search).get("remoteProbe");
  if (mode === "fail" || mode === "timeout") previewRemoteProbeOutcome = mode;
}

export function previewRemoteProbeDownloads(boothId: string): DownloadView[] {
  return placedDownloads(boothId);
}

const previewImageCache = new Map<string, string>();

store.products.push(
  product({
    boothId: "8901001",
    title: "見本のワールド小物",
    shop: "見本",
    category: "world",
    genre: "furniture_props",
    descriptionSnippet: "ワールドに置く小物です。",
  }),
  product({
    boothId: "8901002",
    title: "見本の夜空スカイボックス",
    shop: "見本",
    thumbnailUrl: "/samples/sky.svg",
    category: "world",
    genre: "skybox",
    descriptionSnippet: "ワールドの空に使う見本です。",
  }),
  product({
    boothId: "8901003",
    title: "見本のジャンル未設定ワールド素材",
    shop: "見本",
    category: "world",
    genre: null,
    descriptionSnippet: "ジャンルがまだ決まっていない見本です。",
  }),
  product({
    boothId: "8901004",
    title: "未入手の無料ワールド小物",
    shop: "見本",
    category: "world",
    genre: "furniture_props",
    isFree: true,
    unpublished: true,
    descriptionSnippet: "ファイルがまだ手元にない無料の見本です。",
  }),
  product({
    boothId: "8903101",
    title: "見本のその他小物",
    shop: "見本",
    category: "other",
    genre: "other",
    categoryLocked: false,
    genreLocked: false,
    descriptionSnippet: "自動分類でその他になった見本です。",
  }),
  product({
    boothId: "8903102",
    title: "北の見本の落ちた本体",
    shop: "見本",
    category: "other",
    genre: "other",
    categoryLocked: false,
    genreLocked: false,
    compatibleAvatars: ["北の見本"],
    descriptionSnippet: "抽出では北の見本と出た見本です。",
  }),
);
for (let index = 1; index <= 12; index += 1) {
  const name = `サンプル候補${String(index).padStart(2, "0")}`;
  store.products.push(
    product({
      boothId: String(8902000 + index),
      title: `${name}の小物`,
      shop: "見本",
      category: "gimmick",
      needsReview: true,
      confidence: 0.4,
      unresolvedNames: [name],
      descriptionSnippet: "確認キューの見本です。",
    }),
  );
}
store.avatars.push(
  {
    id: "sample-north",
    displayName: "北の見本",
    aliases: ["North"],
    baseBody: null,
    notes: "",
    isBuiltin: false,
    owned: false,
  },
  {
    id: "sample-south",
    displayName: "南の見本",
    aliases: ["South"],
    baseBody: null,
    notes: "",
    isBuiltin: false,
    owned: false,
  },
);

if (typeof location !== "undefined" && new URLSearchParams(location.search).get("browser") === "1") {
  store.settings.builtinBrowser = true;
  store.settings.routeDownloads = true;
  store.setupComplete = true;
}

let previewBoothLoggedIn = !(
  typeof location !== "undefined" && new URLSearchParams(location.search).get("account") === "out"
);

if (stressPreview) {
  store.setupComplete = true;
  for (let index = store.avatars.length; index < 1000; index += 1) {
    store.avatars.push({
      id: `stress-${index}`,
      displayName: `対応表${index}`,
      aliases: index % 5 === 0 ? [`Alias${index}`] : [],
      baseBody: null,
      notes: "",
      isBuiltin: true,
      owned: false,
    });
  }
  for (let index = 0; index < 50; index += 1) {
    store.products.push(
      product({
        boothId: String(900000 + index),
        title: `要確認の商品 ${index}`,
        shop: "計測工房",
        category: "outfit",
        needsReview: true,
        unresolvedNames: [`候補${index * 2}`, `候補${index * 2 + 1}`],
        descriptionSnippet: "計測用の説明です。",
      }),
    );
  }
}

function notePreviewCall(command: string) {
  if (!stressPreview || typeof window === "undefined") return;
  const target = window as Window & { __vrclibCalls?: string[] };
  target.__vrclibCalls ??= [];
  target.__vrclibCalls.push(command);
}

function linksOf(boothId: string): string[] {
  return store.links.filter((link) => link.productId === boothId).map((link) => link.avatarId);
}

function familyOf(avatar: AvatarRecord): Set<string> {
  const ids = new Set<string>([avatar.id]);
  if (!avatar.baseBody) return ids;
  for (const other of store.avatars) {
    if (other.baseBody === avatar.baseBody) ids.add(other.id);
  }
  return ids;
}

function filesFor(boothId: string): FileBrief[] {
  return store.files
    .filter((file) => file.linkedProductId === boothId)
    .map((file) => ({
      path: file.path,
      filename: file.filename,
      isDir: file.isDir,
      size: file.size,
      matchScore: file.matchScore,
    }));
}

function placedDownloads(boothId: string): DownloadView[] {
  const configured = store.settings.remoteLibraryFolder.trim().length > 0;
  const reachable = configured && store.remoteReachable;
  return store.downloads
    .filter((row) => row.boothId === boothId)
    .map((row) => ({
      variation: row.variation,
      filename: row.filename,
      sizeBytes: row.sizeBytes,
      state: row.state === "remote" && !reachable ? "missing" : row.state,
      localPath: row.localPath,
      remotePath: row.remotePath,
    }));
}

function hasLocalCopy(boothId: string): boolean {
  const downloads = placedDownloads(boothId);
  const names = new Set(downloads.map((file) => file.filename));
  const extra = filesFor(boothId).filter((file) => !names.has(file.filename)).length;
  const presence = filePresence(downloads, extra);
  return presence === "local" || presence === "partial";
}

function browseGenre(item: Product, known: readonly string[]): string {
  const pinned = Boolean(item.categoryLocked && item.genreLocked);
  if (item.isFree && !pinned && !hasLocalCopy(item.boothId)) return GENRE_FREE;
  return item.genre && known.includes(item.genre) ? item.genre : GENRE_UNSET;
}

function itemView(item: Product, viaBaseBody: boolean): ItemView {
  const downloads = placedDownloads(item.boothId);
  const names = new Set(downloads.map((file) => file.filename));
  const files = filesFor(item.boothId).filter((file) => !names.has(file.filename));
  const remoteConfigured = store.settings.remoteLibraryFolder.trim().length > 0;
  const remoteChecked = !remoteConfigured || store.remoteChecked;
  return {
    boothId: item.boothId,
    title: item.title,
    shop: item.shop,
    url: item.url,
    descriptionSnippet: item.descriptionSnippet.slice(0, 500),
    thumbnailUrl: item.thumbnailUrl,
    thumbnailPath: item.thumbnailPath,
    category: item.category,
    genre: item.genre ?? null,
    classificationPinned: Boolean(item.categoryLocked && item.genreLocked),
    isGeneric: item.isGeneric,
    confidence: item.confidence,
    viaBaseBody,
    localFiles: files.slice(0, 8),
    localFileCount: files.length,
    variations: store.variations
      .filter((row) => row.boothId === item.boothId)
      .map((row) => ({ name: row.name, owned: row.owned })),
    downloads,
    isFree: item.isFree,
    isAdult: item.isAdult,
    unpublished: item.unpublished,
    remoteConfigured,
    remoteChecked,
    remoteReachable: remoteConfigured && store.remoteChecked && store.remoteReachable,
  };
}

function classify(avatar: AvatarRecord): {
  owned: ItemView[];
  sections: Section[];
  generic: ItemView[];
  thumbnailPath: string | null;
  thumbnailUrl: string | null;
} {
  const family = familyOf(avatar);
  const specific: { key: string; item: ItemView }[] = [];
  const generic: ItemView[] = [];
  const owned: ItemView[] = [];
  for (const item of activeProducts()) {
    if (item.category === "world") continue;
    const linked = new Set(linksOf(item.boothId));
    const inFamily = [...linked].some((id) => family.has(id));
    if (item.category === "avatar") {
      if (linked.has(avatar.id)) owned.push(itemView(item, false));
      continue;
    }
    if (inFamily) {
      specific.push({
        key: item.category ?? "uncategorized",
        item: itemView(item, !linked.has(avatar.id)),
      });
    } else if (item.isGeneric && linked.size === 0) {
      generic.push(itemView(item, false));
    }
  }
  const sections: Section[] = [];
  for (const key of ["outfit", "hair", "accessory", "texture", "gimmick", "other", "uncategorized"]) {
    const items = specific.filter((row) => row.key === key).map((row) => row.item);
    if (items.length > 0) sections.push({ category: key, items });
  }
  return {
    owned,
    sections,
    generic,
    thumbnailPath: owned.find((body) => body.thumbnailPath)?.thumbnailPath ?? null,
    thumbnailUrl: owned.find((body) => body.thumbnailUrl)?.thumbnailUrl ?? null,
  };
}

function sectionCount(sections: Section[], key: string): number {
  return sections.find((section) => section.category === key)?.items.length ?? 0;
}

function cards(): AvatarCard[] {
  return activeAvatars()
    .filter((avatar) => avatar.owned || referencedOnly(avatar.id))
    .map((avatar) => {
      const view = classify(avatar);
      const body = store.baseBodies.find((item) => item.id === avatar.baseBody);
      return {
        id: avatar.id,
        displayName: avatar.displayName,
        baseBody: avatar.baseBody,
        baseBodyName: body?.displayName ?? null,
        aliases: avatar.aliases,
        isBuiltin: avatar.isBuiltin,
        isPlaceholder: avatar.id.startsWith("owned-"),
        owned: avatar.owned,
        thumbnailPath: view.thumbnailPath,
        thumbnailUrl: view.thumbnailUrl,
        outfitCount: sectionCount(view.sections, "outfit"),
        hairCount: sectionCount(view.sections, "hair"),
        gimmickCount: sectionCount(view.sections, "gimmick"),
        genericCount: view.generic.length,
      };
    });
}

function referencedOnly(avatarId: string): boolean {
  return store.links.some((link) => {
    if (link.avatarId !== avatarId) return false;
    const product = activeProducts().find((item) => item.boothId === link.productId);
    return product != null && product.category !== "avatar" && product.category !== "world";
  });
}

function detail(id: string): AvatarDetail {
  const avatar = activeAvatars().find((item) => item.id === id);
  if (!avatar) throw new Error("アバターが見つかりません");
  const view = classify(avatar);
  const family = familyOf(avatar);
  const unsupportedOutfits = activeProducts()
    .filter((item) => {
      if (item.category !== "outfit") return false;
      const linked = linksOf(item.boothId);
      return (
        !linked.some((id) => family.has(id)) &&
        !(item.isGeneric && linked.length === 0)
      );
    })
    .map((item) => itemView(item, false))
    .sort((a, b) => {
      if (a.title !== b.title) return a.title < b.title ? -1 : 1;
      return a.boothId < b.boothId ? -1 : a.boothId > b.boothId ? 1 : 0;
    });
  const body = store.baseBodies.find((item) => item.id === avatar.baseBody);
  const siblings = activeAvatars()
    .filter(
      (other) =>
        other.id !== avatar.id &&
        other.owned &&
        Boolean(avatar.baseBody) &&
        other.baseBody === avatar.baseBody,
    )
    .map((other) => ({ id: other.id, displayName: other.displayName }));
  return {
    id: avatar.id,
    displayName: avatar.displayName,
    baseBody: avatar.baseBody,
    baseBodyName: body?.displayName ?? null,
    baseBodyNote: body?.note ?? null,
    aliases: avatar.aliases,
    isBuiltin: avatar.isBuiltin,
    isPlaceholder: avatar.id.startsWith("owned-"),
    owned: avatar.owned,
    siblings,
    ownedProducts: view.owned,
    sections: view.sections,
    genericItems: view.generic,
    unsupportedOutfits,
  };
}

function candidates() {
  const counts = new Map<string, number>();
  for (const product of activeProducts()) {
    for (const name of product.unresolvedNames) {
      counts.set(name, (counts.get(name) ?? 0) + 1);
    }
  }
  return [...counts.entries()]
    .map(([name, productCount]) => ({ name, productCount }))
    .sort((left, right) => right.productCount - left.productCount || left.name.localeCompare(right.name, "ja"));
}

function queuePresence(boothId: string): ReturnType<typeof filePresence> {
  const downloads = store.downloads.filter((row) => row.boothId === boothId);
  const names = new Set(downloads.map((row) => row.filename));
  const extra = store.files.filter(
    (file) => file.linkedProductId === boothId && !file.isDir && !names.has(file.filename),
  ).length;
  return filePresence(
    downloads.map((row) => ({ state: row.state })),
    extra,
  );
}

function queue(kind: string): QueuePage {
  const unextracted = activeProducts().filter((item) => item.category == null).length;
  const needsReview = activeProducts().filter((item) => item.needsReview && item.category != null).length;
  const unlinked = store.files.filter((file) => file.linkedProductId == null && !excludedProducts.has(file.candidateProductId ?? ""));
  const products: QueueProduct[] = activeProducts()
    .filter((item) => (kind === "unextracted" ? item.category == null : item.needsReview && item.category != null))
    .map((item) => ({
      boothId: item.boothId,
      title: item.title,
      shop: item.shop,
      url: item.url,
      descriptionSnippet: item.descriptionSnippet,
      thumbnailUrl: item.thumbnailUrl,
      thumbnailPath: item.thumbnailPath,
      category: item.category,
      genre: item.genre ?? null,
      classificationPinned: Boolean(item.categoryLocked && item.genreLocked),
      isGeneric: item.isGeneric,
      confidence: item.confidence,
      compatibleAvatars: item.compatibleAvatars,
      unresolvedNames: item.unresolvedNames,
      linkedAvatarIds: linksOf(item.boothId),
      userConfirmed: item.userConfirmed,
      filePresence: queuePresence(item.boothId),
    }));
  const files: QueueFile[] = unlinked.map((file) => ({
    path: file.path,
    filename: file.filename,
    isDir: file.isDir,
    size: file.size,
    matchScore: file.matchScore,
    candidateProductId: file.candidateProductId,
    candidateTitle:
      activeProducts().find((item) => item.boothId === file.candidateProductId)?.title ?? null,
  }));
  return {
    unextracted,
    needsReview,
    unlinkedFiles: unlinked.length,
    products: kind === "files" ? [] : products,
    files: kind === "files" ? files : [],
  };
}

function stats(): LibraryStats {
  const page = queue("review");
  return {
    products: activeProducts().length,
    ownedAvatars: activeAvatars().filter((avatar) => avatar.owned).length,
    unextracted: page.unextracted,
    needsReview: page.needsReview,
    queueCount: page.unextracted + page.needsReview,
    unlinkedFiles: page.unlinkedFiles,
    ollamaModel: store.settings.ollamaModel,
  };
}

function needsReview(item: Product, linkCount: number): boolean {
  if (item.userConfirmed) return false;
  if (item.category == null) return true;
  const unresolved = item.unresolvedNames.filter((name) => !isIgnorableName(name));
  if (unresolved.length > 0) return true;
  const placeholder = linksOf(item.boothId).some((id) => id.startsWith("owned-"));
  if (placeholder) return true;
  if (item.category === "world" || item.category === "gimmick" || item.category === "other") return false;
  if (item.category === "avatar") return linkCount === 0;
  return !item.isGeneric && linkCount === 0;
}

function isIgnorableName(name: string): boolean {
  const lower = name.toLowerCase();
  return (
    name.endsWith("様") ||
    lower.includes("modular avatar") ||
    lower.includes("qvpen") ||
    lower.includes("liltoon") ||
    lower === "avatar" ||
    name.includes("セール") ||
    name.includes("アバター対応")
  );
}

function updateProduct(update: ProductUpdate) {
  const item = store.products.find((product) => product.boothId === update.boothId);
  if (!item) throw new Error("商品が見つかりません");
  if (update.category) {
    item.category = update.category as Category;
    item.categoryLocked = true;
  }
  if (update.genre) { item.genre = update.genre; item.genreLocked = true; }
  if (typeof update.isGeneric === "boolean") item.isGeneric = update.isGeneric;
  if (update.avatarIds) {
    for (const id of update.avatarIds) {
      if (!store.avatars.some((avatar) => avatar.id === id)) {
        throw new Error(`アバター ${id} が対応表にありません`);
      }
    }
    item.linksLocked = true;
    item.unresolvedNames = [];
    store.links = store.links.filter((link) => link.productId !== item.boothId);
    for (const id of update.avatarIds) store.links.push({ productId: item.boothId, avatarId: id });
  }
  if (typeof update.confirm === "boolean") item.userConfirmed = update.confirm;
  item.needsReview = needsReview(item, linksOf(item.boothId).length);
  item.updatedAt = new Date().toISOString();
}

const GENRE_CATEGORY: Record<string, Category> = {
  outfit: "outfit",
  shoes_socks: "outfit",
  hair: "hair",
  accessory: "accessory",
  head_ears_tail: "accessory",
  makeup_skin: "texture",
  eye_texture: "texture",
  expression_animation: "gimmick",
  pose: "gimmick",
  gimmick: "gimmick",
  particle_effect: "gimmick",
  shader: "other",
  tool_editor: "other",
  other: "other",
  world: "world",
  furniture_props: "world",
  world_gimmick: "world",
  texture_material: "world",
  skybox: "world",
  lighting_postprocess: "world",
  world_shader: "world",
  building_terrain: "world",
  audio_bgm: "world",
  tool_other: "world",
};

const DESCRIPTION_HEAD_CHARS = 400;
const CLASSIFICATION_FEEDBACK_PURPOSE =
  "分類ルールの改善に渡すデータです。説明は先頭だけです。corrections は手動で移した履歴と、自動分類へ戻した操作、その取り消しです。過去の移動は復元していません。manualClassifications はいま手動で固定している分類です。トークンやファイルの場所は含みません。";

interface FeedbackPlacement {
  category: string | null;
  genre: string | null;
}

interface FeedbackMove {
  boothId: string;
  at: string;
  action: "move" | "unpin" | "restore";
  from: FeedbackPlacement;
  to: FeedbackPlacement | null;
}

const classificationMoves: FeedbackMove[] = [
  {
    boothId: "8800202",
    at: "2026-10-27T00:00:00Z",
    action: "move",
    from: { category: "other", genre: "other" },
    to: { category: "hair", genre: "hair" },
  },
];

function descriptionHead(text: string): string {
  return Array.from(text).slice(0, DESCRIPTION_HEAD_CHARS).join("");
}

function feedbackStatus(item: Product): string {
  if (item.category == null) return "unclassified";
  if (item.categoryLocked && item.genreLocked) return "manual";
  if (item.needsReview) return "needs_review";
  return "classified";
}

function feedbackPlacement(item: Product): FeedbackPlacement {
  return { category: item.category, genre: item.genre ?? null };
}

function feedbackProduct(item: Product) {
  return {
    boothId: item.boothId,
    title: item.title,
    shop: item.shop,
    descriptionHead: descriptionHead(item.descriptionSnippet),
    category: item.category,
    genre: item.genre ?? null,
    classificationStatus: feedbackStatus(item),
  };
}

function pushClassificationMove(
  action: FeedbackMove["action"],
  boothId: string,
  from: FeedbackPlacement,
  to: FeedbackPlacement | null,
) {
  classificationMoves.push({ boothId, at: new Date().toISOString(), action, from, to });
}

function classificationFeedback(scope: string): string {
  if (scope !== "other" && scope !== "unclassified" && scope !== "both") {
    throw new Error("対象が不正です");
  }
  const matches = (item: Product) => {
    const unclassified = item.category == null;
    const other = item.category === "other";
    if (scope === "other") return other;
    if (scope === "unclassified") return unclassified;
    return other || unclassified;
  };
  const byId = (left: Product, right: Product) => (left.boothId < right.boothId ? -1 : left.boothId > right.boothId ? 1 : 0);
  const products = activeProducts().filter(matches).sort(byId).map(feedbackProduct);
  const manualClassifications = activeProducts()
    .filter((item) => item.categoryLocked && item.genreLocked)
    .sort(byId)
    .map(feedbackProduct);
  return `${JSON.stringify({
    format: "vrclibman.classification-feedback",
    version: 1,
    scope,
    descriptionHeadChars: DESCRIPTION_HEAD_CHARS,
    purpose: CLASSIFICATION_FEEDBACK_PURPOSE,
    products,
    corrections: classificationMoves
      .filter((row) => !excludedProducts.has(row.boothId))
      .map((row) => ({ ...row, from: { ...row.from }, to: row.to ? { ...row.to } : null })),
    manualClassifications,
  }, null, 2)}\n`;
}

function previewSnapshot(item: Product): ClassificationSnapshot {
  return {
    boothId: item.boothId,
    category: item.category,
    genre: item.genre ?? null,
    categoryLocked: item.categoryLocked,
    genreLocked: Boolean(item.genreLocked),
    userConfirmed: item.userConfirmed,
    needsReview: item.needsReview,
  };
}

function movePreviewGenre(boothId: string, kind: string, genre: string): ClassificationSnapshot {
  if (kind !== "asset" && kind !== "world") throw new Error("ジャンルの種類が不正です");
  if (!genresForKind(kind).includes(genre)) throw new Error("ジャンルが不正です");
  const category = GENRE_CATEGORY[genre];
  if (!category || (category === "world") !== (kind === "world")) throw new Error("ジャンルが不正です");
  const item = store.products.find((product) => product.boothId === boothId);
  if (!item) throw new Error("商品が見つかりません");
  if (excludedProducts.has(boothId)) throw new Error("この商品は除外されています");
  const snapshot = previewSnapshot(item);
  const from = feedbackPlacement(item);
  const already = item.category === category && item.genre === genre && item.categoryLocked && Boolean(item.genreLocked);
  if (!already) {
    updateProduct({ boothId, category, genre });
    pushClassificationMove("move", boothId, from, { category, genre });
  }
  return snapshot;
}

function restorePreviewClassification(raw: unknown) {
  const snapshot = raw as ClassificationSnapshot;
  const item = store.products.find((product) => product.boothId === snapshot?.boothId);
  if (!item) throw new Error("商品が見つかりません");
  const from = feedbackPlacement(item);
  const categoryLockedBefore = item.categoryLocked;
  const genreLockedBefore = Boolean(item.genreLocked);
  item.category = snapshot.category as Category | null;
  item.genre = snapshot.genre;
  item.categoryLocked = snapshot.categoryLocked;
  item.genreLocked = snapshot.genreLocked;
  item.userConfirmed = snapshot.userConfirmed;
  item.needsReview = snapshot.needsReview;
  const to = feedbackPlacement(item);
  if (
    snapshot.links == null &&
    (from.category !== to.category ||
      from.genre !== to.genre ||
      categoryLockedBefore !== item.categoryLocked ||
      genreLockedBefore !== Boolean(item.genreLocked))
  ) {
    pushClassificationMove("restore", item.boothId, from, to);
  }
  if (typeof snapshot.linksLocked === "boolean") item.linksLocked = snapshot.linksLocked;
  if (Array.isArray(snapshot.compatibleAvatars)) item.compatibleAvatars = [...snapshot.compatibleAvatars];
  if (Array.isArray(snapshot.unresolvedNames)) item.unresolvedNames = [...snapshot.unresolvedNames];
  if (typeof snapshot.isGeneric === "boolean") item.isGeneric = snapshot.isGeneric;
  if (Array.isArray(snapshot.links)) {
    store.links = store.links.filter((link) => link.productId !== item.boothId);
    for (const link of snapshot.links) {
      store.links.push({ productId: item.boothId, avatarId: link.avatarId, source: link.source });
    }
  }
  for (const avatar of snapshot.ownedAvatars ?? []) applyOwnedRestore(item.boothId, avatar);
}

function previewLinks(boothId: string): SnapshotLink[] {
  return store.links
    .filter((link) => link.productId === boothId)
    .map((link) => ({ avatarId: link.avatarId, source: link.source ?? "extracted" }));
}

function categoryFromGenre(genre: string | null | undefined): Category | null {
  if (!genre) return null;
  return GENRE_CATEGORY[genre] ?? null;
}

function otherAvatarBodies(avatarId: string, exceptBoothId: string): boolean {
  return store.products.some(
    (product) =>
      product.boothId !== exceptBoothId &&
      product.category === "avatar" &&
      store.links.some((link) => link.productId === product.boothId && link.avatarId === avatarId),
  );
}

function applyOwnedRestore(boothId: string, avatar: OwnedAvatarRestore) {
  let row = store.avatars.find((item) => item.id === avatar.id);
  if (avatar.owned) {
    if (!row) {
      if (!avatar.id.startsWith("owned-")) throw new Error(`アバター ${avatar.id} が対応表にありません`);
      row = {
        id: avatar.id,
        displayName: avatar.displayName,
        aliases: [...avatar.aliases],
        baseBody: null,
        notes: "",
        isBuiltin: false,
        owned: true,
      };
      store.avatars.push(row);
    }
    row.owned = true;
    return;
  }
  if (otherAvatarBodies(avatar.id, boothId)) return;
  if (row) row.owned = false;
  const linked = store.links.some((link) => link.avatarId === avatar.id);
  if (avatar.removeIfUnused && avatar.id.startsWith("owned-") && !linked && row && !row.isBuiltin) {
    store.avatars = store.avatars.filter((item) => item.id !== avatar.id);
  }
}

function unpinPreviewClassification(boothId: string): ClassificationSnapshot {
  const item = store.products.find((product) => product.boothId === boothId);
  if (!item) throw new Error("商品が見つかりません");
  if (!(item.categoryLocked && item.genreLocked)) throw new Error("この商品は手動で固定されていません");
  const snapshot = previewSnapshot(item);
  const from = feedbackPlacement(item);
  const body = store.links.find(
    (link) => link.productId === boothId && (link.source === "owned" || link.source === "catalog"),
  );
  if (item.category === "avatar" && item.linksLocked && body) {
    const avatar = store.avatars.find((row) => row.id === body.avatarId);
    snapshot.linksLocked = true;
    snapshot.links = previewLinks(boothId);
    snapshot.compatibleAvatars = [...item.compatibleAvatars];
    snapshot.unresolvedNames = [...item.unresolvedNames];
    snapshot.isGeneric = item.isGeneric;
    if (avatar) {
      snapshot.ownedAvatars = [{
        id: avatar.id,
        displayName: avatar.displayName,
        aliases: [...avatar.aliases],
        owned: true,
        removeIfUnused: false,
      }];
    }
    item.category = categoryFromGenre(item.genre);
    item.categoryLocked = false;
    item.genreLocked = false;
    item.linksLocked = false;
    store.links = store.links.filter((link) => link.productId !== boothId);
    if (avatar && !otherAvatarBodies(avatar.id, boothId)) avatar.owned = false;
    return snapshot;
  }
  item.categoryLocked = false;
  item.genreLocked = false;
  pushClassificationMove("unpin", boothId, from, null);
  return snapshot;
}

function previewBoothCategory(row: Record<string, unknown>): { id: number | null; name: string; parent: string } | null {
  const category = row.category ?? row.boothCategory;
  if (!category || typeof category !== "object" || Array.isArray(category)) return null;
  const object = category as Record<string, unknown>;
  const rawId = object.id;
  const id = typeof rawId === "number" && Number.isInteger(rawId) && rawId > 0 ? rawId : null;
  const name = textOf(object.name).slice(0, 80);
  const parentValue = object.parent;
  const parent = typeof parentValue === "string"
    ? parentValue.trim().slice(0, 80)
    : parentValue && typeof parentValue === "object"
      ? textOf((parentValue as Record<string, unknown>).name).slice(0, 80)
      : "";
  if (id == null && !name && !parent) return null;
  return { id, name, parent };
}

function previewBoothTags(row: Record<string, unknown>): string[] | null {
  const value = row.tags ?? row.boothTags;
  if (!Array.isArray(value)) return null;
  const tags: string[] = [];
  for (const item of value) {
    const name = (typeof item === "string"
      ? item
      : item && typeof item === "object"
        ? textOf((item as Record<string, unknown>).name)
        : "").trim().slice(0, 40);
    if (!name || tags.includes(name)) continue;
    tags.push(name);
    if (tags.length === 40) break;
  }
  return tags;
}

function applyPreviewBooth(target: Product, category: { id: number | null; name: string; parent: string } | null, tags: string[] | null): boolean {
  let changed = false;
  if (category && (
    target.boothCategoryId !== category.id
    || target.boothCategoryName !== category.name
    || target.boothCategoryParent !== category.parent
  )) {
    target.boothCategoryId = category.id;
    target.boothCategoryName = category.name;
    target.boothCategoryParent = category.parent;
    changed = true;
  }
  if (tags && (target.boothTags ?? []).join("\0") !== tags.join("\0")) {
    target.boothTags = tags;
    changed = true;
  }
  return changed;
}

function avatarByLabel(label: string): AvatarRecord | "ambiguous" | null {
  const key = label.trim().toLocaleLowerCase("ja");
  if (!key) return null;
  const hits = store.avatars.filter(
    (avatar) =>
      avatar.displayName.toLocaleLowerCase("ja") === key ||
      avatar.aliases.some((alias) => alias.toLocaleLowerCase("ja") === key),
  );
  if (hits.length === 1) return hits[0];
  if (hits.length > 1) return "ambiguous";
  return null;
}

function registerPreviewOwnedBody(boothId: string, name: string | null): RegisterOwnedBodyResult {
  const item = store.products.find((product) => product.boothId === boothId);
  if (!item) throw new Error("商品が見つかりません");
  if (excludedProducts.has(boothId)) throw new Error("この商品は除外されています");
  if (item.category === "avatar") throw new Error("この商品はすでにアバター本体です");
  const target = previewOwnedTarget(item, name);
  if ("needsName" in target) {
    return { kind: "needsName", suggestedName: target.suggestedName, candidates: target.candidates };
  }
  const previousOwned = store.avatars.find((avatar) => avatar.id === target.avatarId);
  const snapshot: ClassificationSnapshot = {
    ...previewSnapshot(item),
    linksLocked: item.linksLocked,
    links: previewLinks(boothId),
    compatibleAvatars: [...item.compatibleAvatars],
    unresolvedNames: [...item.unresolvedNames],
    isGeneric: item.isGeneric,
    ownedAvatars:
      previousOwned?.owned
        ? []
        : [{
            id: target.avatarId,
            displayName: target.displayName,
            aliases: previousOwned ? [...previousOwned.aliases] : [],
            owned: false,
            removeIfUnused: !previousOwned,
          }],
  };
  if (!previousOwned) {
    store.avatars.push({
      id: target.avatarId,
      displayName: target.displayName,
      aliases: [],
      baseBody: null,
      notes: "",
      isBuiltin: false,
      owned: true,
    });
  } else {
    previousOwned.owned = true;
    if (target.avatarId.startsWith("owned-") && !previousOwned.isBuiltin) {
      previousOwned.displayName = target.displayName;
    }
  }
  item.category = "avatar";
  item.categoryLocked = true;
  item.genreLocked = true;
  item.linksLocked = true;
  item.userConfirmed = true;
  item.needsReview = false;
  item.isGeneric = false;
  item.compatibleAvatars = [target.displayName];
  store.links = store.links.filter((link) => link.productId !== boothId);
  store.links.push({ productId: boothId, avatarId: target.avatarId, source: "owned" });
  return { kind: "registered", snapshot, avatarId: target.avatarId, displayName: target.displayName };
}

function previewOwnedTarget(
  item: Product,
  name: string | null,
): { avatarId: string; displayName: string } | { needsName: true; suggestedName: string; candidates: string[] } {
  const supplied = name?.trim() ?? "";
  if (name != null && !supplied) throw new Error("アバター名を入力してください");
  if (supplied) {
    const hit = avatarByLabel(supplied);
    if (hit === "ambiguous") throw new Error("同じ名前のアバターが複数あります");
    if (hit) return { avatarId: hit.id, displayName: hit.displayName };
    return { avatarId: `owned-${item.boothId}`, displayName: supplied };
  }
  const labels = item.compatibleAvatars.map((label) => label.trim()).filter(Boolean);
  const extracted = labels.length > 0 ? labels : item.unresolvedNames.map((label) => label.trim()).filter(Boolean);
  if (extracted.length === 0) return { needsName: true, suggestedName: "", candidates: [] };
  const resolved: AvatarRecord[] = [];
  const unresolved: string[] = [];
  for (const label of extracted) {
    const hit = avatarByLabel(label);
    if (hit && hit !== "ambiguous") {
      if (!resolved.some((avatar) => avatar.id === hit.id)) resolved.push(hit);
    } else {
      unresolved.push(label);
    }
  }
  if (resolved.length === 1 && unresolved.length === 0) {
    return { avatarId: resolved[0].id, displayName: resolved[0].displayName };
  }
  if (resolved.length === 0 && unresolved.length === 1) {
    return { avatarId: `owned-${item.boothId}`, displayName: unresolved[0] };
  }
  const candidates: string[] = [];
  for (const avatar of resolved) {
    if (!candidates.includes(avatar.displayName)) candidates.push(avatar.displayName);
  }
  for (const label of unresolved) {
    if (!candidates.includes(label)) candidates.push(label);
  }
  return { needsName: true, suggestedName: "", candidates };
}

function textOf(value: unknown): string {
  if (typeof value === "string") return value.trim();
  if (typeof value === "number") return String(value);
  return "";
}

function unnamedIn(items: unknown[]): number {
  let count = 0;
  for (const raw of items) {
    if (!raw || typeof raw !== "object") continue;
    const row = raw as Record<string, unknown>;
    const list = Array.isArray(row.downloads) ? row.downloads : Array.isArray(row.files) ? row.files : [];
    for (const item of list) {
      if (!item || typeof item !== "object") continue;
      const file = item as Record<string, unknown>;
      const name = textOf(file.filename || file.name);
      const id = textOf(file.downloadableId || file.downloadable_id);
      if (!name || name === id || /^\d{4,12}$/.test(name)) count += 1;
    }
  }
  return count;
}

function importExport(json: string): ImportReport {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch (err) {
    throw new Error(`JSONを読めません: ${err instanceof Error ? err.message : "parse"}`);
  }
  const record = parsed as { products?: unknown };
  const items = Array.isArray(parsed)
    ? parsed
    : Array.isArray(record.products)
      ? record.products
      : null;
  if (!items) throw new Error("products 配列が見つかりません");
  let inserted = 0;
  let updated = 0;
  let unchanged = 0;
  let skipped = 0;
  let variations = 0;
  let downloads = 0;
  const warnings: string[] = [];
  for (const raw of items) {
    if (!raw || typeof raw !== "object") {
      skipped += 1;
      continue;
    }
    const row = raw as Record<string, unknown>;
    let boothId = textOf(row.boothId || row.booth_id || row.id);
    if (!/^\d{4,12}$/.test(boothId)) {
      const match = textOf(row.url).match(/\/items\/(\d+)/);
      boothId = match?.[1] ?? "";
    }
    const title = textOf(row.title || row.name).slice(0, 300);
    variations += Array.isArray(row.variations) ? row.variations.length : 0;
    downloads += Array.isArray(row.downloads) ? row.downloads.length : Array.isArray(row.files) ? row.files.length : 0;
    if (!/^\d{4,12}$/.test(boothId) || !title) {
      skipped += 1;
      warnings.push("商品IDかタイトルがないデータをスキップしました");
      continue;
    }
    const adult = row.isAdult === true || row.is_adult === true;
    const category = adult ? null : previewBoothCategory(row);
    const tags = adult ? null : previewBoothTags(row);
    const incoming = {
      title,
      shop: textOf(row.shop || row.shopName).slice(0, 120),
      descriptionSnippet: adult ? "" : textOf(row.descriptionSnippet || row.description).slice(0, 8000),
      url: textOf(row.url) || `https://booth.pm/ja/items/${boothId}`,
      thumbnailUrl: textOf(row.thumbnailUrl || row.imageUrl) || null,
    };
    const existing = store.products.find((item) => item.boothId === boothId);
    if (!existing) {
      const created = product({
          boothId,
          title: incoming.title,
          shop: incoming.shop,
          descriptionSnippet: incoming.descriptionSnippet,
          url: incoming.url,
          thumbnailUrl: incoming.thumbnailUrl,
          category: null,
          categoryLocked: false,
          confidence: null,
          needsReview: true,
        });
      applyPreviewBooth(created, category, tags);
      store.products.push(created);
      inserted += 1;
      continue;
    }
    const metaChanged = applyPreviewBooth(existing, category, tags);
    const same =
      existing.title === incoming.title &&
      existing.shop === incoming.shop &&
      existing.descriptionSnippet === incoming.descriptionSnippet &&
      existing.url === incoming.url &&
      existing.thumbnailUrl === incoming.thumbnailUrl &&
      !metaChanged;
    if (same) {
      unchanged += 1;
      continue;
    }
    const descriptionChanged = existing.descriptionSnippet !== incoming.descriptionSnippet;
    existing.title = incoming.title;
    existing.shop = incoming.shop;
    existing.url = incoming.url;
    existing.thumbnailUrl = incoming.thumbnailUrl;
    if (descriptionChanged && incoming.descriptionSnippet) {
      existing.descriptionSnippet = incoming.descriptionSnippet;
      if (!existing.categoryLocked) {
        existing.category = null;
        existing.confidence = null;
        existing.needsReview = true;
        if (!existing.linksLocked) {
          store.links = store.links.filter((link) => link.productId !== existing.boothId);
        }
      }
    }
    updated += 1;
  }
  if (inserted + updated + unchanged > 0 && variations === 0 && downloads === 0) {
    warnings.push(
      "バリエーションもダウンロードも 0 件です。ファイルの有無は分かりません。拡張の「解析結果をコピー」で構造を確認するか、ライブラリを再インポートするか、フォルダを調べてください",
    );
  }
  return {
    inserted,
    updated,
    unchanged,
    skipped,
    variations,
    downloads,
    unnamedDownloads: unnamedIn(items),
    warnings: warnings.slice(0, 8),
  };
}

function slug(name: string): string {
  const ascii = name
    .normalize("NFKC")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
  const base = ascii && /^[a-z]/.test(ascii) ? ascii : `a${Math.abs(hashCode(name))}`;
  let id = base.slice(0, 64);
  let suffix = 2;
  while (store.avatars.some((avatar) => avatar.id === id) || store.baseBodies.some((body) => body.id === id)) {
    id = `${base.slice(0, 60)}-${suffix}`;
    suffix += 1;
  }
  return id;
}

function hashCode(value: string): number {
  let hash = 0;
  for (const char of value) hash = (hash * 31 + char.charCodeAt(0)) | 0;
  return hash;
}

function saveAvatar(input: AvatarInput): string {
  const displayName = input.displayName.trim();
  if (!displayName) throw new Error("表示名を入力してください");
  const aliases = input.aliases.map((alias) => alias.trim()).filter(Boolean).slice(0, 32);
  if (input.id) {
    const avatar = store.avatars.find((item) => item.id === input.id);
    if (!avatar) throw new Error("アバターが見つかりません");
    avatar.displayName = displayName;
    avatar.aliases = aliases;
    avatar.baseBody = input.baseBody ?? null;
    avatar.notes = input.notes;
    return avatar.id;
  }
  const id = slug(displayName);
  store.avatars.push({
    id,
    displayName,
    aliases,
    baseBody: input.baseBody ?? null,
    notes: input.notes,
    isBuiltin: false,
    owned: false,
  });
  return id;
}

function desktopOnly(): never {
  throw new Error("この操作はデスクトップアプリで使えます。ブラウザ確認用の見本では動きません。");
}

const previewProgress = new EventTarget();
let previewLibraryCancel = false;

export function emitPreviewProgress(progress: JobProgress): void {
  previewProgress.dispatchEvent(new CustomEvent("progress", { detail: progress }));
}

export function listenPreviewProgress(onProgress: (progress: JobProgress) => void): () => void {
  const listener = (event: Event) => {
    const detail = (event as CustomEvent<JobProgress>).detail;
    onProgress(detail);
  };
  previewProgress.addEventListener("progress", listener);
  return () => previewProgress.removeEventListener("progress", listener);
}

const previewUpdaterBus = new EventTarget();
let previewUpdaterStatus: AppUpdateStatus = {
  revision: 0,
  phase: "unavailable",
  version: null,
  notes: null,
  downloaded: 0,
  total: null,
  error: null,
  message: "アプリ内更新はインストールした配布版で利用できます。",
};

function emitPreviewUpdater(status: AppUpdateStatus): void {
  previewUpdaterStatus = status;
  previewUpdaterBus.dispatchEvent(new CustomEvent("updater", { detail: status }));
}

export function listenPreviewUpdaterChanged(onStatus: (status: AppUpdateStatus) => void): () => void {
  const listener = (event: Event) => {
    onStatus((event as CustomEvent<AppUpdateStatus>).detail);
  };
  previewUpdaterBus.addEventListener("updater", listener);
  return () => previewUpdaterBus.removeEventListener("updater", listener);
}

function previewWalkMessage(
  label: string,
  page: number,
  last: number,
  products: number,
  files: number,
  named: number,
  unnamed: number,
): string {
  return `${label} ${page}/${last} ページ（商品 ${products}、ファイル ${files}、名前あり ${named}、名前なし ${unnamed}）`;
}

async function previewLibraryWalk(): Promise<LibraryWalkReport> {
  if (!store.settings.builtinBrowser) {
    throw new Error("内蔵ブラウザは設定で有効にしてから使います");
  }
  previewLibraryCancel = false;
  const steps = [
    { label: "ライブラリ", page: 1, last: 2, products: 1, files: 2, named: 1, unnamed: 1 },
    { label: "ライブラリ", page: 2, last: 2, products: 2, files: 3, named: 2, unnamed: 1 },
    { label: "ギフト", page: 1, last: 1, products: 3, files: 4, named: 2, unnamed: 2 },
  ];
  let seen = steps[0];
  for (let index = 0; index < steps.length; index += 1) {
    if (previewLibraryCancel) break;
    seen = steps[index];
    emitPreviewProgress({
      job: "browser-library",
      current: 0,
      total: 0,
      message: previewWalkMessage(seen.label, seen.page, seen.last, seen.products, seen.files, seen.named, seen.unnamed),
    });
    await new Promise((resolve) => setTimeout(resolve, 800));
    if (previewLibraryCancel) break;
  }
  if (!previewLibraryCancel) {
    emitPreviewProgress({
      job: "browser-library",
      current: 1,
      total: 2,
      message: "商品情報 1/2",
    });
  }
  const cancelled = previewLibraryCancel;
  const report: LibraryWalkReport = {
    products: seen.products,
    files: seen.files,
    namedFiles: seen.named,
    unnamedFiles: seen.unnamed,
    pages: cancelled ? 1 : 3,
    cancelled,
    notice: "",
    inserted: seen.products,
    updated: 0,
    unchanged: 0,
  };
  emitPreviewProgress({
    job: "browser-library",
    current: 1,
    total: 1,
    message: `商品 ${report.products}、ファイル ${report.files}、名前あり ${report.namedFiles}、名前なし ${report.unnamedFiles}`,
  });
  return report;
}

let boothPageIntervalMs = 1000;
let importMode: BoothImportMode | null = null;
const installedModels = ["qwen3.5:9b", "gemma4:12b", "qwen3:4b"];
const previewIntegration = {
  paired: true,
  expectedExtensionVersion: extensionManifest.version,
  extensionVersion: "0.1.10",
  extensionLastSeenAt: "2026-10-24T01:00:00Z",
  extensionImport: { at: "2026-10-24T01:00:00Z", products: 3, inserted: 1, updated: 1, unchanged: 1, skipped: 0 },
  browserImport: null,
};

/** 見本のクラウド分類。キーの文字列は持たず、保存済みかどうかだけを再現する。 */
let previewCloudKeySaved = true;

function previewCloudBaseUrl(value: string): string {
  const trimmed = value.trim().replace(/\/+$/, "");
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    throw new Error("クラウドAPIの接続先 URL が正しくありません");
  }
  const loopback = ["127.0.0.1", "localhost", "[::1]"].includes(url.hostname);
  if (url.protocol !== "https:" && !(url.protocol === "http:" && loopback)) throw new Error("クラウドAPIの接続先は https にしてください");
  if (url.username || url.password) throw new Error("クラウドAPIの接続先に認証情報は入れられません。キーは別欄に保存します");
  if (url.search || url.hash) throw new Error("クラウドAPIの接続先に ? や # は入れられません");
  return trimmed;
}

export async function dispatch<T>(command: string, args: Record<string, unknown>): Promise<T> {
  notePreviewCall(command);
  switch (command) {
    case "updater_status":
      return previewUpdaterStatus as T;
    case "check_app_update": {
      const manual = args.manual !== false;
      const next: AppUpdateStatus = {
        revision: previewUpdaterStatus.revision + 1,
        phase: "available",
        version: "99.0.0",
        notes: "見本の更新内容",
        downloaded: 0,
        total: null,
        error: manual ? null : null,
        message: null,
      };
      emitPreviewUpdater(next);
      return next as T;
    }
    case "download_app_update": {
      const next: AppUpdateStatus = {
        ...previewUpdaterStatus,
        revision: previewUpdaterStatus.revision + 1,
        phase: "ready",
        downloaded: 1024,
        total: 1024,
        error: null,
      };
      emitPreviewUpdater(next);
      return next as T;
    }
    case "install_app_update": {
      const next: AppUpdateStatus = {
        ...previewUpdaterStatus,
        revision: previewUpdaterStatus.revision + 1,
        phase: "ready",
        error: "見本では再起動しません",
      };
      emitPreviewUpdater(next);
      return next as T;
    }
    case "list_managed_products":
      return store.products.map((p) => ({ boothId: p.boothId, title: p.title, shop: p.shop, category: p.category, excluded: excludedProducts.has(p.boothId) })) as T;
    case "set_product_excluded": {
      const id = String(args.boothId ?? "");
      if (!store.products.some((p) => p.boothId === id)) throw new Error("商品が見つかりません");
      if (args.excluded === true) excludedProducts.add(id); else excludedProducts.delete(id);
      return undefined as T;
    }
    case "library_stats":
      return stats() as T;
    case "list_avatars":
      return cards() as T;
    case "get_avatar":
      return detail(String(args.id ?? "")) as T;
    case "list_products_by_category":
      return activeProducts()
        .filter((item) => item.category === args.category)
        .map((item) => itemView(item, false)) as T;
    case "list_products_by_genre": {
      const kind = String(args.kind); const wanted = args.genre == null ? null : String(args.genre);
      if (kind !== "asset" && kind !== "world") throw new Error("ジャンルの種類が不正です");
      // Same as Rust: unclassified products (no category) are not listed by genre.
      const rows = activeProducts().filter((item) => item.category != null && (kind === "world" ? item.category === "world" : item.category !== "world" && item.category !== "avatar"));
      const known: readonly string[] = genresForKind(kind);
      const bucket = (item: Product) => browseGenre(item, known);
      const counts: Record<string, number> = {}; for (const item of rows) counts[bucket(item)] = (counts[bucket(item)] ?? 0) + 1;
      return { items: rows.filter((item) => !wanted || bucket(item) === wanted).sort((a,b) => a.title.localeCompare(b.title, "ja")).map((item) => itemView(item, false)), counts, total: rows.length } as T;
    }
    case "probe_remote_files":
      store.remoteChecked = true;
      return detail(String(args.id ?? "")) as T;
    case "probe_remote_products": {
      const ids = Array.isArray(args.ids) ? args.ids.map((id) => String(id)) : [];
      if (previewRemoteProbeOutcome === "timeout") {
        await new Promise((resolve) => setTimeout(resolve, 15_000));
      }
      store.remoteChecked = true;
      const reachable = previewRemoteProbeOutcome !== "fail" && store.remoteReachable;
      return ids.map(
        (boothId): RemoteProductProbe => ({
          boothId,
          remoteChecked: true,
          remoteReachable: reachable,
          downloads: placedDownloads(boothId),
        }),
      ) as T;
    }
    case "list_queue":
      return queue(String(args.kind ?? "review")) as T;
    case "list_product_options":
      return activeProducts()
        .map((item) => ({ boothId: item.boothId, title: item.title, shop: item.shop, category: item.category }))
        .sort((left, right) => left.title.localeCompare(right.title, "ja")) as T;
    case "list_knowledge":
      return { avatars: store.avatars, baseBodies: store.baseBodies } satisfies Knowledge as T;
    case "integration_status":
      return { ...previewIntegration, mode: !store.settings.builtinBrowser ? "extension" : importMode === "browser" ? "browser" : "both" } satisfies IntegrationStatus as T;
    case "set_booth_import_mode": {
      const mode = args.mode as BoothImportMode;
      if (!["extension", "browser", "both"].includes(mode)) throw new Error("BOOTH の読み込み方が不正です");
      importMode = mode;
      store.settings.builtinBrowser = mode !== "extension";
      return undefined as T;
    }
    case "get_settings":
      return { ...store.settings } as T;
    case "get_booth_page_interval":
      return boothPageIntervalMs as T;
    case "set_booth_page_interval": {
      const next = Number(args.intervalMs);
      if (![1000, 500, 250].includes(next)) throw new Error("BOOTH のアクセス間隔が不正です");
      boothPageIntervalMs = next;
      return undefined as T;
    }
    case "extension_link":
      return { ...extensionLink } as T;
    case "regenerate_extension_token":
      extensionLink.token = extensionLink.token === "K7NP-4Q2M" ? "M8QR-5S3N" : "K7NP-4Q2M";
      return { ...extensionLink } as T;
    case "remote_sync_status":
      return { running: false, copied: 0, skipped: 0, lastCompletedAt: null, error: null } as T;
    case "update_settings":
    case "save_settings": {
      const settings = command === "update_settings" ? { ...store.settings, ...(args.patch as Partial<Settings>) } : args.settings as Settings;
      if (!/^http:\/\/(127\.0\.0\.1|localhost|\[::1\])(:\d+)?$/i.test(settings.ollamaBaseUrl.replace(/\/$/, ""))) throw new Error("Ollamaは手元のPC（localhost）だけに接続します");
      if (!/^[a-zA-Z0-9._:/-]{1,80}$/.test(settings.ollamaModel.trim())) throw new Error("モデル名が不正です");
      if (settings.remoteAutoSync && !settings.remoteLibraryFolder.trim()) throw new Error("自動同期をオンにする前に、リモートライブラリのフォルダを選んでください");
      if (settings.routeDownloads && !settings.libraryFolder.trim()) throw new Error("自動整理をオンにする前に、ライブラリのフォルダを選んでください");
      const backgroundMode = Boolean(settings.backgroundMode);
      const boothLinkTarget = settings.boothLinkTarget === "builtin" ? "builtin" : "external";
      const cloudBaseUrl = previewCloudBaseUrl(settings.cloudBaseUrl ?? "https://api.openai.com/v1");
      const cloudModel = String(settings.cloudModel ?? "").trim();
      if (cloudModel.length > 200 || /[\u0000-\u001f]/.test(cloudModel)) throw new Error("クラウドAPIのモデル名に使えない文字があります");
      const llmBackend = settings.llmBackend === "apple" || settings.llmBackend === "cloud" ? settings.llmBackend : "ollama";
      store.settings = { ...settings, llmBackend, cloudBaseUrl, cloudModel, ollamaModel: settings.ollamaModel.trim(), backgroundMode, launchAtLogin: backgroundMode && Boolean(settings.launchAtLogin), confidenceThreshold: Math.min(1, Math.max(0, settings.confidenceThreshold)), boothLinkTarget };
      return (command === "update_settings" ? { ...store.settings } : undefined) as T;
    }
    case "import_export":
      return importExport(String(args.json ?? "")) as T;
    case "update_product":
      updateProduct(args.update as ProductUpdate);
      return undefined as T;
    case "move_product_genre":
      return movePreviewGenre(String(args.boothId ?? ""), String(args.kind ?? ""), String(args.genre ?? "")) as T;
    case "restore_product_classification":
      restorePreviewClassification(args.snapshot);
      return undefined as T;
    case "unpin_product_classification":
      return unpinPreviewClassification(String(args.boothId ?? "")) as T;
    case "classification_feedback":
      return classificationFeedback(String(args.scope ?? "")) as T;
    case "save_classification_feedback":
      classificationFeedback(String(args.scope ?? ""));
      if (!String(args.path ?? "").toLowerCase().endsWith(".json")) throw new Error("保存先は JSON ファイルにしてください");
      return undefined as T;
    case "register_owned_body":
      return registerPreviewOwnedBody(String(args.boothId ?? ""), args.name == null ? null : String(args.name)) as T;
    case "attach_product_to_avatar": {
      const avatarId = String(args.avatarId ?? "");
      const boothId = String(args.boothId ?? "");
      if (!store.avatars.some((avatar) => avatar.id === avatarId)) {
        throw new Error(`アバター ${avatarId} が対応表にありません`);
      }
      if (excludedProducts.has(boothId)) throw new Error("この商品は除外されています");
      const item = store.products.find((product) => product.boothId === boothId);
      if (!item) throw new Error("商品が見つかりません");
      if (item.category === "avatar") throw new Error("アバター本体は衣装として追加できません");
      if (item.category === "world") throw new Error("ワールドはアバターに追加できません");
      const existing = store.links.find((link) => link.productId === boothId && link.avatarId === avatarId);
      if (!existing) store.links.push({ productId: boothId, avatarId });
      item.linksLocked = true;
      return undefined as T;
    }
    case "link_file": {
      const file = store.files.find((item) => item.path === args.path);
      if (!file) throw new Error("ファイルがスキャン結果にありません");
      const productId = (args.productId as string | null) || null;
      if (productId && !store.products.some((item) => item.boothId === productId)) {
        throw new Error("商品が見つかりません");
      }
      file.linkedProductId = productId;
      file.matchScore = productId ? 1 : file.matchScore;
      return undefined as T;
    }
    case "save_avatar":
      return saveAvatar(args.avatar as AvatarInput) as T;
    case "delete_avatar": {
      const avatar = store.avatars.find((item) => item.id === args.id);
      if (!avatar) throw new Error("アバターが見つかりません");
      if (avatar.isBuiltin) throw new Error("初期データのアバターは削除できません");
      store.avatars = store.avatars.filter((item) => item.id !== avatar.id);
      store.links = store.links.filter((link) => link.avatarId !== avatar.id);
      return undefined as T;
    }
    case "save_base_body": {
      const body = args.body as { id?: string | null; displayName: string; note: string };
      const displayName = body.displayName.trim();
      if (!displayName) throw new Error("表示名を入力してください");
      if (body.id) {
        const existing = store.baseBodies.find((item) => item.id === body.id);
        if (!existing) throw new Error("素体が見つかりません");
        existing.displayName = displayName;
        existing.note = body.note;
        return existing.id as T;
      }
      const id = slug(displayName);
      store.baseBodies.push({ id, displayName, note: body.note });
      return id as T;
    }
    case "delete_base_body":
      store.baseBodies = store.baseBodies.filter((item) => item.id !== args.id);
      for (const avatar of store.avatars) {
        if (avatar.baseBody === args.id) avatar.baseBody = null;
      }
      return undefined as T;
    case "merge_avatars": {
      const from = store.avatars.find((item) => item.id === args.fromId);
      const into = store.avatars.find((item) => item.id === args.intoId);
      if (!from || !into) throw new Error("アバターが見つかりません");
      if (from.isBuiltin) throw new Error("初期データのアバターは統合元にできません");
      if (from.id === into.id) throw new Error("同じアバターです");
      const aliases = new Set([...into.aliases, from.displayName, ...from.aliases]);
      into.aliases = [...aliases].slice(0, 32);
      store.links = store.links
        .map((link) => (link.avatarId === from.id ? { ...link, avatarId: into.id } : link))
        .filter((link, index, all) => all.findIndex((other) => other.productId === link.productId && other.avatarId === link.avatarId) === index);
      store.avatars = store.avatars.filter((item) => item.id !== from.id);
      return undefined as T;
    }
    case "restore_seed":
      return 0 as T;
    case "reresolve":
      return store.products.length as T;
    case "cancel_extract":
      return undefined as T;
    case "scan_library":
    case "classification_status": {
      const classifierVersion = 6;
      const staleCount = store.products.filter(
        (item) =>
          !(item.categoryLocked && item.genreLocked) &&
          item.category != null &&
          (item.classifierVersion ?? 1) < classifierVersion,
      ).length;
      return { classifierVersion, staleCount } as T;
    }
    case "extract_products":
      desktopOnly();
    case "ollama_loaded_models":
      return store.settings.llmBackend === "ollama"
        ? ([
            {
              name: store.settings.ollamaModel,
              sizeBytes: 2_500_000_000,
              expiresAt: "2026-10-27T12:00:00Z",
            },
          ] as T)
        : ([] as T);
    case "unload_ollama_model":
      return undefined as T;
    case "test_ollama": {
      const installed = [...installedModels];
      const selected = store.settings.ollamaModel;
      const modelReady = installed.some((name) => name === selected);
      return {
        models: installed,
        modelReady,
        modelSizeBytes: modelReady ? 2_500_000_000 : null,
      } as T;
    }
    case "catalog_info":
      return {
        count: 932,
        fetchedOn: "2026-10-23",
        upstreamUpdated: "2026-02-12 18:36:02 JST",
        sourceUrl: "https://github.com/eringiriri/mochifitter_list",
        termsUrl: "https://github.com/eringiriri/mochifitter_list/blob/main/terms.html",
        author: "えりんぎ",
        credit: "もちふぃった～プロファイル一覧",
      } as T;
    case "setup_status":
      return {
        complete: store.setupComplete,
        model: store.settings.ollamaModel,
        defaultModel: "qwen3.5:9b",
        backend: store.settings.llmBackend,
      } as T;
    case "cloud_key_status":
      return previewCloudKeySaved as T;
    case "save_cloud_api_key": {
      const key = String(args.key ?? "").trim();
      if (!key) throw new Error("APIキーを入力してください");
      if (!/^[\x21-\x7e]+$/.test(key)) throw new Error("APIキーに使えない文字（空白や全角文字）があります");
      previewCloudKeySaved = true;
      return true as T;
    }
    case "delete_cloud_api_key":
      previewCloudKeySaved = false;
      return false as T;
    case "test_cloud_classifier": {
      if (!previewCloudKeySaved) {
        return { ok: false, kind: "キー未保存", message: "クラウドAPIのキーが保存されていません。設定で保存してください", modelListed: null } as T;
      }
      const model = store.settings.cloudModel.trim();
      return {
        ok: true,
        kind: "成功",
        message: model ? `接続できました。モデル ${model} を使えます（見本）` : "接続できました。モデル名を入力してください（見本）",
        modelListed: model ? true : null,
      } as T;
    }
    case "apple_fm_status":
      return {
        availability: "available",
        variant: "default",
        contextSize: 8192,
        message: "見本では Apple のオンデバイスモデルを選べます",
      } as T;
    case "complete_setup":
      store.setupComplete = true;
      return undefined as T;
    case "reopen_setup":
      store.setupComplete = false;
      return undefined as T;
    case "avatar_candidates":
      return candidates() as T;
    case "add_free_product": {
      const url = String(args.url ?? "");
      const match = url.match(/\/items\/(\d{4,12})/);
      if (!match) throw new Error("商品のURLからIDを読めません");
      const boothId = match[1];
      const existing = store.products.find((item) => item.boothId === boothId);
      if (existing) {
        existing.isFree = true;
        return boothId as T;
      }
      store.products.push(
        product({
          boothId,
          title: `商品 ${boothId}`,
          shop: "",
          category: null,
          needsReview: true,
          descriptionSnippet: "",
          isFree: true,
        }),
      );
      return boothId as T;
    }
    case "booth_account": {
      if (!store.settings.builtinBrowser || !previewBoothLoggedIn) {
        return { loggedIn: false, displayName: "", pixivId: "", avatarUrl: "" } as T;
      }
      return { loggedIn: true, displayName: "テスト利用者", pixivId: "424242", avatarUrl: "" } as T;
    }
    case "download_booth_file": {
      if (!store.settings.builtinBrowser) {
        throw new Error("内蔵ブラウザはオフです。拡張のダウンロードを使ってください");
      }
      if (!store.settings.routeDownloads) {
        throw new Error("ダウンロードの自動整理はオフです");
      }
      if (!store.settings.libraryFolder.trim()) {
        throw new Error("ライブラリのフォルダを選んでから、自動整理をオンにしてください");
      }
      const boothId = String(args.boothId ?? "");
      const variation = String(args.variation ?? "");
      const filename = String(args.filename ?? "");
      const row = store.downloads.find(
        (item) => item.boothId === boothId && item.variation === variation && item.filename === filename,
      );
      if (!row) throw new Error("ダウンロード項目がありません");
      const number = /^\d{4,12}$/.test(filename);
      if (!number && !/\.(unitypackage|zip|rar|7z|vrm)$/i.test(filename)) {
        throw new Error("この行にはダウンロード番号がありません。拡張を更新して再インポートしてください");
      }
      row.state = "local";
      row.localPath = `${store.settings.libraryFolder}\\${boothId}\\${filename}`;
      return undefined as T;
    }
    case "pull_remote_file": {
      const row = store.downloads.find(
        (item) =>
          item.boothId === args.boothId &&
          item.variation === String(args.variation ?? "") &&
          item.filename === args.filename,
      );
      if (!row || row.state !== "remote") throw new Error("リモートにファイルがありません");
      if (!store.settings.remoteLibraryFolder.trim() || !store.remoteReachable) {
        throw new Error("リモートのフォルダに届きません。接続を確認してからもう一度");
      }
      if (!store.settings.libraryFolder.trim()) {
        throw new Error("先にフォルダを調べて、ライブラリの場所を決めてください");
      }
      row.state = "local";
      row.localPath = `${store.settings.libraryFolder}\\${row.boothId}\\${row.filename}`;
      return undefined as T;
    }
    case "push_remote_file": {
      const row = store.downloads.find(
        (item) =>
          item.boothId === args.boothId &&
          item.variation === String(args.variation ?? "") &&
          item.filename === args.filename,
      );
      if (!row || row.state !== "local" || !row.localPath) throw new Error("手元にファイルがありません");
      if (!store.settings.remoteLibraryFolder.trim() || !store.remoteReachable) {
        throw new Error("リモートのフォルダに届きません。接続を確認してからもう一度");
      }
      row.remotePath = `${store.settings.remoteLibraryFolder}\\${row.boothId}\\${row.filename}`;
      return undefined as T;
    }
    case "mark_owned_avatar": {
      const boothId = String(args.boothId ?? "");
      const product = store.products.find((item) => item.boothId === boothId);
      if (!product) throw new Error("商品が見つかりません");
      if (product.category && product.category !== "avatar") {
        throw new Error("この商品はアバター本体ではない分類です");
      }
      let avatarId = store.links.find((link) => link.productId === boothId)?.avatarId;
      if (!avatarId) {
        avatarId = `owned-${boothId}`;
        store.avatars.push({
          id: avatarId,
          displayName: product.title,
          aliases: [],
          baseBody: null,
          notes: "",
          isBuiltin: false,
          owned: true,
        });
        store.links.push({ productId: boothId, avatarId });
      }
      const avatar = store.avatars.find((item) => item.id === avatarId);
      if (avatar) avatar.owned = true;
      product.category = "avatar";
      product.needsReview = false;
      product.userConfirmed = true;
      return avatarId as T;
    }
    case "register_avatar_candidate": {
      const name = String(args.name ?? "").trim();
      if (!store.avatars.some((avatar) => avatar.displayName === name)) {
        store.avatars.push({
          id: slug(name),
          displayName: name,
          aliases: [],
          baseBody: null,
          notes: "",
          isBuiltin: false,
          owned: false,
        });
      }
      for (const product of store.products) {
        product.unresolvedNames = product.unresolvedNames.filter((item) => item !== name);
      }
      return slug(name) as T;
    }
    case "alias_avatar_candidate": {
      const name = String(args.name ?? "").trim();
      const avatar = store.avatars.find((item) => item.id === String(args.avatarId ?? ""));
      if (!avatar) throw new Error("アバターが見つかりません");
      if (!avatar.aliases.includes(name) && avatar.displayName !== name) avatar.aliases.push(name);
      for (const product of store.products) {
        product.unresolvedNames = product.unresolvedNames.filter((item) => item !== name);
      }
      return undefined as T;
    }
    case "pull_ollama_model": {
      const model = String(args.model ?? "").trim();
      if (!/^[a-zA-Z0-9._:/-]{1,80}$/.test(model)) throw new Error("モデル名が不正です");
      if (!installedModels.includes(model)) installedModels.push(model);
      return undefined as T;
    }
    case "refresh_avatar_catalog":
    case "booth_browser_log":
      return { path: "", text: "見本では記録はありません" } as T;
    case "browser_state": return previewBrowserState() as T;
    case "browser_action": return previewBrowserAction(args.action as BrowserAction) as T;
    case "save_browser_bookmark": return previewSaveBookmark(args.originalUrl as string | null, String(args.title ?? ""), String(args.url ?? "")) as T;
    case "delete_browser_bookmark": return previewDeleteBookmark(String(args.url ?? "")) as T;
    case "open_booth_browser": previewOpenBrowser(); return undefined as T;
    case "show_booth_library": previewBrowserAction({ kind: "navigate", url: "https://accounts.booth.pm/library" }); return undefined as T;
    case "browse_booth_history": previewBrowserAction({ kind: args.forward ? "forward" : "back" }); return undefined as T;
    case "import_booth_library": return (await previewLibraryWalk()) as T;
    case "close_booth_browser": previewCloseBrowser(); return undefined as T;
    case "cancel_booth_library":
      previewLibraryCancel = true;
      return undefined as T;
    case "clear_booth_session":
      previewBoothLoggedIn = false;
      return undefined as T;
    case "cache_product_preview": {
      const boothId = String(args.boothId ?? "");
      if (!/^\d{4,12}$/.test(boothId)) throw new Error("商品IDが不正です");
      const item = store.products.find((row) => row.boothId === boothId);
      if (!item) return null as T;
      if (item.isAdult && args.reveal !== true) return null as T;
      if (!item.thumbnailUrl) return null as T;
      const hit = previewImageCache.get(boothId);
      if (hit) return hit as T;
      if (args.download !== true) return null as T;
      previewImageCache.set(boothId, item.thumbnailUrl);
      return item.thumbnailUrl as T;
    }
    case "import_booth_page":
    case "cache_thumbnails":
    case "route_staged_download":
      desktopOnly();
    case "list_staged_downloads":
      return [] as T;
    default:
      throw new Error(`未対応の操作です: ${command}`);
  }
}
