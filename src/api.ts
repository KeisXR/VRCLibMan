import { invoke } from "@tauri-apps/api/core";
import type {
  AvatarCard,
  AvatarDetail,
  AvatarInput,
  AvatarCandidate,
  CatalogInfo,
  CatalogRefresh,
  ExtractReport,
  ExtensionLink,
  ImportReport,
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
} from "./types";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
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

export function listenProgress(onProgress: (progress: JobProgress) => void): Promise<() => void> {
  if (!isTauri()) {
    return Promise.resolve(() => undefined);
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen<JobProgress>("job-progress", (event) => onProgress(event.payload)),
  );
}

export const api = {
  stats: () => call<LibraryStats>("library_stats"),
  avatars: () => call<AvatarCard[]>("list_avatars"),
  avatar: (id: string) => call<AvatarDetail>("get_avatar", { id }),
  probeRemoteFiles: (id: string) => call<AvatarDetail>("probe_remote_files", { id }),
  queue: (kind: string, offset = 0, limit = 50) =>
    call<QueuePage>("list_queue", { kind, offset, limit }),
  productOptions: () => call<ProductOption[]>("list_product_options"),
  knowledge: () => call<Knowledge>("list_knowledge"),
  settings: () => call<Settings>("get_settings"),
  extensionLink: () => call<ExtensionLink>("extension_link"),
  regenerateExtensionToken: () => call<ExtensionLink>("regenerate_extension_token"),
  saveSettings: (settings: Settings) => call<void>("save_settings", { settings }),
  openBoothBrowser: () => call<void>("open_booth_browser"),
  showBoothLibrary: () => call<void>("show_booth_library"),
  browseBoothHistory: (forward: boolean) => call<void>("browse_booth_history", { forward }),
  importBoothPage: () => call<ImportReport>("import_booth_page"),
  clearBoothSession: () => call<void>("clear_booth_session"),
  closeBoothBrowser: () => call<void>("close_booth_browser"),
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
  pullOllamaModel: () => call<void>("pull_ollama_model"),
  avatarCandidates: () => call<AvatarCandidate[]>("avatar_candidates"),
  registerAvatarCandidate: (name: string) => call<string>("register_avatar_candidate", { name }),
  aliasAvatarCandidate: (name: string, avatarId: string) =>
    call<void>("alias_avatar_candidate", { name, avatarId }),
  markOwnedAvatar: (boothId: string) => call<string>("mark_owned_avatar", { boothId }),
  addFreeProduct: (url: string) => call<string>("add_free_product", { url }),
  pullRemoteFile: (boothId: string, variation: string, filename: string) =>
    call<void>("pull_remote_file", { boothId, variation, filename }),
  pushRemoteFile: (boothId: string, variation: string, filename: string) =>
    call<void>("push_remote_file", { boothId, variation, filename }),
  cacheThumbnails: () => call<number>("cache_thumbnails"),
};

export async function saveLibraryAndScan(folder: string, settings?: Settings): Promise<ScanReport> {
  const current = settings ?? (await api.settings());
  await api.saveSettings({ ...current, libraryFolder: folder });
  return api.scanLibrary(folder);
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
