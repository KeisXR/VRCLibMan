import { PREVIEW_PANE_WIDTH, clampStoredPreviewPaneWidth } from "./previewPane";

/**
 * Right panel tab kinds. A later PR adds "files" by flipping `available`
 * and registering a renderer in `components/rightPanelRegistry.tsx`.
 * The same allow-list lives in `src-tauri/src/model.rs` (`RIGHT_PANEL_KINDS`, `RIGHT_PANEL_SINGLETONS`).
 */
export const RIGHT_PANEL_KINDS = ["preview", "files", "browser"] as const;
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
  files: { kind: "files", label: "ファイル", singleton: true, available: false },
  // One browser session (one set of BOOTH tabs), so one panel tab shows it.
  browser: { kind: "browser", label: "ブラウザ", singleton: true, available: true, requires: "builtinBrowser" },
};

export const RIGHT_PANEL_MAX_TABS = 12;
export const RIGHT_PANEL_LIST_MIN = 360;
export const RIGHT_PANEL_SEPARATOR = 6;
export const RIGHT_PANEL_SHORTCUT_LABEL = "Ctrl+Shift+B";

export interface RightPanelTab {
  /** Session-only id for React keys. Not stored. */
  id: string;
  kind: RightPanelKind;
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

/** Only kinds and the active index are stored. Page addresses never go into settings. */
export function serializeRightPanelTabs(state: RightPanelTabs): string {
  const tabs = state.tabs.slice(0, RIGHT_PANEL_MAX_TABS);
  const active = Math.max(0, tabs.findIndex((tab) => tab.id === state.activeId));
  return JSON.stringify({ tabs: tabs.map((tab) => tab.kind), active });
}

/** Tabs whose kind is implemented in this build and turned on. Hidden tabs stay in settings. */
export function visibleTabs(state: RightPanelTabs, gates: RightPanelGates = {}): RightPanelTab[] {
  return state.tabs.filter((tab) => kindAvailable(tab.kind, gates));
}

export function activeTab(state: RightPanelTabs, gates: RightPanelGates = {}): RightPanelTab | null {
  const shown = visibleTabs(state, gates);
  return shown.find((tab) => tab.id === state.activeId) ?? shown[0] ?? null;
}

export function canAddTab(state: RightPanelTabs, kind: RightPanelKind, gates: RightPanelGates = {}): boolean {
  const info = RIGHT_PANEL_KIND_INFO[kind];
  if (!kindAvailable(kind, gates)) return false;
  if (info.singleton && state.tabs.some((tab) => tab.kind === kind)) return true;
  return state.tabs.length < RIGHT_PANEL_MAX_TABS;
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
