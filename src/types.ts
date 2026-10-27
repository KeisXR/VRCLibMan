export type Category = "avatar" | "outfit" | "hair" | "accessory" | "texture" | "gimmick" | "world" | "other";

export const CATEGORY_LABEL: Record<string, string> = {
  avatar: "アバター",
  outfit: "衣装",
  hair: "髪",
  accessory: "アクセサリ",
  texture: "テクスチャ",
  gimmick: "ギミック",
  world: "ワールド",
  other: "その他",
  uncategorized: "未分類",
};

export const CATEGORIES: Category[] = ["avatar", "outfit", "hair", "accessory", "texture", "gimmick", "world", "other"];

export const ASSET_GENRES = ["outfit", "shoes_socks", "hair", "accessory", "head_ears_tail", "makeup_skin", "eye_texture", "expression_animation", "pose", "gimmick", "particle_effect", "shader", "tool_editor", "other"] as const;
export const WORLD_GENRES = ["world", "furniture_props", "world_gimmick", "texture_material", "skybox", "lighting_postprocess", "world_shader", "building_terrain", "audio_bgm", "tool_other"] as const;
export const GENRE_LABEL: Record<string, string> = {
  outfit: "衣装", shoes_socks: "靴・靴下", hair: "髪型", accessory: "アクセサリ", head_ears_tail: "頭・耳・しっぽ", makeup_skin: "メイク・肌テクスチャ", eye_texture: "アイテクスチャ", expression_animation: "表情・アニメーション", pose: "ポーズ", gimmick: "ギミック", particle_effect: "パーティクル・エフェクト", shader: "シェーダー", tool_editor: "ツール・エディタ拡張", other: "その他", world: "ワールド本体", furniture_props: "家具・小物（ベッドなど）", world_gimmick: "ワールドギミック（Udon）", texture_material: "テクスチャ置換・マテリアル", skybox: "Skybox", lighting_postprocess: "ライティング・ポストプロセス", world_shader: "ワールド用シェーダー", building_terrain: "建物・地形パーツ", audio_bgm: "音・BGM", tool_other: "ツール・その他",
};
export function genresForCategory(category: Category | string): readonly string[] { return category === "world" ? WORLD_GENRES : ASSET_GENRES; }

export interface AvatarCard {
  id: string;
  displayName: string;
  baseBody: string | null;
  baseBodyName: string | null;
  aliases: string[];
  isBuiltin: boolean;
  isPlaceholder: boolean;
  owned: boolean;
  thumbnailPath: string | null;
  thumbnailUrl: string | null;
  outfitCount: number;
  hairCount: number;
  gimmickCount: number;
  genericCount: number;
}

export interface FileBrief {
  path: string;
  filename: string;
  isDir: boolean;
  size: number;
  matchScore: number;
}

export type FilePlace = "local" | "remote" | "missing";

export interface VariationView {
  name: string;
  owned: boolean;
}

export interface DownloadView {
  variation: string;
  filename: string;
  sizeBytes: number | null;
  state: FilePlace;
  localPath: string | null;
  remotePath: string | null;
}

export interface ItemView {
  boothId: string;
  title: string;
  shop: string;
  url: string;
  descriptionSnippet: string;
  thumbnailUrl: string | null;
  thumbnailPath: string | null;
  category: Category | null;
  genre: string | null;
  isGeneric: boolean;
  confidence: number | null;
  viaBaseBody: boolean;
  localFiles: FileBrief[];
  localFileCount: number;
  variations: VariationView[];
  downloads: DownloadView[];
  isFree: boolean;
  isAdult: boolean;
  remoteConfigured: boolean;
  remoteChecked: boolean;
  remoteReachable: boolean;
}

export interface OwnedBody {
  boothId: string;
  title: string;
  shop: string;
  url: string;
  thumbnailUrl: string | null;
  thumbnailPath: string | null;
}

export interface Section {
  category: string;
  items: ItemView[];
}

export interface Sibling {
  id: string;
  displayName: string;
}

export interface AvatarDetail {
  id: string;
  displayName: string;
  baseBody: string | null;
  baseBodyName: string | null;
  baseBodyNote: string | null;
  aliases: string[];
  isBuiltin: boolean;
  isPlaceholder: boolean;
  owned: boolean;
  siblings: Sibling[];
  ownedProducts: OwnedBody[];
  sections: Section[];
  genericItems: ItemView[];
  unsupportedOutfits: ItemView[];
}

export interface QueueProduct {
  boothId: string;
  title: string;
  shop: string;
  url: string;
  descriptionSnippet: string;
  thumbnailUrl: string | null;
  thumbnailPath: string | null;
  category: Category | null;
  genre: string | null;
  isGeneric: boolean;
  confidence: number | null;
  compatibleAvatars: string[];
  unresolvedNames: string[];
  linkedAvatarIds: string[];
  userConfirmed: boolean;
  filePresence: "local" | "partial" | "remote" | "missing" | "unknown";
}

export interface QueueFile {
  path: string;
  filename: string;
  isDir: boolean;
  size: number;
  matchScore: number;
  candidateProductId: string | null;
  candidateTitle: string | null;
}

export interface QueuePage {
  unextracted: number;
  needsReview: number;
  unlinkedFiles: number;
  products: QueueProduct[];
  files: QueueFile[];
}

export interface ProductOption {
  boothId: string;
  title: string;
  shop: string;
}

export interface AvatarRecord {
  id: string;
  displayName: string;
  baseBody: string | null;
  aliases: string[];
  isBuiltin: boolean;
  owned: boolean;
  notes: string;
}

export interface BaseBody {
  id: string;
  displayName: string;
  note: string;
}

export interface Knowledge {
  avatars: AvatarRecord[];
  baseBodies: BaseBody[];
}

export interface ExtensionLink {
  token: string;
  port: number;
  listening: boolean;
  error: string | null;
}

export type BoothImportMode = "extension" | "browser" | "both";

export interface ImportReceipt {
  at: string;
  products: number;
  inserted: number;
  updated: number;
  unchanged: number;
  skipped: number;
}

export interface IntegrationStatus {
  mode: BoothImportMode;
  paired: boolean;
  expectedExtensionVersion: string;
  extensionVersion: string | null;
  extensionLastSeenAt: string | null;
  extensionImport: ImportReceipt | null;
  browserImport: ImportReceipt | null;
}

export interface Settings {
  ollamaBaseUrl: string;
  ollamaModel: string;
  confidenceThreshold: number;
  libraryFolder: string;
  builtinBrowser: boolean;
  remoteLibraryFolder: string;
  remoteAutoSync: boolean;
  routeDownloads: boolean;
  llmBackend: "ollama" | "apple";
  backgroundMode: boolean;
  launchAtLogin: boolean;
}

export interface RemoteSyncStatus {
  running: boolean;
  copied: number;
  skipped: number;
  lastCompletedAt: string | null;
  error: string | null;
}

export interface StagedDownload {
  path: string;
  boothId: string;
  filename: string;
  reason: string;
}

export interface LibraryStats {
  products: number;
  ownedAvatars: number;
  unextracted: number;
  needsReview: number;
  queueCount: number;
  unlinkedFiles: number;
  ollamaModel: string;
}

export interface BrowserDiagnostic {
  path: string;
  text: string;
}

export interface BoothAccount {
  loggedIn: boolean;
  displayName: string;
  pixivId: string;
  avatarUrl: string;
}

export interface ImportReport {
  inserted: number;
  updated: number;
  unchanged: number;
  skipped: number;
  variations: number;
  downloads: number;
  unnamedDownloads: number;
  warnings: string[];
}

export interface LibraryWalkReport {
  products: number;
  files: number;
  namedFiles: number;
  unnamedFiles: number;
  pages: number;
  cancelled: boolean;
  notice: string;
  inserted: number;
  updated: number;
  unchanged: number;
}

export interface ScanReport {
  indexed: number;
  linked: number;
  truncated: boolean;
  root: string;
}

export interface ExtractReport {
  processed: number;
  failed: number;
  cancelled: boolean;
  errors: string[];
}

export const DEFAULT_OLLAMA_MODEL = "qwen3.5:9b";

export interface OllamaStatus {
  models: string[];
  modelReady: boolean;
  modelSizeBytes: number | null;
}

export interface CatalogInfo {
  count: number;
  fetchedOn: string;
  upstreamUpdated: string;
  sourceUrl: string;
  termsUrl: string;
  author: string;
  credit: string;
}

export interface CatalogRefresh {
  added: number;
  total: number;
  upstreamUpdated: string;
}

export interface SetupStatus {
  complete: boolean;
  model: string;
  defaultModel: string;
  backend: "ollama" | "apple";
}

export interface AppleFmStatus {
  availability: string;
  variant: string;
  contextSize: number;
  message: string;
}

export interface AvatarCandidate {
  name: string;
  productCount: number;
}

export interface JobProgress {
  job: string;
  current: number;
  total: number;
  message: string;
}

export interface BoothFileStatus {
  boothId: string;
  variation: string;
  filename: string;
  phase: "receiving" | "error" | "done";
  message: string;
}

export interface ManagedProduct {
  boothId: string;
  title: string;
  shop: string;
  category: Category | null;
  excluded: boolean;
}

export interface ProductUpdate {
  boothId: string;
  category?: string | null;
  genre?: string | null;
  isGeneric?: boolean | null;
  avatarIds?: string[] | null;
  confirm?: boolean | null;
}

export interface AvatarInput {
  id?: string | null;
  displayName: string;
  baseBody?: string | null;
  aliases: string[];
  notes: string;
}

export interface BrowserBookmark { title: string; url: string; }
export interface BrowserTab { id: string; url: string; title: string; loading: boolean; canGoBack: boolean; canGoForward: boolean; }
export interface BrowserState { open: boolean; tabs: BrowserTab[]; activeId: string | null; bookmarks: BrowserBookmark[]; busy: boolean; }
export type BrowserAction =
  | { kind: "newTab"; url?: string }
  | { kind: "navigate"; url: string }
  | { kind: "selectTab" | "closeTab"; tabId: string }
  | { kind: "back" | "forward" | "reload" | "stop" }
  | { kind: "toolbarHeight"; height: number };

export interface AppUpdateStatus {
  revision: number;
  phase: "unavailable" | "idle" | "checking" | "current" | "available" | "downloading" | "ready" | "installing";
  version: string | null;
  notes: string | null;
  downloaded: number;
  total: number | null;
  error: string | null;
  message: string | null;
}
