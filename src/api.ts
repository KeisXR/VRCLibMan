import { invoke } from "@tauri-apps/api/core";
import { syncExportFilename } from "./syncExport";
import { readWithTimeout } from "./ipcRecovery";
import { createPanelReporter } from "./panelReporter";
import type {
  AppUpdateStatus,
  AssetUpdateStatus,
  AssetUpdateRecord,
  AssetUpdateConfigure,
  AvatarCard,
  BrowserState,
  BrowserAction,
  ManagedProduct,
  BoothImportMode,
  IntegrationStatus,
  AvatarDetail,
  AvatarInput,
  CatalogInfo,
  CatalogRefresh,
  ExtractReport,
  ExtractTarget,
  ClassificationStatus,
  ClassificationSnapshot,
  RegisterOwnedBodyResult,
  ExtensionLink,
  BrowserDiagnostic,
  BoothAccount,
  ImportReport,
  LibraryWalkReport,
  PurchaseLabel,
  PurchasePage,
  PurchaseQuery,
  PurchaseStatus,
  PurchaseSummary,
  PurchaseWalkReport,
  BoothFileStatus,
  FreeFilesReport,
  JobProgress,
  Knowledge,
  LibraryStats,
  SetupStatus,
  ProductOption,
  ProductUpdate,
  ProductEditView,
  UnlinkedFilesPage,
  ClassificationRunPage,
  ClassificationEntry,
  UnresolvedNameCount,
  ScanReport,
  Settings,
  SyncExportReport,
  RemoteSyncStatus,
  CopyResult,
  StagedDownload,
  DirListing,
  ZipListing,
  ItemView,
  GenreKind,
  GenreProducts,
  UnityStatus,
  UnityPackageList,
  PreparedUnityPackage,
  UnityLaunchResult,
  UnityLaunchMode,
} from "./types";

export type ClassificationFeedbackScope = "other" | "unclassified" | "both";

const CLASSIFICATION_FEEDBACK_FILENAME = "vrclibman-classification-feedback.json";

export async function appVersion(): Promise<string> {
  if (isTauri()) {
    const { getVersion } = await import("@tauri-apps/api/app");
    return getVersion();
  }
  return (await import("../package.json")).version;
}

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export const assetUpdates = {
  status: () => readWithTimeout(() => call<AssetUpdateStatus>("asset_update_status"), "アセット更新の状態"),
  history: () => readWithTimeout(() => call<AssetUpdateRecord[]>("asset_update_history"), "アセット更新の記録"),
  configure: (input: AssetUpdateConfigure) => call<AssetUpdateStatus>("configure_asset_updates", { input }),
  check: () => call<AssetUpdateStatus>("check_asset_updates"),
};

export const appUpdater = {
  status: () => call<AppUpdateStatus>("updater_status"),
  check: (manual = true) => call<AppUpdateStatus>("check_app_update", { manual }),
  download: () => call<AppUpdateStatus>("download_app_update"),
  install: () => call<AppUpdateStatus>("install_app_update"),
};

export function listenUpdaterChanged(onStatus: (status: AppUpdateStatus) => void): Promise<() => void> {
  if (!isTauri()) {
    return import("./preview/api").then((preview) => preview.listenPreviewUpdaterChanged(onStatus));
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen<AppUpdateStatus>("updater-changed", (event) => onStatus(event.payload)),
  );
}

const refreshBus = new EventTarget();

export function requestRefresh(): void {
  refreshBus.dispatchEvent(new Event("refresh"));
}

export function onRefresh(listener: () => void): () => void {
  refreshBus.addEventListener("refresh", listener);
  return () => refreshBus.removeEventListener("refresh", listener);
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    const preview = await import("./preview/api");
    return preview.dispatch<T>(command, args ?? {});
  }
  return invoke<T>(command, args);
}

export const reportBrowserPanel = createPanelReporter((report) =>
  call<BrowserState>("browser_action", { action: { kind: "panelArea", ...report } }),
);

export function listenBoothUsed(onUsed: () => void): Promise<() => void> {
  if (!isTauri()) {
    return Promise.resolve(() => undefined);
  }
  return import("@tauri-apps/api/event").then(({ listen }) => listen("booth-browser-used", () => onUsed()));
}

/** 購入履歴が変わった（読み込みの途中、購入完了の記録、注文詳細の取得）。 */
export function listenPurchasesChanged(onChange: () => void): Promise<() => void> {
  if (!isTauri()) {
    return import("./preview/purchases").then((preview) => preview.listenPreviewPurchases(onChange));
  }
  return import("@tauri-apps/api/event").then(({ listen }) => listen("purchases-changed", () => onChange()));
}

export function listenBoothFile(onStatus: (status: BoothFileStatus) => void): Promise<() => void> {
  if (!isTauri()) {
    return import("./preview/api").then((preview) => preview.listenPreviewBoothFile(onStatus));
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen<BoothFileStatus>("booth-file-download", (event) => onStatus(event.payload)),
  );
}

export function listenProgress(onProgress: (progress: JobProgress) => void): Promise<() => void> {
  if (!isTauri()) {
    return import("./preview/api").then((preview) => preview.listenPreviewProgress(onProgress));
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen<JobProgress>("job-progress", (event) => onProgress(event.payload)),
  );
}

export const api = {
  stats: () => call<LibraryStats>("library_stats"),
  avatars: () => call<AvatarCard[]>("list_avatars"),
  avatar: (id: string) => call<AvatarDetail>("get_avatar", { id }),
  productsByCategory: (category: string) => call<ItemView[]>("list_products_by_category", { category }),
  productsByGenre: (kind: GenreKind, genre?: string | null) => call<GenreProducts>("list_products_by_genre", { kind, genre: genre ?? null }),
  probeRemoteFiles: (id: string) => call<AvatarDetail>("probe_remote_files", { id }),
  probeRemoteProducts: (ids: string[]) => call<import("./types").RemoteProductProbe[]>("probe_remote_products", { ids }),
  productForEdit: (boothId: string) => call<ProductEditView>("product_for_edit", { boothId }),
  unlinkedFiles: (offset = 0, limit = 50) =>
    call<UnlinkedFilesPage>("list_unlinked_files", { offset, limit }),
  classificationRuns: (offset = 0, limit = 50) =>
    call<ClassificationRunPage>("classification_log_runs", { offset, limit }),
  classificationEntries: (runId: number, offset = 0, limit = 100) =>
    call<ClassificationEntry[]>("classification_log_entries", { runId, offset, limit }),
  searchClassificationEntries: (query: string, limit = 200) =>
    call<ClassificationEntry[]>("classification_log_search", { query, limit }),
  unresolvedNames: () => call<UnresolvedNameCount[]>("classification_unresolved_names"),
  undoClassificationEntry: (entryId: number) =>
    call<void>("undo_classification_entry", { entryId }),
  managedProducts: () => call<ManagedProduct[]>("list_managed_products"),
  setProductExcluded: (boothId: string, excluded: boolean) => call<void>("set_product_excluded", { boothId, excluded }),
  productOptions: () => call<ProductOption[]>("list_product_options"),
  knowledge: () => call<Knowledge>("list_knowledge"),
  settings: () => call<Settings>("get_settings"),
  remoteSyncStatus: () => call<RemoteSyncStatus>("remote_sync_status"),
  /** 自動同期のワーカーを今すぐ起こす。2 本目は作らない。 */
  retryRemoteSync: () => call<void>("retry_remote_sync"),
  /** 説明の全文まで含めた検索。一致した商品の BOOTH ID を返す。 */
  searchProductIds: (query: string) => call<string[]>("search_product_ids", { query }),
  integrationStatus: () => call<IntegrationStatus>("integration_status"),
  setBoothImportMode: (mode: BoothImportMode) => call<void>("set_booth_import_mode", { mode }),
  extensionLink: () => call<ExtensionLink>("extension_link"),
  regenerateExtensionToken: () => call<ExtensionLink>("regenerate_extension_token"),
  saveSettings: async (settings: Settings) => {
    await call<void>("save_settings", { settings });
  },
  updateSettings: async (patch: Partial<Settings>) => {
    const saved = await call<Settings>("update_settings", { patch });
    return saved;
  },
  boothPageInterval: () => call<number>("get_booth_page_interval"),
  setBoothPageInterval: (intervalMs: number) => call<void>("set_booth_page_interval", { intervalMs }),
  browserState: () => readWithTimeout(() => call<BrowserState>("browser_state"), "ブラウザの状態"),
  browserPanelSnapshot: () => call<string | null>("browser_panel_snapshot"),
  browserAction: (action: BrowserAction) => call<BrowserState>("browser_action", { action }),
  saveBrowserBookmark: (originalUrl: string | null, title: string, url: string) => call<BrowserState>("save_browser_bookmark", { originalUrl, title, url }),
  deleteBrowserBookmark: (url: string) => call<BrowserState>("delete_browser_bookmark", { url }),
  openBoothBrowser: () => call<void>("open_booth_browser"),
  boothAccount: () => call<BoothAccount>("booth_account"),
  showBoothLibrary: () => call<void>("show_booth_library"),
  browseBoothHistory: (forward: boolean) => call<void>("browse_booth_history", { forward }),
  importBoothPage: () => call<ImportReport>("import_booth_page"),
  importBoothLibrary: () => call<LibraryWalkReport>("import_booth_library"),
  cancelBoothLibrary: () => call<void>("cancel_booth_library"),
  importBoothPurchases: () => call<PurchaseWalkReport>("import_booth_purchases"),
  cancelBoothPurchases: () => call<void>("cancel_booth_purchases"),
  purchaseHistory: (query: PurchaseQuery) =>
    readWithTimeout(() => call<PurchasePage>("purchase_history", { query }), "購入履歴"),
  purchaseStatus: () => readWithTimeout(() => call<PurchaseStatus>("purchase_status"), "購入履歴の状態"),
  purchaseSummary: (year: number | null) =>
    readWithTimeout(() => call<PurchaseSummary>("purchase_summary", { year }), "購入履歴の集計"),
  setPurchaseShowPrices: (show: boolean) => call<void>("set_purchase_show_prices", { show }),
  createPurchaseLabel: (name: string) => call<PurchaseLabel>("create_purchase_label", { name }),
  renamePurchaseLabel: (id: number, name: string) => call<void>("rename_purchase_label", { id, name }),
  deletePurchaseLabel: (id: number) => call<void>("delete_purchase_label", { id }),
  setPurchaseItemLabels: (itemId: number, labelIds: number[]) =>
    call<void>("set_purchase_item_labels", { itemId, labelIds }),
  setPurchaseGift: (orderId: string, gift: boolean | null) => call<void>("set_purchase_gift", { orderId, gift }),
  clearBoothSession: () => call<void>("clear_booth_session"),
  closeBoothBrowser: () => call<void>("close_booth_browser"),
  boothBrowserLog: () => call<BrowserDiagnostic>("booth_browser_log"),
  importExport: (json: string) => call<ImportReport>("import_export", { json }),
  updateProduct: (update: ProductUpdate) => call<void>("update_product", { update }),
  browserAutoPending: () => call<{ boothId: string; title: string }[]>("browser_auto_pending"),
  browserAutoFinish: (boothId: string) => call<void>("browser_auto_finish", { boothId }),
  productAvatarSuggestions: (boothId: string) => call<{ avatarIds: string[]; unresolvedNames: string[] }>("product_avatar_suggestions", { boothId }),
  moveProductGenre: (boothId: string, kind: GenreKind, genre: string, avatarIds?: string[]) =>
    call<ClassificationSnapshot>("move_product_genre", { boothId, kind, genre, avatarIds: avatarIds ?? null }),
  restoreProductClassification: (snapshot: ClassificationSnapshot) =>
    call<void>("restore_product_classification", { snapshot }),
  unpinProductClassification: (boothId: string) =>
    call<ClassificationSnapshot>("unpin_product_classification", { boothId }),
  registerOwnedBody: (boothId: string, name?: string | null) =>
    call<RegisterOwnedBodyResult>("register_owned_body", { boothId, name: name ?? null }),
  attachProductToAvatar: (avatarId: string, boothId: string) =>
    call<void>("attach_product_to_avatar", { avatarId, boothId }),
  linkFile: (path: string, productId: string | null) =>
    call<void>("link_file", { path, productId }),
  saveAvatar: (avatar: AvatarInput) => call<string>("save_avatar", { avatar }),
  deleteAvatar: (id: string) => call<void>("delete_avatar", { id }),
  setAvatarHeroImage: (avatarId: string, sourcePath: string) =>
    call<string>("set_avatar_hero_image", { avatarId, sourcePath }),
  clearAvatarHeroImage: (avatarId: string) => call<void>("clear_avatar_hero_image", { avatarId }),
  saveBaseBody: (body: { id?: string | null; displayName: string; note: string }) =>
    call<string>("save_base_body", { body }),
  deleteBaseBody: (id: string) => call<void>("delete_base_body", { id }),
  mergeAvatars: (fromId: string, intoId: string) =>
    call<void>("merge_avatars", { fromId, intoId }),
  restoreSeed: () => call<number>("restore_seed"),
  reresolve: () => call<number>("reresolve"),
  previewExistingImport: (path: string) => call<ExistingImportPreview>("preview_existing_import", { path }),
  registerExistingImport: (path: string, selections: ExistingImportSelection[], copy: boolean) => call<{ registered: number; errors: string[] }>("register_existing_import", { path, selections, copy }),
  scanLibrary: (path: string) => call<ScanReport>("scan_library", { path }),
  classificationStatus: () => call<ClassificationStatus>("classification_status"),
  extractProducts: (target: ExtractTarget) => call<ExtractReport>("extract_products", { target }),
  cancelExtract: () => call<void>("cancel_extract"),
  unityEditors: () => call<UnityStatus>("unity_editors"),
  /** 空文字で未設定に戻す。Unity 本体でないファイルは Rust 側で拒否する。 */
  setUnityEditor: (path: string) => call<UnityStatus>("set_unity_editor", { path }),
  catalogInfo: () => call<CatalogInfo>("catalog_info"),
  refreshAvatarCatalog: () => call<CatalogRefresh>("refresh_avatar_catalog"),
  setupStatus: () => call<SetupStatus>("setup_status"),
  completeSetup: () => call<void>("complete_setup"),
  reopenSetup: () => call<void>("reopen_setup"),
  markOwnedAvatar: (boothId: string) => call<string>("mark_owned_avatar", { boothId }),
  addFreeProduct: (url: string) => call<string>("add_free_product", { url }),
  pullRemoteFile: (boothId: string, variation: string, filename: string, overwrite = false) =>
    call<CopyResult>("pull_remote_file", { boothId, variation, filename, overwrite }),
  downloadBoothFile: (boothId: string, variation: string, filename: string) =>
    call<string>("download_booth_file", { boothId, variation, filename }),
  fetchFreeFiles: (boothId: string) => call<FreeFilesReport>("fetch_free_files", { boothId }),
  pushRemoteFile: (boothId: string, variation: string, filename: string, overwrite = false) =>
    call<CopyResult>("push_remote_file", { boothId, variation, filename, overwrite }),
  cacheThumbnails: () => call<number>("cache_thumbnails"),
  cacheProductPreview: (boothId: string, download: boolean, reveal: boolean) =>
    call<string | null>("cache_product_preview", { boothId, download, reveal }),
  stagedDownloads: () => call<StagedDownload[]>("list_staged_downloads"),
  /** 右パネルのファイルタブ。空のパスはライブラリフォルダ。読み取りのみ。 */
  listDirectory: (path: string) => call<DirListing>("list_directory", { path }),
  /** zip の中身を一覧だけ読む。展開しない。 */
  listZipEntries: (path: string) => call<ZipListing>("list_zip_entries", { path }),
  /** zip か .unitypackage の中の Unity パッケージを一覧する。展開しない。ライブラリ内のファイルだけ。 */
  unityPackages: (path: string) => call<UnityPackageList>("unity_packages", { path }),
  /** 選んだものだけをアプリのキャッシュに取り出す。単体の .unitypackage はそのまま返す。 */
  prepareUnityPackages: (path: string, entries: string[], boothId: string | null) =>
    call<PreparedUnityPackage[]>("prepare_unity_packages", { path, entries, boothId }),
  /** 先頭から順に Unity へ渡す。Unity の起動を待つときは途中で止まる。 */
  launchUnityPackages: (paths: string[], mode: UnityLaunchMode) =>
    call<UnityLaunchResult>("launch_unity_packages", { paths, mode }),
  routeStagedDownload: (path: string) => call<void>("route_staged_download", { path }),
  classificationFeedback: (scope: ClassificationFeedbackScope) =>
    call<string>("classification_feedback", { scope }),
  saveClassificationFeedback: (scope: ClassificationFeedbackScope, path: string) =>
    call<void>("save_classification_feedback", { scope, path }),
  syncExportJson: () => call<string>("sync_export_json"),
  saveSyncExport: (path: string) => call<SyncExportReport>("save_sync_export", { path }),
};

export async function copyClassificationFeedback(scope: ClassificationFeedbackScope): Promise<void> {
  const json = await api.classificationFeedback(scope);
  if (!navigator.clipboard?.writeText) {
    throw new Error("クリップボードにコピーできません");
  }
  await navigator.clipboard.writeText(json);
}

export async function saveClassificationFeedback(
  scope: ClassificationFeedbackScope,
): Promise<"saved" | "cancelled"> {
  if (!isTauri()) {
    downloadJson(CLASSIFICATION_FEEDBACK_FILENAME, await api.classificationFeedback(scope));
    return "saved";
  }
  const { save } = await import("@tauri-apps/plugin-dialog");
  const selected = await save({
    title: "分類改善用データを保存",
    defaultPath: CLASSIFICATION_FEEDBACK_FILENAME,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (typeof selected !== "string") return "cancelled";
  await api.saveClassificationFeedback(scope, selected);
  return "saved";
}

export async function exportSyncData(deviceName: string): Promise<SyncExportReport | "cancelled"> {
  const filename = syncExportFilename(deviceName, new Date());
  if (!isTauri()) {
    // 見本は ZIP を作らず、形式の確認用に manifest だけを落とす。
    const json = await api.syncExportJson();
    downloadJson(filename.replace(/\.zip$/, "-manifest.json"), json);
    return { counts: (JSON.parse(json) as SyncExportReport).counts };
  }
  const { save } = await import("@tauri-apps/plugin-dialog");
  const selected = await save({
    title: "データ移行のファイルをエクスポート",
    defaultPath: filename,
    filters: [{ name: "ZIP", extensions: ["zip"] }],
  });
  if (typeof selected !== "string") return "cancelled";
  return api.saveSyncExport(selected);
}

function downloadJson(filename: string, json: string) {
  const blob = new Blob([json], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

export async function saveLibraryAndScan(folder: string): Promise<ScanReport> {
  const saved = await api.updateSettings({ libraryFolder: folder });
  return api.scanLibrary(saved.libraryFolder);
}

export async function revealInFolder(path: string): Promise<void> {
  if (!isTauri()) return;
  const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
  await revealItemInDir(path);
}

export async function chooseFolder(): Promise<string | null> {
  if (!isTauri()) {
    throw new Error("フォルダ選択はデスクトップアプリで使えます");
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    directory: true,
    multiple: false,
    title: "アセットのフォルダを選ぶ",
  });
  return typeof selected === "string" ? selected : null;
}

/** Unity 本体を選ぶ。Windows は Unity.exe、Mac は Unity.app、Linux は Unity。 */
export async function chooseUnityEditor(): Promise<string | null> {
  if (!isTauri()) {
    throw new Error("Unity の選択はデスクトップアプリで使えます");
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const windows = navigator.userAgent.includes("Windows");
  const selected = await open({
    directory: false,
    multiple: false,
    title: "Unity 本体を選ぶ",
    ...(windows ? { filters: [{ name: "Unity", extensions: ["exe"] }] } : {}),
  });
  return typeof selected === "string" ? selected : null;
}

/** アバターのヒーロー画像を選ぶ。読み込みと検証は Rust 側で行う。 */
export async function chooseHeroImage(): Promise<string | null> {
  if (!isTauri()) {
    throw new Error("画像の選択はデスクトップアプリで使えます");
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    directory: false,
    multiple: false,
    title: "ヒーロー画像を選ぶ",
    filters: [{ name: "画像", extensions: ["png", "jpg", "jpeg", "webp"] }],
  });
  return typeof selected === "string" ? selected : null;
}

export async function openExternal(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}

export interface BoothLinkDeps {
  settings: () => Promise<Pick<Settings, "builtinBrowser" | "boothLinkTarget">>;
  openBuiltin: (url: string) => Promise<unknown>;
  openExternal: (url: string) => Promise<void>;
}

/** Product links only. Other URLs stay on openExternal. */
export async function openBoothLinkWith(url: string, deps: BoothLinkDeps): Promise<"builtin" | "external"> {
  let useBuiltin = false;
  try {
    const settings = await deps.settings();
    useBuiltin = settings.builtinBrowser && settings.boothLinkTarget === "builtin";
  } catch {
    useBuiltin = false;
  }
  if (useBuiltin) {
    try {
      await deps.openBuiltin(url);
      return "builtin";
    } catch {
      // Busy, disabled, or any other failure opens the system browser.
    }
  }
  await deps.openExternal(url);
  return "external";
}

export async function openBoothLink(url: string): Promise<void> {
  await openBoothLinkWith(url, {
    settings: () => api.settings(),
    openBuiltin: (target) => api.browserAction({ kind: "newTab", url: target }),
    openExternal,
  });
}

/** スキャン済み・記録済みのファイルだけを Rust 側で確かめてから開く。 */
export async function openLocal(path: string): Promise<void> {
  await call<void>("open_local_path", { path });
}

export function listenBrowserChanged(onChanged: () => void): Promise<() => void> {
  if (!isTauri()) return import("./preview/browser").then(({ onPreviewBrowserChanged }) => onPreviewBrowserChanged(onChanged));
  return import("@tauri-apps/api/event").then(({ listen }) => listen("browser-state-changed", onChanged));
}

/** The app moved the browser into the right panel (or opened it there) and wants the tab shown. */
export function listenBrowserPanelRequested(onRequested: (tabId?: string) => void): Promise<() => void> {
  if (!isTauri()) return import("./preview/browser").then(({ onPreviewBrowserPanelRequested }) => onPreviewBrowserPanelRequested(onRequested));
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen<string>("browser-panel-requested", (event) => onRequested(event.payload)),
  );
}

export async function listenBrowserControlEvents(onFocus: () => void, onError: (message: string) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  const [focus, error] = await Promise.all([listen("browser-focus-address", onFocus), listen<string>("browser-operation-error", (event) => onError(event.payload))]);
  return () => { focus(); error(); };
}

export interface ExistingImportChoice { boothId: string; title: string; variation: string; filename: string; sizeBytes: number | null; registered: boolean }
export interface ExistingImportEntry { path: string; filename: string; size: number; status: "matched" | "review" | "unmatched" | "registered"; choices: ExistingImportChoice[] }
export interface ExistingImportPreview { entries: ExistingImportEntry[]; truncated: boolean }
export interface ExistingImportSelection { path: string; boothId: string; variation: string; filename: string; size: number }
