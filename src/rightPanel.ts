import { PREVIEW_PANE_WIDTH, clampStoredPreviewPaneWidth } from "./previewPane";

/**
 * Right panel tab kinds. A kind is enabled by flipping `available`
 * and registering a renderer in `components/rightPanelRegistry.tsx`.
 * The same allow-list lives in `src-tauri/src/model.rs` (`RIGHT_PANEL_KINDS`, `RIGHT_PANEL_SINGLETONS`).
 */
export const RIGHT_PANEL_KINDS = ["preview", "files"] as const;
export type RightPanelKind = (typeof RIGHT_PANEL_KINDS)[number];

export interface RightPanelKindInfo {
  kind: RightPanelKind;
  label: string;
  /** Only one tab of this kind may exist. */
  singleton: boolean;
  /** Shown in the add menu and rendered. Unavailable kinds are kept in settings but hidden. */
  available: boolean;
  /** Shown only while this app setting is on. The browser follows "BOOTH の読み込み方" (built-in browser). */
  requires?: keyof RightPanelGates;
}

/** Settings that turn tab kinds on or off at run time. */
export interface RightPanelGates {
  builtinBrowser?: boolean;
}

export const RIGHT_PANEL_KIND_INFO: Record<RightPanelKind, RightPanelKindInfo> = {
  preview: { kind: "preview", label: "プレビュー", singleton: true, available: true },
  files: { kind: "files", label: "ファイル", singleton: true, available: true },
};

export const RIGHT_PANEL_MAX_TABS = 12;
export const RIGHT_PANEL_LIST_MIN = 360;
export const RIGHT_PANEL_SEPARATOR = 6;
export const RIGHT_PANEL_SHORTCUT_LABEL = "Ctrl+Shift+B";

export interface RightPanelTab {
  /** Session-only id for React keys. Not stored. */
  id: string;
  kind: RightPanelKind | "web";
  /** Rust page id. Present only for a web tab, and never written to settings. */
  tabId?: string;
  /** Page title shown on a web tab. Not stored. */
  title?: string;
}

export interface RightPanelTabs {
  tabs: RightPanelTab[];
  activeId: string | null;
}

let nextTabNumber = 1;
export function newTabId(kind: RightPanelKind): string {
  const id = `${kind}-${nextTabNumber}`;
  nextTabNumber += 1;
  return id;
}

/** The kind is implemented and its setting is on. */
export function kindAvailable(kind: RightPanelKind, gates: RightPanelGates = {}): boolean {
  const info = RIGHT_PANEL_KIND_INFO[kind];
  if (!info.available) return false;
  return info.requires ? gates[info.requires] === true : true;
}

export function isRightPanelKind(value: unknown): value is RightPanelKind {
  return typeof value === "string" && (RIGHT_PANEL_KINDS as readonly string[]).includes(value);
}

export function defaultRightPanelTabs(): RightPanelTabs {
  const tab = { id: newTabId("preview"), kind: "preview" as const };
  return { tabs: [tab], activeId: tab.id };
}

/** Read the stored JSON. Unknown kinds, duplicates of singletons and free text are dropped. */
export function parseRightPanelTabs(raw: string | null | undefined): RightPanelTabs {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw ?? "");
  } catch {
    return defaultRightPanelTabs();
  }
  if (!parsed || typeof parsed !== "object") return defaultRightPanelTabs();
  const record = parsed as { tabs?: unknown; active?: unknown };
  if (!Array.isArray(record.tabs)) return defaultRightPanelTabs();
  const wanted = typeof record.active === "number" && Number.isInteger(record.active) ? record.active : 0;
  const tabs: RightPanelTab[] = [];
  let activeIndex = 0;
  record.tabs.forEach((kind, index) => {
    if (tabs.length >= RIGHT_PANEL_MAX_TABS || !isRightPanelKind(kind)) return;
    if (RIGHT_PANEL_KIND_INFO[kind].singleton) {
      const existing = tabs.findIndex((tab) => tab.kind === kind);
      if (existing >= 0) {
        // An active duplicate keeps focus on the tab it collapsed into.
        if (index === wanted) activeIndex = existing;
        return;
      }
    }
    if (index <= wanted) activeIndex = tabs.length;
    tabs.push({ id: newTabId(kind), kind });
  });
  return { tabs, activeId: tabs[activeIndex]?.id ?? null };
}

/** Only fixed kinds and their active index are stored. Page addresses and web tabs are not. */
export function serializeRightPanelTabs(state: RightPanelTabs): string {
  const tabs = state.tabs.filter((tab): tab is RightPanelTab & { kind: RightPanelKind } => tab.kind !== "web").slice(0, RIGHT_PANEL_MAX_TABS);
  const activeTab = state.tabs.find((tab) => tab.id === state.activeId);
  let active = 0;
  if (activeTab && activeTab.kind !== "web") {
    active = Math.max(0, tabs.findIndex((tab) => tab.id === activeTab.id));
  } else {
    const index = state.tabs.findIndex((tab) => tab.id === state.activeId);
    const prior = state.tabs.slice(0, Math.max(0, index)).filter((tab) => tab.kind !== "web");
    active = prior.length === 0 ? 0 : prior.length - 1;
  }
  if (!tabs.length) active = 0;
  return JSON.stringify({ tabs: tabs.map((tab) => tab.kind), active });
}

/** Page tabs follow the built-in browser setting. They are not a stored kind. */
export function pageTabsAllowed(gates: RightPanelGates = {}): boolean {
  return gates.builtinBrowser === true;
}

export function tabLabel(tab: RightPanelTab): string {
  if (tab.kind === "web") return tab.title?.trim() || "ページ";
  return RIGHT_PANEL_KIND_INFO[tab.kind].label;
}

/** Tabs whose kind is implemented in this build and turned on. Hidden tabs stay in settings. */
export function visibleTabs(state: RightPanelTabs, gates: RightPanelGates = {}): RightPanelTab[] {
  return state.tabs.filter((tab) => {
    if (tab.kind === "web") return pageTabsAllowed(gates) && Boolean(tab.tabId);
    return kindAvailable(tab.kind, gates);
  });
}

export function activeTab(state: RightPanelTabs, gates: RightPanelGates = {}): RightPanelTab | null {
  const shown = visibleTabs(state, gates);
  return shown.find((tab) => tab.id === state.activeId) ?? shown[0] ?? null;
}

/** The closed panel's launcher badge: the tabs the panel would show, at most "99+". */
export function launcherCountLabel(state: RightPanelTabs, gates: RightPanelGates = {}): string {
  const count = visibleTabs(state, gates).length;
  return count > 99 ? "99+" : String(count);
}

export function canAddTab(state: RightPanelTabs, kind: RightPanelKind, gates: RightPanelGates = {}): boolean {
  const info = RIGHT_PANEL_KIND_INFO[kind];
  if (!kindAvailable(kind, gates)) return false;
  if (info.singleton && state.tabs.some((tab) => tab.kind === kind)) return true;
  const fixed = state.tabs.filter((tab) => tab.kind !== "web").length;
  return fixed < RIGHT_PANEL_MAX_TABS;
}

/** Add a tab, or focus the existing one for single-instance kinds. */
export function openTab(state: RightPanelTabs, kind: RightPanelKind): RightPanelTabs {
  const info = RIGHT_PANEL_KIND_INFO[kind];
  if (info.singleton) {
    const existing = state.tabs.find((tab) => tab.kind === kind);
    if (existing) return { ...state, activeId: existing.id };
  }
  if (state.tabs.length >= RIGHT_PANEL_MAX_TABS) return state;
  const tab = { id: newTabId(kind), kind };
  return { tabs: [...state.tabs, tab], activeId: tab.id };
}

export function selectTab(state: RightPanelTabs, id: string): RightPanelTabs {
  if (!state.tabs.some((tab) => tab.id === id)) return state;
  return { ...state, activeId: id };
}

/** Closing the active tab focuses its right neighbour, or the left one at the end. */
export function closeTab(state: RightPanelTabs, id: string): RightPanelTabs {
  const index = state.tabs.findIndex((tab) => tab.id === id);
  if (index < 0) return state;
  const tabs = state.tabs.filter((tab) => tab.id !== id);
  if (state.activeId !== id) return { tabs, activeId: state.activeId };
  const next = tabs[index] ?? tabs[index - 1] ?? null;
  return { tabs, activeId: next?.id ?? null };
}

export function moveTab(state: RightPanelTabs, id: string, offset: number): RightPanelTabs {
  const index = state.tabs.findIndex((tab) => tab.id === id);
  const target = index + offset;
  if (index < 0 || target < 0 || target >= state.tabs.length) return state;
  const tabs = [...state.tabs];
  const [tab] = tabs.splice(index, 1);
  tabs.splice(target, 0, tab!);
  return { ...state, tabs };
}

/** Ctrl+Shift+B (Cmd+Shift+B on Mac) shows or hides the panel. */
export function isRightPanelShortcut(event: {
  key: string;
  code?: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}): boolean {
  if (!event.shiftKey || event.altKey) return false;
  if (!(event.ctrlKey || event.metaKey)) return false;
  return event.code === "KeyB" || event.key.toLowerCase() === "b";
}

/**
 * Docked width on screen. The stored width is kept as is; the panel takes at most half the
 * window and leaves room for the rail, the genre column and a usable list.
 */
export function rightPanelMaxWidth(windowWidth: number, leftColumns: number): number {
  const half = Math.floor(windowWidth / 2);
  const room = Math.floor(windowWidth - leftColumns - RIGHT_PANEL_SEPARATOR - RIGHT_PANEL_LIST_MIN);
  return Math.max(PREVIEW_PANE_WIDTH.min, Math.min(PREVIEW_PANE_WIDTH.absoluteMax, half, room));
}

export function fitRightPanelWidth(stored: number, windowWidth: number, leftColumns: number): number {
  const preferred = clampStoredPreviewPaneWidth(stored);
  return Math.min(rightPanelMaxWidth(windowWidth, leftColumns), preferred);
}

export interface PanelPage {
  tabId: string;
  title: string;
}

/** Keep the user's order. Drop pages that left the panel and append pages that arrived. */
export function mergePanelWebTabs(state: RightPanelTabs, pages: PanelPage[]): RightPanelTabs {
  const titles = new Map(pages.map((page) => [page.tabId, page.title.trim() || "ページ"]));
  let changed = false;
  const tabs: RightPanelTab[] = [];
  for (const tab of state.tabs) {
    if (tab.kind !== "web" || !tab.tabId) {
      tabs.push(tab);
      continue;
    }
    const title = titles.get(tab.tabId);
    if (title == null) {
      changed = true;
      continue;
    }
    if (tab.title !== title) {
      changed = true;
      tabs.push({ ...tab, title });
    } else {
      tabs.push(tab);
    }
  }
  for (const page of pages) {
    if (tabs.some((tab) => tab.kind === "web" && tab.tabId === page.tabId)) continue;
    changed = true;
    tabs.push({ id: `web-${page.tabId}`, kind: "web", tabId: page.tabId, title: titles.get(page.tabId) });
  }
  let activeId = state.activeId;
  if (activeId && !tabs.some((tab) => tab.id === activeId)) {
    changed = true;
    const index = state.tabs.findIndex((tab) => tab.id === activeId);
    activeId = (tabs[index] ?? tabs[index - 1] ?? null)?.id ?? null;
  }
  if (!changed) return state;
  return { tabs, activeId };
}

export function focusWebTab(state: RightPanelTabs, tabId: string, title = "ページ"): RightPanelTabs {
  const existing = state.tabs.find((tab) => tab.kind === "web" && tab.tabId === tabId);
  if (existing) return existing.id === state.activeId ? state : { ...state, activeId: existing.id };
  return {
    tabs: [...state.tabs, { id: `web-${tabId}`, kind: "web", tabId, title: title.trim() || "ページ" }],
    activeId: `web-${tabId}`,
  };
}

/** Move `id` so it sits at `index` in the list before the removal (a gap index). */
export function placeTab(state: RightPanelTabs, id: string, index: number): RightPanelTabs {
  const from = state.tabs.findIndex((tab) => tab.id === id);
  if (from < 0) return state;
  const tabs = state.tabs.slice();
  const [tab] = tabs.splice(from, 1);
  if (!tab) return state;
  let target = index;
  if (target > from) target -= 1;
  target = Math.max(0, Math.min(target, tabs.length));
  if (target === from) return state;
  tabs.splice(target, 0, tab);
  return { ...state, tabs };
}

export function webTabIds(state: RightPanelTabs): string[] {
  return state.tabs.flatMap((tab) => (tab.kind === "web" && tab.tabId ? [tab.tabId] : []));
}

/** The page tabs keep their order. Moving a preview or file tab between pages leaves it as it was. */
export function sameWebOrder(a: RightPanelTabs, b: RightPanelTabs): boolean {
  const before = webTabIds(a);
  const after = webTabIds(b);
  return before.length === after.length && before.every((id, index) => id === after[index]);
}
