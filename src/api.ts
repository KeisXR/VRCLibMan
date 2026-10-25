import { invoke } from "@tauri-apps/api/core";
import type {
  AppUpdateStatus,
  AvatarCard,
  BrowserState,
  BrowserAction,
  ManagedProduct,
  BoothImportMode,
  IntegrationStatus,
  AvatarDetail,
  AvatarInput,
  AvatarCandidate,
  CatalogInfo,
  CatalogRefresh,
  ExtractReport,
  ExtensionLink,
  BrowserDiagnostic,
  BoothAccount,
  ImportReport,
  LibraryWalkReport,
  BoothFileStatus,
  JobProgress,
  Knowledge,
  LibraryStats,
  OllamaStatus,
  AppleFmStatus,
  SetupStatus,
  ProductOption,
  ProductUpdate,
  QueuePage,
  ScanReport,
  Settings,
  StagedDownload,
  ItemView,
} from "./types";

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

export const appUpdater = {
  status: () => call<AppUpdateStatus>("updater_status"),
  check: () => call<AppUpdateStatus>("check_app_update"),
  download: () => call<AppUpdateStatus>("download_app_update"),
  install: () => call<AppUpdateStatus>("install_app_update"),
};

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

export function listenBoothUsed(onUsed: () => void): Promise<() => void> {
  if (!isTauri()) {
    return Promise.resolve(() => undefined);
  }
  return import("@tauri-apps/api/event").then(({ listen }) => listen("booth-browser-used", () => onUsed()));
}

export function listenBoothFile(onStatus: (status: BoothFileStatus) => void): Promise<() => void> {
  if (!isTauri()) {
    return Promise.resolve(() => undefined);
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
  probeRemoteFiles: (id: string) => call<AvatarDetail>("probe_remote_files", { id }),
  queue: (kind: string, offset = 0, limit = 50) =>
    call<QueuePage>("list_queue", { kind, offset, limit }),
  managedProducts: () => call<ManagedProduct[]>("list_managed_products"),
  setProductExcluded: (boothId: string, excluded: boolean) => call<void>("set_product_excluded", { boothId, excluded }),
  productOptions: () => call<ProductOption[]>("list_product_options"),
  knowledge: () => call<Knowledge>("list_knowledge"),
  settings: () => call<Settings>("get_settings"),
  integrationStatus: () => call<IntegrationStatus>("integration_status"),
  setBoothImportMode: (mode: BoothImportMode) => call<void>("set_booth_import_mode", { mode }),
  extensionLink: () => call<ExtensionLink>("extension_link"),
  regenerateExtensionToken: () => call<ExtensionLink>("regenerate_extension_token"),
  saveSettings: (settings: Settings) => call<void>("save_settings", { settings }),
  updateSettings: (patch: Partial<Settings>) => call<Settings>("update_settings", { patch }),
  boothPageInterval: () => call<number>("get_booth_page_interval"),
  setBoothPageInterval: (intervalMs: number) => call<void>("set_booth_page_interval", { intervalMs }),
  browserState: () => call<BrowserState>("browser_state"),
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
  clearBoothSession: () => call<void>("clear_booth_session"),
  closeBoothBrowser: () => call<void>("close_booth_browser"),
  boothBrowserLog: () => call<BrowserDiagnostic>("booth_browser_log"),
  importExport: (json: string) => call<ImportReport>("import_export", { json }),
  updateProduct: (update: ProductUpdate) => call<void>("update_product", { update }),
  linkFile: (path: string, productId: string | null) =>
    call<void>("link_file", { path, productId }),
  saveAvatar: (avatar: AvatarInput) => call<string>("save_avatar", { avatar }),
  deleteAvatar: (id: string) => call<void>("delete_avatar", { id }),
  saveBaseBody: (body: { id?: string | null; displayName: string; note: string }) =>
    call<string>("save_base_body", { body }),
  deleteBaseBody: (id: string) => call<void>("delete_base_body", { id }),
  mergeAvatars: (fromId: string, intoId: string) =>
    call<void>("merge_avatars", { fromId, intoId }),
  restoreSeed: () => call<number>("restore_seed"),
  reresolve: () => call<number>("reresolve"),
  scanLibrary: (path: string) => call<ScanReport>("scan_library", { path }),
  extractProducts: (force: boolean) => call<ExtractReport>("extract_products", { force }),
  cancelExtract: () => call<void>("cancel_extract"),
  testOllama: () => call<OllamaStatus>("test_ollama"),
  appleFmStatus: () => call<AppleFmStatus>("apple_fm_status"),
  catalogInfo: () => call<CatalogInfo>("catalog_info"),
  refreshAvatarCatalog: () => call<CatalogRefresh>("refresh_avatar_catalog"),
  setupStatus: () => call<SetupStatus>("setup_status"),
  completeSetup: () => call<void>("complete_setup"),
  reopenSetup: () => call<void>("reopen_setup"),
  pullOllamaModel: (model: string) => call<void>("pull_ollama_model", { model }),
  avatarCandidates: () => call<AvatarCandidate[]>("avatar_candidates"),
  registerAvatarCandidate: (name: string) => call<string>("register_avatar_candidate", { name }),
  aliasAvatarCandidate: (name: string, avatarId: string) =>
    call<void>("alias_avatar_candidate", { name, avatarId }),
  markOwnedAvatar: (boothId: string) => call<string>("mark_owned_avatar", { boothId }),
  addFreeProduct: (url: string) => call<string>("add_free_product", { url }),
  pullRemoteFile: (boothId: string, variation: string, filename: string) =>
    call<void>("pull_remote_file", { boothId, variation, filename }),
  downloadBoothFile: (boothId: string, variation: string, filename: string) =>
    call<void>("download_booth_file", { boothId, variation, filename }),
  pushRemoteFile: (boothId: string, variation: string, filename: string) =>
    call<void>("push_remote_file", { boothId, variation, filename }),
  cacheThumbnails: () => call<number>("cache_thumbnails"),
  stagedDownloads: () => call<StagedDownload[]>("list_staged_downloads"),
  routeStagedDownload: (path: string) => call<void>("route_staged_download", { path }),
};

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

export async function openExternal(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}

export async function openLocal(path: string): Promise<void> {
  if (!isTauri()) {
    return;
  }
  const { openPath } = await import("@tauri-apps/plugin-opener");
  await openPath(path);
}

export function listenBrowserChanged(onChanged: () => void): Promise<() => void> {
  if (!isTauri()) return import("./preview/browser").then(({ onPreviewBrowserChanged }) => onPreviewBrowserChanged(onChanged));
  return import("@tauri-apps/api/event").then(({ listen }) => listen("browser-state-changed", onChanged));
}

export async function listenBrowserControlEvents(onFocus: () => void, onError: (message: string) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  const [focus, error] = await Promise.all([listen("browser-focus-address", onFocus), listen<string>("browser-operation-error", (event) => onError(event.payload))]);
  return () => { focus(); error(); };
}
