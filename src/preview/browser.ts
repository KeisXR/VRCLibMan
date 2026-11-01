import type {
  BrowserAction,
  BrowserBookmark,
  BrowserHostKind,
  BrowserPanelRect,
  BrowserState,
  BrowserTab,
} from "../types";
const listeners = new Set<() => void>();
const panelListeners = new Set<() => void>();
/** Preview stand-in for the "browser-panel-requested" event the app emits after docking. */
export function onPreviewBrowserPanelRequested(listener: () => void) {
  panelListeners.add(listener);
  return () => {
    panelListeners.delete(listener);
  };
}
function requestPanel() {
  for (const listener of panelListeners) listener();
}
function changed() {
  for (const listener of listeners) listener();
}
export function onPreviewBrowserChanged(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
const HOME = "https://accounts.booth.pm/library";
interface PreviewTab extends BrowserTab {
  entries: string[];
  index: number;
}
let tabs: PreviewTab[] = [];
let activeId: string | null = null;
let host: BrowserHostKind | null = null;
let preferWindow = false;
let panelArea: { area: BrowserPanelRect | null; visible: boolean } = { area: null, visible: false };
/** `?browserPanel=0` previews an OS without the docked browser (macOS and Linux today). */
function panelSupported() {
  return !(
    typeof location !== "undefined" &&
    new URLSearchParams(location.search).get("browserPanel") === "0"
  );
}
/** The last rectangle the right panel reported, for checks in the browser preview. */
export function previewBrowserPanelArea() {
  return { area: panelArea.area ? { ...panelArea.area } : null, visible: panelArea.visible };
}
/** Same rule as `start_in_panel` in `src-tauri/src/browser_surface.rs`. */
function startHost(): BrowserHostKind {
  return panelSupported() && !preferWindow ? "panel" : "window";
}
function ensureHost() {
  if (host) return;
  host = startHost();
  if (host === "panel") requestPanel();
}
let nextId = 1;
const defaults = [
  { title: "BOOTH", url: "https://booth.pm/" },
  { title: "購入ライブラリ", url: HOME },
];
let bookmarks: BrowserBookmark[] = (() => {
  try {
    const saved = localStorage.getItem("vrclibman.preview.bookmarks");
    if (saved) return JSON.parse(saved) as BrowserBookmark[];
  } catch {
    /* Preview storage is optional. */
  }
  return defaults;
})();
function persist() {
  try {
    localStorage.setItem(
      "vrclibman.preview.bookmarks",
      JSON.stringify(bookmarks),
    );
  } catch {
    /* Preview storage is optional. */
  }
}
function url(input: string): string {
  const value = input.trim();
  if (!value || value.length > 4096 || /[\x00-\x1f]/.test(value))
    throw new Error("URLを入力してください（4096文字以内）");
  let parsed: URL;
  try {
    parsed = new URL(value.includes("://") ? value : `https://${value}`);
  } catch {
    throw new Error("URLが正しくありません");
  }
  if (
    !["http:", "https:"].includes(parsed.protocol) ||
    parsed.username ||
    parsed.password ||
    ["tauri.localhost", "asset.localhost", "ipc.localhost"].includes(
      parsed.hostname,
    ) ||
    parsed.hostname.endsWith(".tauri.localhost") ||
    (["localhost", "127.0.0.1", "[::1]"].includes(parsed.hostname) &&
      parsed.port === "1420")
  )
    throw new Error("http または https のページを開いてください");
  return parsed.href;
}
function active() {
  return tabs.find((tab) => tab.id === activeId);
}
function update(tab: PreviewTab) {
  tab.url = tab.entries[tab.index];
  tab.title = new URL(tab.url).hostname;
  tab.canGoBack = tab.index > 0;
  tab.canGoForward = tab.index < tab.entries.length - 1;
}
function add(input = HOME) {
  const target = url(input);
  if (tabs.length >= 20) throw new Error("タブは20個まで開けます");
  const id = `booth-tab-${nextId++}`;
  tabs.push({
    id,
    url: target,
    title: new URL(target).hostname,
    loading: false,
    canGoBack: false,
    canGoForward: false,
    entries: [target],
    index: 0,
  });
  activeId = id;
}
export function previewBrowserState(): BrowserState {
  return {
    open: tabs.length > 0,
    activeId,
    tabs: tabs.map(({ entries: _entries, index: _index, ...tab }) => ({
      ...tab,
    })),
    bookmarks: bookmarks.map((item) => ({ ...item })),
    busy: false,
    host,
    panelSupported: panelSupported(),
  };
}
export function previewBrowserAction(action: BrowserAction): BrowserState {
  if (action.kind === "newTab") {
    add(action.url);
    ensureHost();
  } else if (action.kind === "dock") {
    if (!panelSupported())
      throw new Error("この環境ではブラウザを右パネルに表示できません。別ウィンドウで開きます");
    preferWindow = false;
    if (!tabs.length) add();
    host = "panel";
    panelArea = { area: action.area, visible: action.area != null };
    requestPanel();
  } else if (action.kind === "undock") {
    preferWindow = true;
    if (!tabs.length) add();
    host = "window";
    panelArea = { area: null, visible: false };
  } else if (action.kind === "panelArea") {
    if (host === "panel")
      panelArea = { area: action.area, visible: action.visible && action.area != null };
  } else if (action.kind === "navigate") {
    const target = url(action.url);
    const tab = active();
    if (!tab) {
      add(target);
      ensureHost();
    } else {
      tab.entries = [...tab.entries.slice(0, tab.index + 1), target];
      tab.index++;
      update(tab);
    }
  } else if (action.kind === "selectTab") {
    if (!tabs.some((tab) => tab.id === action.tabId))
      throw new Error("タブが見つかりません");
    activeId = action.tabId;
  } else if (action.kind === "closeTab") {
    const index = tabs.findIndex((tab) => tab.id === action.tabId);
    if (index < 0) throw new Error("タブが見つかりません");
    tabs.splice(index, 1);
    if (activeId === action.tabId)
      activeId = tabs[Math.min(index, tabs.length - 1)]?.id ?? null;
    if (!tabs.length) host = null;
  } else if (action.kind === "back" || action.kind === "forward") {
    const tab = active();
    if (tab) {
      tab.index = Math.max(
        0,
        Math.min(
          tab.entries.length - 1,
          tab.index + (action.kind === "forward" ? 1 : -1),
        ),
      );
      update(tab);
    }
  }
  changed();
  return previewBrowserState();
}
export function previewSaveBookmark(
  originalUrl: string | null,
  title: string,
  input: string,
): BrowserState {
  const target = url(input);
  const item = {
    title: title.trim().slice(0, 100) || new URL(target).hostname,
    url: target,
  };
  const original =
    originalUrl == null
      ? -1
      : bookmarks.findIndex((item) => item.url === originalUrl);
  if (originalUrl != null && original < 0)
    throw new Error("編集するブックマークが見つかりません");
  if (original >= 0) {
    bookmarks[original] = item;
    bookmarks = bookmarks.filter(
      (entry, index, all) =>
        all.findIndex((candidate) => candidate.url === entry.url) === index,
    );
    bookmarks.find((entry) => entry.url === target)!.title = item.title;
  } else {
    const found = bookmarks.find((item) => item.url === target);
    if (found) found.title = item.title;
    else {
      if (bookmarks.length >= 200)
        throw new Error("ブックマークは200件まで保存できます");
      bookmarks.push(item);
    }
  }
  persist();
  changed();
  return previewBrowserState();
}
export function previewDeleteBookmark(target: string): BrowserState {
  bookmarks = bookmarks.filter((item) => item.url !== target);
  persist();
  changed();
  return previewBrowserState();
}
export function previewOpenBrowser() {
  if (!active()) add();
  if (host === "panel") requestPanel();
  ensureHost();
  changed();
}
export function previewCloseBrowser() {
  tabs = [];
  activeId = null;
  host = null;
  panelArea = { area: null, visible: false };
  changed();
}
/** Test helper: forget the session and the "open in its own window" choice. */
export function resetPreviewBrowser() {
  previewCloseBrowser();
  preferWindow = false;
}
if (
  typeof location !== "undefined" &&
  new URLSearchParams(location.search).get("browserToolbar") === "1"
) {
  add();
  host = "window";
}
