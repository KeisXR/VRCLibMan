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
  outfit: "衣装", shoes_socks: "靴・靴下", hair: "髪型", accessory: "アクセサリ", head_ears_tail: "頭・耳・しっぽ", makeup_skin: "メイク・肌テクスチャ", eye_texture: "アイテクスチャ", expression_animation: "表情・アニメーション", pose: "ポーズ", gimmick: "ギミック", particle_effect: "パーティクル・エフェクト", shader: "シェーダー", tool_editor: "ツール・エディタ拡張", other: "その他", world: "ワールド本体", furniture_props: "家具・小物（ベッドなど）", world_gimmick: "ワールドギミック（Udon）", texture_material: "テクスチャ置換・マテリアル", skybox: "Skybox", lighting_postprocess: "ライティング・ポストプロセス", world_shader: "ワールド用シェーダー", building_terrain: "建物・地形パーツ", audio_bgm: "音・BGM", tool_other: "ツール・その他", unset: "ジャンル未設定", free: "無料",
};
/** Route / query key for products whose genre is missing or belongs to the other kind. Same as genre::UNSET in Rust. */
export const GENRE_UNSET = "unset";
/** Browse key for free products that are not on disk. Same as genre::FREE in Rust. Not a stored genre. */
export const GENRE_FREE = "free";
export function genresForCategory(category: Category | string): readonly string[] { return category === "world" ? WORLD_GENRES : ASSET_GENRES; }
export type GenreKind = "asset" | "world";
export function genresForKind(kind: GenreKind): readonly string[] { return kind === "world" ? WORLD_GENRES : ASSET_GENRES; }

export interface GenreProducts {
  items: ItemView[];
  counts: Record<string, number>;
  total: number;
}

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
  downloadableId?: string | null;
  isNew?: boolean;
  superseded?: boolean;
}

export interface RemoteProductProbe {
  boothId: string;
  remoteChecked: boolean;
  remoteReachable: boolean;
  downloads: DownloadView[];
}

export interface ItemView {
  purchasedEditions: PurchasedEditions;
  boothId: string;
  title: string;
  shop: string;
  url: string;
  descriptionSnippet: string;
  thumbnailUrl: string | null;
  thumbnailPath: string | null;
  category: Category | null;
  genre: string | null;
  /** Both category and genre were locked by a manual placement. */
  classificationPinned: boolean;
  isGeneric: boolean;
  confidence: number | null;
  viaBaseBody: boolean;
  localFiles: FileBrief[];
  localFileCount: number;
  variations: VariationView[];
  downloads: DownloadView[];
  isFree: boolean;
  isAdult: boolean;
  /** Library badge「非公開」. Distinct from 公開終了. */
  unpublished: boolean;
  remoteConfigured: boolean;
  remoteChecked: boolean;
  remoteReachable: boolean;
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
  thumbnailUrl: string | null;
  thumbnailPath: string | null;
  /** カスタムのヒーロー画像だけ。未指定は null で、サムネイルへフォールバックする。 */
  heroImagePath: string | null;
  /** 同じパスのまま差し替えても画面が気付けるよう、ファイルの更新時刻を持つ。 */
  heroImageVersion: number | null;
  siblings: Sibling[];
  ownedProducts: ItemView[];
  sections: Section[];
  genericItems: ItemView[];
  unsupportedOutfits: ItemView[];
  unsetEditionItems: ItemView[];
}

export interface PurchasedEditions { mode: "auto" | "specific" | "all" | "unknown"; avatarIds: string[]; }

export interface ProductEditView {
  purchasedEditions: PurchasedEditions;
  boothId: string;
  title: string;
  shop: string;
  url: string;
  thumbnailUrl: string | null;
  thumbnailPath: string | null;
  category: Category | null;
  genre: string | null;
  /** ジャンルを手で固定している。false のときは「自動」で決まったジャンル。 */
  genreLocked: boolean;
  isGeneric: boolean;
  linkedAvatarIds: string[];
}

export interface UnlinkedFile {
  path: string;
  filename: string;
  isDir: boolean;
  size: number;
  matchScore: number;
  candidateProductId: string | null;
  candidateTitle: string | null;
}

export interface UnlinkedFilesPage {
  total: number;
  files: UnlinkedFile[];
}

export interface ClassificationRunView {
  id: number;
  startedAt: string;
  kind: string;
  classifier: string;
  note: string | null;
  changedCount: number;
}

export interface ClassificationRunPage {
  total: number;
  runs: ClassificationRunView[];
}

export interface ClassificationLink {
  id: string;
  name: string;
}

export interface ClassificationEntry {
  id: number;
  runId: number;
  startedAt: string;
  kind: string;
  classifier: string;
  boothId: string;
  title: string | null;
  beforeCategory: string | null;
  afterCategory: string | null;
  beforeLinks: ClassificationLink[];
  afterLinks: ClassificationLink[];
  confidence: number | null;
  unresolvedNames: string[];
  undone: boolean;
  /** 元に戻せない理由。null なら戻せる。 */
  undoBlocked: string | null;
}

export interface UnresolvedNameCount {
  name: string;
  productCount: number;
}

export interface ProductOption {
  boothId: string;
  title: string;
  shop: string;
  category: Category | null;
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

export type ExtractTarget = "unclassified" | "stale" | "all";

export interface ClassificationStatus {
  classifierVersion: number;
  staleCount: number;
}

export interface Settings {
  confidenceThreshold: number;
  libraryFolder: string;
  builtinBrowser: boolean;
  /** 内蔵ブラウザの強制ダークモード。背景と文字だけを暗くし、画像・動画・キャンバスは元の色のまま。既定はオフ。 */
  browserForceDark: boolean;
  browserAutoRegister: boolean;
  boothLinkTarget: "external" | "builtin";
  remoteLibraryFolder: string;
  remoteAutoSync: boolean;
  routeDownloads: boolean;
  /** 実行環境が常駐とログイン起動を利用できるか。設定では変更しない。 */
  backgroundSupported: boolean;
  backgroundMode: boolean;
  launchAtLogin: boolean;
  classificationDismissedVersion: number;
  uiRailWidth: number;
  /** 左サイドバーをアイコンだけの細い列にする。幅 uiRailWidth は開いたときの値として残す。 */
  uiRailCollapsed: boolean;
  uiGenreWidth: number;
  /** アセットとワールドの大きい画像欄。列幅と同じく設定に残す。 */
  previewPane: boolean;
  previewPaneWidth: number;
  /** アバター詳細のヒーロー。large | small | off（旧デザイン）。既定は small。 */
  avatarHero: "large" | "small" | "off";
  /** 右パネルのタブの種類と並び、選択中の位置（JSON）。URL は含めない。開閉は previewPane、幅は previewPaneWidth を引き継ぐ。 */
  rightPanelTabs: string;
  checkUpdateOnStartup: boolean;
  uiUpdateDismissedVersion: string;
  /** 初回起動で作る端末 ID。変更しない。 */
  deviceId: string;
  /** データ移行のファイルに記録する端末名。既定は OS のホスト名。 */
  deviceName: string;
}

export interface SyncCounts {
  products: number;
  links: number;
  avatars: number;
  fileLinks: number;
  config: number;
  /** パッケージに実際に入れた画像の数。 */
  thumbnails: number;
  heroImages: number;
  /** 購入履歴の注文と商品の数。 */
  purchaseOrders: number;
  purchaseItems: number;
}

export interface SyncExportReport {
  counts: SyncCounts;
}

export interface RemoteSyncStatus {
  running: boolean;
  copied: number;
  /** リモートの同じ場所に既にあったファイル（記録済みで残っている、または同じ名前・同じ大きさ）。 */
  skipped: number;
  /** コピー済みと記録したが、リモートの元の場所に無いファイル。自動ではコピーし直さない。 */
  missing: number;
  /** missing の最初の 20 件（商品ID/ファイル名）。 */
  missingFiles: string[];
  /** 同じ名前で大きさの違うファイルがリモートにあり、コピーしなかったもの。記録しないので次の確認でまた見る。 */
  conflicts: number;
  conflictFiles: { file: string; localSize: number; remoteSize: number }[];
  /** 登録した手元のファイルが見つからず、コピーできなかったもの。 */
  sourceMissing: number;
  sourceMissingFiles: string[];
  /** コピーに失敗したもの。1 件ずつの理由を持つ。 */
  failed: number;
  failures: { file: string; error: string }[];
  lastCompletedAt: string | null;
  /** 確認全体の問題（リモートのフォルダに届かないなど）。 */
  error: string | null;
}

/** 「リモートへコピー」「ローカルへコピー」の結果。conflict は何も書かず、上書きは確認してから。 */
export interface CopyResult {
  status: "copied" | "already" | "conflict";
  sourceSize: number | null;
  existingSize: number | null;
}

/** 右パネル「ファイル」タブ。読み取り専用の一覧で、書き込み・削除・移動はしない。 */
export type FileEntryKind = "dir" | "file" | "link";

export interface FileEntry {
  name: string;
  kind: FileEntryKind;
  /** バイト数。フォルダとリンクは null。 */
  size: number | null;
  /** 表示だけの補足。`名前（商品名）` のように添える。ディスク上の名前は変えない。 */
  label?: FileLabel;
}

/** ローカル DB の商品名。外部へ送らない。 */
export interface FileLabel {
  title: string;
  /** BOOTH の年齢制限。ライブラリ一覧と同じ「R-18」バッジを出す。 */
  adult: boolean;
}

export interface DirListing {
  /** 解決済みのフォルダ（Windows の \\?\ は付けない）。 */
  path: string;
  parent: string | null;
  /** 設定のライブラリフォルダ。存在するときだけ。 */
  library: string | null;
  /** 設定のリモートライブラリ。応答したときだけ。 */
  remote: string | null;
  insideLibrary: boolean;
  entries: FileEntry[];
  truncated: boolean;
  /** 開いているフォルダ自体が商品フォルダのときの補足。 */
  label?: FileLabel;
}

export interface ZipEntry {
  /** zip 内の / 区切りのパス。展開はしない。 */
  path: string;
  dir: boolean;
  size: number;
}

export interface ZipListing {
  path: string;
  library: string | null;
  remote: string | null;
  entries: ZipEntry[];
  truncated: boolean;
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
  unlinkedFiles: number;
  /** 商品に結び付いた手元のファイル（スキャンして結び付いたもの、ダウンロード先を記録したもの）。 */
  registeredFiles: number;
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

export type PurchaseGroupKind = "avatar" | "generic" | "world" | "unclassified" | "multiple" | "unset";

/** アバター別の枠。`key` は `avatar:<id>`、`generic`、`unclassified`。 */
export interface PurchaseGroup {
  key: string;
  kind: PurchaseGroupKind;
  label: string;
  avatarId: string | null;
  /** アバター本体の購入。 */
  body: boolean;
}

/** アプリの中で購入した商品を開く先。ライブラリにあり、除外しておらず、分類が決まっている商品だけ。 */
export interface PurchaseLibraryLink {
  kind: "asset" | "world" | "avatar";
  avatarId: string | null;
}

export interface PurchaseItemView {
  id: number;
  boothId: string;
  name: string;
  variation: string | null;
  shop: string;
  priceYen: number | null;
  boostYen: number | null;
  amountYen: number | null;
  quantity: number;
  inLibrary: boolean;
  libraryLink: PurchaseLibraryLink | null;
  thumbnailPath: string | null;
  thumbnailUrl: string | null;
  groups: PurchaseGroup[];
  labelIds: number[];
}

export interface PurchaseOrderView {
  orderId: string;
  /** BOOTH の表示どおりの日本時間（`YYYY-MM-DDTHH:MM:SS+09:00`）。詳細を読むまで null のことがある。 */
  orderedAt: string | null;
  recordedAt: string;
  totalYen: number | null;
  gift: boolean;
  giftDetected: boolean;
  giftOverride: boolean | null;
  detailRead: boolean;
  source: string;
  title: string;
  items: PurchaseItemView[];
}

export interface PurchaseLabel {
  id: number;
  name: string;
  items: number;
}

export interface PurchaseGroupOption {
  key: string;
  kind: PurchaseGroupKind;
  label: string;
  items: number;
}

export interface PurchaseStatus {
  orders: number;
  items: number;
  pendingDetails: number;
  complete: boolean;
  lastReadAt: string | null;
  showPrices: boolean;
  running: boolean;
}

export interface PurchasePage {
  orders: PurchaseOrderView[];
  total: number;
  offset: number;
  limit: number;
  labels: PurchaseLabel[];
  groups: PurchaseGroupOption[];
  years: number[];
  status: PurchaseStatus;
}

export interface PurchaseQuery {
  query?: string;
  labelId?: number | null;
  group?: string | null;
  gift?: boolean | null;
  year?: number | null;
  offset?: number;
  limit?: number;
}

export interface PurchaseSummaryGroup {
  key: string;
  kind: PurchaseGroupKind;
  label: string;
  avatarId: string | null;
  owned: boolean;
  items: number;
  bodyItems: number;
  amountYen: number;
  unknownAmountItems: number;
}

export interface PurchaseSummary {
  year: number | null;
  years: number[];
  orders: number;
  items: number;
  totalYen: number;
  unknownAmountItems: number;
  giftItems: number;
  giftYen: number;
  groups: PurchaseSummaryGroup[];
  showPrices: boolean;
}

export interface PurchaseWalkReport {
  pages: number;
  added: number;
  removed: number;
  details: number;
  cancelled: boolean;
  notice: string;
}

export interface ScanReport {
  indexed: number;
  linked: number;
  truncated: boolean;
  root: string;
}

export interface ExtractReport {
  processed: number;
  cancelled: boolean;
  /** BOOTH のカテゴリとタグで決まった件数。 */
  boothSettled: number;
  /** カテゴリとタグで決まらず、タイトルと説明の規則で決めた件数。 */
  ruleFilled: number;
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
}

/** 進捗の状態。省略時は running（古い送り方との互換。`src/progress.ts` の `progressState`）。 */
export type JobProgressState = "running" | "done" | "error" | "cancelled";

/** `job-progress` の中身。決まりは docs/design/README.md#進捗。 */
export interface JobProgress {
  job: string;
  current: number;
  /** 0 は終わりが分からない（不定）。 */
  total: number;
  /** 件数（current / total）を必ず書く。対象の名前は付けてよい。検索語や URL は書かない。 */
  message: string;
  state?: JobProgressState;
  /** true の仕事は上部の帯に出さず、その設定の欄だけで見せる。 */
  background?: boolean;
}

/** 「ファイルを取得」の結果。保存後の商品のファイル行の数。 */
export interface FreeFilesReport {
  files: number;
}

export interface BoothFileStatus {
  boothId: string;
  variation: string;
  filename: string;
  phase: "queued" | "receiving" | "moving" | "error" | "done";
  message: string;
  /** 完了時だけ、ライブラリへ移した先の絶対パス。 */
  path?: string | null;
  /** 受け取り中だけ、書いたバイト数。 */
  received?: number | null;
  /** 受け取り中で、大きさが分かるときだけ、全体のバイト数。 */
  total?: number | null;
}

export interface ManagedProduct {
  boothId: string;
  title: string;
  shop: string;
  category: Category | null;
  excluded: boolean;
}

export interface SnapshotLink {
  avatarId: string;
  source: string;
}

export interface OwnedAvatarRestore {
  id: string;
  displayName: string;
  aliases: string[];
  owned: boolean;
  removeIfUnused: boolean;
}

export interface ClassificationSnapshot {
  boothId: string;
  category: string | null;
  genre: string | null;
  categoryLocked: boolean;
  genreLocked: boolean;
  userConfirmed: boolean;
  needsReview: boolean;
  linksLocked?: boolean | null;
  links?: SnapshotLink[] | null;
  ownedAvatars?: OwnedAvatarRestore[];
  compatibleAvatars?: string[];
  unresolvedNames?: string[];
  isGeneric?: boolean;
}

export type RegisterOwnedBodyResult =
  | {
      kind: "registered";
      snapshot: ClassificationSnapshot;
      avatarId: string;
      displayName: string;
    }
  | {
      kind: "needsName";
      suggestedName: string;
      candidates: string[];
    };

export interface ProductUpdate {
  purchasedEditions?: PurchasedEditions;
  boothId: string;
  category?: string | null;
  genre?: string | null;
  /** 「自動」に戻す。ジャンルの固定を外し、カテゴリ・タグ・文面から選び直す。genre と同時には送らない。 */
  resetGenre?: boolean;
  isGeneric?: boolean | null;
  avatarIds?: string[] | null;
}

export interface AvatarInput {
  id?: string | null;
  displayName: string;
  baseBody?: string | null;
  aliases: string[];
  notes: string;
}

export interface BrowserBookmark { title: string; url: string; }
export interface BrowserTab {
  id: string;
  url: string;
  title: string;
  loading: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  zoomFactor: number;
  /** Which panel or window currently shows this page. Omitted only by older callers. */
  hostId?: string;
}
/** Where the browser pages are shown: their own window, the main window's right panel, or nowhere. */
export type BrowserHostKind = "window" | "panel";
/** One open panel or window and the pages that belong to it, in strip order. */
export interface BrowserHostState {
  id: string;
  kind: BrowserHostKind;
  tabIds: string[];
  activeId: string | null;
}
export interface BrowserState {
  open: boolean;
  tabs: BrowserTab[];
  /** Active page of the focused host. Each host also has `hosts[].activeId`. */
  activeId: string | null;
  hosts: BrowserHostState[];
  bookmarks: BrowserBookmark[];
  busy: boolean;
  /** "panel" when the right panel has pages, otherwise "window" when a separate window does. */
  host: BrowserHostKind | null;
  /** The right panel can host the pages on this OS (Windows). Elsewhere the separate window is used. */
  panelSupported: boolean;
}
/** A rectangle in the main window, in CSS pixels. */
export interface BrowserPanelRect { left: number; top: number; width: number; height: number; }
export type BrowserAction =
  | { kind: "newTab"; url?: string; hostId?: string }
  | { kind: "navigate"; url: string; tabId?: string; hostId?: string }
  | { kind: "selectTab" | "closeTab"; tabId: string }
  | { kind: "back" | "forward" | "reload" | "stop"; tabId?: string; hostId?: string }
  | { kind: "zoom"; action: "in" | "out" | "reset"; tabId?: string; hostId?: string }
  | { kind: "toolbarHeight"; height: number; hostId?: string }
  | { kind: "dock"; area: BrowserPanelRect | null; hostId?: string }
  | { kind: "undock" }
  | { kind: "panelArea"; area: BrowserPanelRect | null; visible: boolean }
  | { kind: "reorderTabs"; hostId: string; tabIds: string[] }
  | { kind: "detachTab"; tabId: string; x: number; y: number }
  | { kind: "dockTab"; tabId: string; index?: number };

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

/** "hub"：Unity Hub の記録、"folder"：インストール先のフォルダ、"manual"：「参照…」で指定 */
export type UnityEditorSource = "hub" | "folder" | "manual";

export interface UnityEditor {
  /** 例：2022.3.22f1。パスから読めないときは空文字 */
  version: string;
  path: string;
  source: UnityEditorSource;
}

export interface UnityStatus {
  editors: UnityEditor[];
  /** 保存している既定の Unity。未設定なら空文字 */
  selectedPath: string;
  /** 保存したパスに Unity 本体が見つからない */
  selectedMissing: boolean;
  hubFound: boolean;
  searched: string[];
}

/** 「Unityで開く」: zip か .unitypackage の中の Unity パッケージ。展開せずに読んだ一覧。 */
export interface UnityPackageEntry {
  /** zip 内のパス（`/` 区切り）。単体の .unitypackage では空文字 */
  entry: string;
  name: string;
  size: number;
}

export interface UnityPackageList {
  path: string;
  kind: "zip" | "package";
  packages: UnityPackageEntry[];
  truncated: boolean;
}

/** Unity に渡すファイル。zip 内のものはアプリのキャッシュ（unity-import/<商品ID>/）に取り出したもの */
export interface PreparedUnityPackage {
  name: string;
  path: string;
  size: number;
}

export interface UnityLaunchResult {
  /** 先頭から渡し終えた件数 */
  launched: number;
  /** Unity の起動を待つため途中で止めた */
  waiting: boolean;
}

/** "unity" は設定の既定 Unity で -openfile、"system" は OS の関連付け */
export type UnityLaunchMode = "unity" | "system";

export interface AssetUpdateRow {
  revision: number;
  candidateKey?: string | null;
  boothId: string;
  title: string;
  enabled: boolean;
  checkedAt: string | null;
  error: string | null;
  changes: string[];
}
export interface AssetUpdateStatus {
  automatic: boolean;
  allProducts: boolean;
  totalProducts: number;
  running: boolean;
  progress?: { done: number; total: number } | null;
  rows: AssetUpdateRow[];
}
export interface AssetUpdateFile { downloadableId: string | null; variation: string; filename: string }
export interface AssetUpdateRecord {
  boothId: string;
  detectedAt: string;
  source: "library" | "public";
  descriptionChanged: boolean;
  removed: AssetUpdateFile[];
  added: AssetUpdateFile[];
}
export interface AssetUpdateConfigure {
  allProducts?: boolean;
  automatic?: boolean;
  boothId?: string;
  enabled?: boolean;
  acknowledge?: boolean;
}
