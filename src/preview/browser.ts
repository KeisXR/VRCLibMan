import type {
  BrowserAction,
  BrowserBookmark,
  BrowserState,
  BrowserTab,
} from "../types";
const listeners = new Set<() => void>();
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
  };
}
export function previewBrowserAction(action: BrowserAction): BrowserState {
  if (action.kind === "newTab") add(action.url);
  else if (action.kind === "navigate") {
    const target = url(action.url);
    const tab = active();
    if (!tab) add(target);
    else {
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
  changed();
}
export function previewCloseBrowser() {
  tabs = [];
  activeId = null;
  changed();
}
if (
  typeof location !== "undefined" &&
  new URLSearchParams(location.search).get("browserToolbar") === "1"
)
  add();
