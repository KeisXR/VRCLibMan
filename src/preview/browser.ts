import type {
  BrowserAction,
  BrowserBookmark,
  BrowserHostKind,
  BrowserHostState,
  BrowserPanelRect,
  BrowserState,
  BrowserTab,
} from "../types";

const listeners = new Set<() => void>();
const panelListeners = new Set<(tabId?: string) => void>();
/** Preview stand-in for the "browser-panel-requested" event the app emits after docking. */
export function onPreviewBrowserPanelRequested(listener: (tabId?: string) => void) {
  panelListeners.add(listener);
  return () => {
    panelListeners.delete(listener);
  };
}
function requestPanel(tabId?: string) {
  for (const listener of panelListeners) listener(tabId);
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
const PANEL = "panel";
interface PreviewTab extends BrowserTab {
  entries: string[];
  index: number;
  hostId: string;
}
interface PreviewHost {
  id: string;
  kind: BrowserHostKind;
  order: string[];
  activeId: string | null;
}
let tabs: PreviewTab[] = [];
let hosts: PreviewHost[] = [];
let focused: string | null = null;
let preferWindow = false;
let nextWindow = 1;
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
function hostById(id: string | null | undefined) {
  return hosts.find((host) => host.id === id);
}
function ensureHostRecord(id: string, kind: BrowserHostKind) {
  const existing = hostById(id);
  if (existing) return existing;
  const host: PreviewHost = { id, kind, order: [], activeId: null };
  hosts.push(host);
  return host;
}
function windowHost() {
  let id = `window-${nextWindow}`;
  while (hostById(id)) {
    nextWindow += 1;
    id = `window-${nextWindow}`;
  }
  nextWindow += 1;
  return ensureHostRecord(id, "window");
}
function coarse(): BrowserHostKind | null {
  if (hosts.some((host) => host.kind === "panel" && host.order.length)) return "panel";
  if (hosts.some((host) => host.kind === "window" && host.order.length)) return "window";
  return null;
}
function focusedHost() {
  return (
    hostById(focused && hostById(focused)?.order.length ? focused : null) ??
    hosts.find((host) => host.order.length) ??
    null
  );
}
function focusedTabId() {
  return focusedHost()?.activeId ?? null;
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
    localStorage.setItem("vrclibman.preview.bookmarks", JSON.stringify(bookmarks));
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
    ["tauri.localhost", "asset.localhost", "ipc.localhost"].includes(parsed.hostname) ||
    parsed.hostname.endsWith(".tauri.localhost") ||
    (["localhost", "127.0.0.1", "[::1]"].includes(parsed.hostname) && parsed.port === "1420")
  )
    throw new Error("http または https のページを開いてください");
  return parsed.href;
}
function tabById(id: string | null | undefined) {
  return tabs.find((tab) => tab.id === id);
}
function update(tab: PreviewTab) {
  tab.url = tab.entries[tab.index] ?? tab.url;
  tab.title = new URL(tab.url).hostname;
  tab.canGoBack = tab.index > 0;
  tab.canGoForward = tab.index < tab.entries.length - 1;
}
function dropEmpty(id: string) {
  const host = hostById(id);
  if (!host || host.order.length) return;
  hosts = hosts.filter((item) => item.id !== id);
  if (focused === id) focused = hosts.find((item) => item.order.length)?.id ?? null;
  if (id === PANEL) panelArea = { area: null, visible: false };
}
function add(input = HOME, hostId?: string) {
  const target = url(input);
  if (tabs.length >= 20) throw new Error("タブは20個まで開けます");
  if (hostId === PANEL && !panelSupported()) {
    throw new Error("この環境ではブラウザを右パネルに表示できません。別ウィンドウで開きます");
  }
  let host = hostId ? hostById(hostId) : focusedHost();
  if (!host) {
    if (hostId === PANEL || (!hostId && startHost() === "panel")) host = ensureHostRecord(PANEL, "panel");
    else if (hostId) host = ensureHostRecord(hostId, "window");
    else host = windowHost();
  }
  const id = `booth-tab-${nextId++}`;
  tabs.push({
    id,
    url: target,
    title: new URL(target).hostname,
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomFactor: 1,
    hostId: host.id,
    entries: [target],
    index: 0,
  });
  host.order.push(id);
  host.activeId = id;
  focused = host.id;
  if (host.kind === "panel") requestPanel(id);
}
function moveTab(tabId: string, toId: string, index = Number.POSITIVE_INFINITY) {
  const tab = tabById(tabId);
  const from = tab ? hostById(tab.hostId) : undefined;
  const to = hostById(toId);
  if (!tab || !from || !to || from.id === to.id) throw new Error("タブが見つかりません");
  const fromIndex = from.order.indexOf(tabId);
  from.order.splice(fromIndex, 1);
  if (from.activeId === tabId) {
    from.activeId = from.order[Math.min(fromIndex, from.order.length - 1)] ?? null;
  }
  const at = Math.max(0, Math.min(index, to.order.length));
  to.order.splice(at, 0, tabId);
  to.activeId = tabId;
  tab.hostId = to.id;
  focused = to.id;
  dropEmpty(from.id);
}
function hostState(): BrowserHostState[] {
  return hosts
    .filter((host) => host.order.length)
    .map((host) => ({
      id: host.id,
      kind: host.kind,
      tabIds: [...host.order],
      activeId: host.activeId,
    }));
}
export function previewBrowserState(): BrowserState {
  return {
    open: tabs.length > 0,
    activeId: focusedTabId(),
    tabs: tabs.map(({ entries: _entries, index: _index, ...tab }) => ({ ...tab })),
    hosts: hostState(),
    bookmarks: bookmarks.map((item) => ({ ...item })),
    busy: false,
    host: coarse(),
    panelSupported: panelSupported(),
  };
}
function targetTab(action: { tabId?: string; hostId?: string }) {
  return tabById(action.tabId) ?? tabById(hostById(action.hostId)?.activeId) ?? tabById(focusedTabId());
}
export function previewBrowserAction(action: BrowserAction): BrowserState {
  if (action.kind === "newTab") {
    add(action.url, action.hostId);
  } else if (action.kind === "dock") {
    if (!panelSupported())
      throw new Error("この環境ではブラウザを右パネルに表示できません。別ウィンドウで開きます");
    preferWindow = false;
    const moving = action.hostId
      ? [...(hostById(action.hostId)?.order ?? [])]
      : hosts.filter((host) => host.kind === "window").flatMap((host) => [...host.order]);
    if (!tabs.length) add();
    ensureHostRecord(PANEL, "panel");
    for (const id of moving) moveTab(id, PANEL);
    panelArea = { area: action.area, visible: action.area != null };
    focused = PANEL;
    requestPanel(hostById(PANEL)?.activeId ?? undefined);
  } else if (action.kind === "undock") {
    preferWindow = true;
    const moving = [...(hostById(PANEL)?.order ?? [])];
    if (!tabs.length) {
      add();
      const created = focusedHost();
      if (created?.kind === "panel") {
        const window = windowHost();
        moveTab(created.order[0]!, window.id);
      }
    } else if (moving.length) {
      const window = windowHost();
      for (const id of moving) moveTab(id, window.id);
    }
    panelArea = { area: null, visible: false };
  } else if (action.kind === "panelArea") {
    if (hostById(PANEL)?.order.length)
      panelArea = { area: action.area, visible: action.visible && action.area != null };
  } else if (action.kind === "navigate") {
    const target = url(action.url);
    const tab = targetTab(action);
    if (!tab) add(target, action.hostId);
    else {
      tab.entries = [...tab.entries.slice(0, tab.index + 1), target];
      tab.index += 1;
      update(tab);
      const host = hostById(tab.hostId);
      if (host) {
        host.activeId = tab.id;
        focused = host.id;
      }
    }
  } else if (action.kind === "selectTab") {
    const tab = tabById(action.tabId);
    if (!tab) throw new Error("タブが見つかりません");
    const host = hostById(tab.hostId);
    if (host) host.activeId = tab.id;
    focused = tab.hostId;
  } else if (action.kind === "closeTab") {
    const tab = tabById(action.tabId);
    if (!tab) throw new Error("タブが見つかりません");
    const host = hostById(tab.hostId);
    const index = host?.order.indexOf(tab.id) ?? -1;
    host?.order.splice(index, 1);
    if (host?.activeId === tab.id)
      host.activeId = host.order[Math.min(index, host.order.length - 1)] ?? null;
    tabs = tabs.filter((item) => item.id !== tab.id);
    if (host) dropEmpty(host.id);
  } else if (action.kind === "back" || action.kind === "forward") {
    const tab = targetTab(action);
    if (tab) {
      tab.index = Math.max(0, Math.min(tab.entries.length - 1, tab.index + (action.kind === "forward" ? 1 : -1)));
      update(tab);
    }
  } else if (action.kind === "zoom") {
    const tab = targetTab(action);
    if (tab) {
      const levels = [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5];
      tab.zoomFactor = action.action === "reset" ? 1 : action.action === "in"
        ? (levels.find((level) => level > tab.zoomFactor + 0.001) ?? 5)
        : ([...levels].reverse().find((level) => level < tab.zoomFactor - 0.001) ?? 0.25);
    }
  } else if (action.kind === "reorderTabs") {
    const host = hostById(action.hostId);
    if (!host || action.tabIds.length !== host.order.length || host.order.some((id) => !action.tabIds.includes(id)))
      throw new Error("タブの並びが一致しません");
    host.order = [...action.tabIds];
  } else if (action.kind === "detachTab") {
    if (!Number.isFinite(action.x) || !Number.isFinite(action.y)) throw new Error("分離先の位置が不正です");
    const window = windowHost();
    moveTab(action.tabId, window.id, 0);
    preferWindow = true;
  } else if (action.kind === "dockTab") {
    if (!panelSupported())
      throw new Error("この環境ではブラウザを右パネルに表示できません。別ウィンドウで開きます");
    preferWindow = false;
    ensureHostRecord(PANEL, "panel");
    moveTab(action.tabId, PANEL, action.index);
    requestPanel(action.tabId);
  }
  changed();
  return previewBrowserState();
}
export function previewSaveBookmark(originalUrl: string | null, title: string, input: string): BrowserState {
  const target = url(input);
  const item = { title: title.trim().slice(0, 100) || new URL(target).hostname, url: target };
  const original = originalUrl == null ? -1 : bookmarks.findIndex((item) => item.url === originalUrl);
  if (originalUrl != null && original < 0) throw new Error("編集するブックマークが見つかりません");
  if (original >= 0) {
    bookmarks[original] = item;
    bookmarks = bookmarks.filter(
      (entry, index, all) => all.findIndex((candidate) => candidate.url === entry.url) === index,
    );
    bookmarks.find((entry) => entry.url === target)!.title = item.title;
  } else {
    const found = bookmarks.find((item) => item.url === target);
    if (found) found.title = item.title;
    else {
      if (bookmarks.length >= 200) throw new Error("ブックマークは200件まで保存できます");
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
  if (!focusedTabId()) add();
  const host = focusedHost();
  if (host?.kind === "panel") requestPanel(host.activeId ?? undefined);
  changed();
}
export function previewCloseBrowser() {
  tabs = [];
  hosts = [];
  focused = null;
  panelArea = { area: null, visible: false };
  changed();
}
/** Test helper: forget the session and the "open in its own window" choice. */
export function resetPreviewBrowser() {
  previewCloseBrowser();
  preferWindow = false;
}
if (typeof location !== "undefined" && new URLSearchParams(location.search).get("browserToolbar") === "1") {
  const requested = new URLSearchParams(location.search).get("browserHost");
  add(HOME, requested && requested.startsWith("window-") ? requested : undefined);
  const host = focusedHost();
  if (host && host.kind !== "window") {
    const window = windowHost();
    if (host.order[0]) moveTab(host.order[0], window.id);
  }
}
