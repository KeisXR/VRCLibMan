import extensionManifest from "../../extension/manifest.json";
import { filePresence } from "../media";
import type {
  AvatarCard,
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
  LibraryStats,
  OwnedBody,
  ProductUpdate,
  QueueFile,
  QueuePage,
  QueueProduct,
  Section,
  Settings,
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
  links: { productId: string; avatarId: string }[];
  settings: Settings;
  setupComplete: boolean;
  variations: { boothId: string; name: string; owned: boolean }[];
  downloads: (DownloadView & { boothId: string })[];
  remoteReachable: boolean;
  remoteChecked: boolean;
}

function product(partial: Partial<Product> & Pick<Product, "boothId" | "title" | "category">): Product {
  return {
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
    remoteLibraryFolder: "\\\\nas\\vrclibman",
    routeDownloads: false,
    llmBackend: "ollama",
    backgroundMode: false,
    launchAtLogin: false,
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
      boothId: "8800112",
      title: "年齢制限のアクセサリ",
      shop: "サンプル工房",
      category: "accessory",
      isAdult: true,
      descriptionSnippet: "",
    }),
    product({
      boothId: "8800102",
      title: "マヌカ用ヘア",
      shop: "サンプル工房",
      category: "hair",
      compatibleAvatars: ["マヌカ"],
    }),
    product({
      boothId: "8800103",
      title: "汎用ペンライト",
      shop: "サンプル工房",
      category: "gimmick",
      isGeneric: true,
      descriptionSnippet: "どのアバターでも使えるギミックです。",
    }),
    product({
      boothId: "8800120",
      title: "瞳のテクスチャ集",
      shop: "サンプル工房",
      category: "texture",
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
  ],
  links: [
    { productId: "4876459", avatarId: "lime" },
    { productId: "7770415", avatarId: "plum" },
    { productId: "5058077", avatarId: "manuka" },
    { productId: "8800101", avatarId: "lime" },
    { productId: "8800101", avatarId: "shinano" },
    { productId: "8800101", avatarId: "manuka" },
    { productId: "8800111", avatarId: "lime" },
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
  ],
  variations: [
    { boothId: "8800101", name: "通常", owned: true },
    { boothId: "8800101", name: "特典", owned: false },
  ],
  downloads: [
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

store.products.push(
  product({
    boothId: "8901001",
    title: "見本のワールド小物",
    shop: "見本",
    category: "world",
    descriptionSnippet: "ワールドに置く小物です。",
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
    remoteConfigured,
    remoteChecked,
    remoteReachable: remoteConfigured && store.remoteChecked && store.remoteReachable,
  };
}

function classify(avatar: AvatarRecord): {
  owned: OwnedBody[];
  sections: Section[];
  generic: ItemView[];
  thumbnailPath: string | null;
  thumbnailUrl: string | null;
} {
  const family = familyOf(avatar);
  const specific: { key: string; item: ItemView }[] = [];
  const generic: ItemView[] = [];
  const owned: OwnedBody[] = [];
  for (const item of activeProducts()) {
    if (item.category === "world") continue;
    const linked = new Set(linksOf(item.boothId));
    const inFamily = [...linked].some((id) => family.has(id));
    if (item.category === "avatar") {
      if (linked.has(avatar.id)) {
        owned.push({
          boothId: item.boothId,
          title: item.title,
          shop: item.shop,
          url: item.url,
          thumbnailUrl: item.thumbnailUrl,
          thumbnailPath: item.thumbnailPath,
        });
      }
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
    const incoming = {
      title,
      shop: textOf(row.shop || row.shopName).slice(0, 120),
      descriptionSnippet: textOf(row.descriptionSnippet || row.description).slice(0, 8000),
      url: textOf(row.url) || `https://booth.pm/ja/items/${boothId}`,
      thumbnailUrl: textOf(row.thumbnailUrl || row.imageUrl) || null,
    };
    const existing = store.products.find((item) => item.boothId === boothId);
    if (!existing) {
      store.products.push(
        product({
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
        }),
      );
      inserted += 1;
      continue;
    }
    const same =
      existing.title === incoming.title &&
      existing.shop === incoming.shop &&
      existing.descriptionSnippet === incoming.descriptionSnippet &&
      existing.url === incoming.url &&
      existing.thumbnailUrl === incoming.thumbnailUrl;
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

export async function dispatch<T>(command: string, args: Record<string, unknown>): Promise<T> {
  notePreviewCall(command);
  switch (command) {
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
    case "probe_remote_files":
      store.remoteChecked = true;
      return detail(String(args.id ?? "")) as T;
    case "list_queue":
      return queue(String(args.kind ?? "review")) as T;
    case "list_product_options":
      return activeProducts()
        .map((item) => ({ boothId: item.boothId, title: item.title, shop: item.shop }))
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
    case "update_settings":
    case "save_settings": {
      const settings = command === "update_settings" ? { ...store.settings, ...(args.patch as Partial<Settings>) } : args.settings as Settings;
      if (!/^http:\/\/(127\.0\.0\.1|localhost|\[::1\])(:\d+)?$/i.test(settings.ollamaBaseUrl.replace(/\/$/, ""))) throw new Error("Ollamaは手元のPC（localhost）だけに接続します");
      if (!/^[a-zA-Z0-9._:/-]{1,80}$/.test(settings.ollamaModel.trim())) throw new Error("モデル名が不正です");
      if (settings.routeDownloads && !settings.libraryFolder.trim()) throw new Error("自動整理をオンにする前に、ライブラリのフォルダを選んでください");
      const backgroundMode = Boolean(settings.backgroundMode);
      store.settings = { ...settings, ollamaModel: settings.ollamaModel.trim(), backgroundMode, launchAtLogin: backgroundMode && Boolean(settings.launchAtLogin), confidenceThreshold: Math.min(1, Math.max(0, settings.confidenceThreshold)) };
      return (command === "update_settings" ? { ...store.settings } : undefined) as T;
    }
    case "import_export":
      return importExport(String(args.json ?? "")) as T;
    case "update_product":
      updateProduct(args.update as ProductUpdate);
      return undefined as T;
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
    case "extract_products":
      desktopOnly();
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
    case "open_booth_browser":
    case "show_booth_library":
    case "browse_booth_history":
    case "import_booth_library":
      return (await previewLibraryWalk()) as T;
    case "close_booth_browser":
      return undefined as T;
    case "cancel_booth_library":
      previewLibraryCancel = true;
      return undefined as T;
    case "clear_booth_session":
      previewBoothLoggedIn = false;
      return undefined as T;
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
