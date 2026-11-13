import { createContext, memo, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent, ReactNode } from "react";
import { createPortal } from "react-dom";
import { api, listenBrowserPanelRequested } from "../api";
import { applyBrowserState, getBrowserSnapshot, subscribeBrowserState } from "../browserStore";
import {
  FLOAT_PANEL_EDGES,
  FLOAT_PANEL_MOVED_EVENT,
  FLOAT_PANEL_MOVING_CLASS,
  FLOAT_PANEL_STEP,
  compactFloatRect,
  moveFloatRect,
  placeFloatRect,
  resizeFloatRect,
} from "../floatingPanel";
import type { FloatEdge, FloatRect } from "../floatingPanel";
import { dragOutcome, insertionIndex, insertMarkerLeft, pointerInsideWindow, TAB_DRAG_THRESHOLD_PX } from "../tabStrip";
import { horizontalWheelDelta, overflowEdges, revealScrollLeft } from "../scrollEdges";
import { PREVIEW_PANE_WIDTH, clampStoredPreviewPaneWidth } from "../previewPane";
import {
  RIGHT_PANEL_KINDS,
  RIGHT_PANEL_KIND_INFO,
  RIGHT_PANEL_SHORTCUT_LABEL,
  activeTab,
  canAddTab,
  closeTab,
  defaultRightPanelTabs,
  focusWebTab,
  isRightPanelShortcut,
  kindAvailable,
  launcherCountLabel,
  mergePanelWebTabs,
  moveTab,
  openTab,
  pageTabsAllowed,
  parseRightPanelTabs,
  placeTab,
  sameWebOrder,
  selectTab,
  serializeRightPanelTabs,
  tabLabel,
  visibleTabs,
  webTabIds,
} from "../rightPanel";
import type { RightPanelGates, RightPanelKind, RightPanelTab, RightPanelTabs } from "../rightPanel";
import type { BrowserAction, BrowserState } from "../types";
import { BrowserPanelTab } from "./BrowserPanel";
import type { ItemView } from "../types";
import { errorText } from "../ui";
import { ResizeSeparator } from "./ResizeSeparator";
import { CloseGlyph, FolderGlyph, GlobeGlyph, PictureGlyph, PlusGlyph } from "./glyphs";
import { RIGHT_PANEL_RENDERERS } from "./rightPanelRegistry";

/** What the asset or world list currently selects for the preview tab. */
export interface PreviewSelection {
  item: ItemView | null;
  clear: () => void;
}

/**
 * Whether and where the right panel is on screen. The app shell holds this because it sets the
 * grid. The tabs live in `RightPanelProvider` instead, so choosing, moving or renaming a tab does
 * not draw the app shell and the page beside the panel again.
 */
export interface RightPanelShell {
  open: boolean;
  maximized: boolean;
  /** The user chose to float the panel as a window in a wide window too. Not saved, like maximized. */
  floating: boolean;
  width: number;
  /** Settings that show or hide tab kinds (the browser needs the built-in browser). */
  gates: RightPanelGates;
  /** Reports a failure in the app's error strip. */
  fail: (message: string) => void;
  setOpen: (open: boolean) => void;
  toggle: () => void;
  close: () => void;
  setMaximized: (value: boolean) => void;
  setFloating: (value: boolean) => void;
  resize: (width: number) => void;
  commitWidth: (width: number) => void;
  resetWidth: () => void;
}

/** The shell and the tabs together, for the panel itself. */
export interface RightPanelValue extends RightPanelShell {
  tabs: RightPanelTabs;
  active: RightPanelTab | null;
  /** The preview tab is on screen, so list rows can be selected. */
  previewActive: boolean;
  preview: PreviewSelection | null;
  publishPreview: (selection: PreviewSelection | null) => void;
  /** The asset screen's preview button: show the preview tab, or hide the panel when it is already shown. */
  togglePreview: () => void;
  openKind: (kind: RightPanelKind) => void;
  select: (id: string) => void;
  closeTabById: (id: string) => void;
  move: (id: string, offset: number) => void;
  arrange: (id: string, index: number) => void;
  /** Open a page tab in the right panel, or a separate window where the panel cannot host one. */
  openPage: () => void;
  detachPage: (tabId: string, x: number, y: number) => void;
}

/** What a product list needs from the panel. It changes only when the preview tab comes or goes. */
export interface RightPanelPreview {
  previewActive: boolean;
  publishPreview: (selection: PreviewSelection | null) => void;
  togglePreview: () => void;
}

const RightPanelContext = createContext<RightPanelValue | null>(null);
const RightPanelPreviewContext = createContext<RightPanelPreview | null>(null);

export function useRightPanel(): RightPanelValue {
  const value = useContext(RightPanelContext);
  if (!value) throw new Error("right panel missing");
  return value;
}

/** For the product lists. Selecting a row or changing tabs does not draw the list again. */
export function useRightPanelPreview(): RightPanelPreview {
  const value = useContext(RightPanelPreviewContext);
  if (!value) throw new Error("right panel missing");
  return value;
}

/** Runs a browser command and keeps its (numbered) result as the window's browser state. */
function browserAction(action: BrowserAction): Promise<void> {
  return api.browserAction(action).then(applyBrowserState);
}

/** Open state, width and the panel shortcut. Lives in the app shell, so it survives page changes. */
export function useRightPanelState(fail: (message: string) => void, gates: RightPanelGates = {}): RightPanelShell {
  const builtinBrowser = gates.builtinBrowser === true;
  const stableGates = useMemo<RightPanelGates>(() => ({ builtinBrowser }), [builtinBrowser]);
  const [open, setOpenState] = useState(false);
  const [maximized, setMaximized] = useState(false);
  const [floating, setFloatingState] = useState(false);
  const [width, setWidth] = useState<number>(PREVIEW_PANE_WIDTH.default);
  const dirty = useRef({ open: false, width: false });
  const openRef = useRef(open);
  openRef.current = open;
  const failRef = useRef(fail);
  failRef.current = fail;
  const failStable = useCallback((message: string) => failRef.current(message), []);
  const report = useCallback((err: unknown) => failRef.current(errorText(err)), []);

  useEffect(() => {
    let alive = true;
    void api.settings().then(
      (settings) => {
        if (!alive) return;
        if (!dirty.current.open) {
          openRef.current = settings.previewPane;
          setOpenState(settings.previewPane);
        }
        if (!dirty.current.width) setWidth(clampStoredPreviewPaneWidth(settings.previewPaneWidth));
      },
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, []);

  const setOpen = useCallback(
    (next: boolean) => {
      dirty.current.open = true;
      openRef.current = next;
      setOpenState(next);
      if (!next) setMaximized(false);
      void api.updateSettings({ previewPane: next }).catch(report);
    },
    [report],
  );
  const toggle = useCallback(() => setOpen(!openRef.current), [setOpen]);
  const close = useCallback(() => setOpen(false), [setOpen]);
  const setFloating = useCallback((next: boolean) => {
    setFloatingState(next);
    if (next) setMaximized(false);
  }, []);

  const publishWidth = useCallback(
    (next: number, persist: boolean) => {
      const stored = clampStoredPreviewPaneWidth(next);
      dirty.current.width = true;
      setWidth(stored);
      if (persist) void api.updateSettings({ previewPaneWidth: stored }).catch(report);
    },
    [report],
  );
  const resize = useCallback((next: number) => publishWidth(next, false), [publishWidth]);
  const commitWidth = useCallback((next: number) => publishWidth(next, true), [publishWidth]);
  const resetWidth = useCallback(() => publishWidth(PREVIEW_PANE_WIDTH.default, true), [publishWidth]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || !isRightPanelShortcut(event)) return;
      event.preventDefault();
      toggle();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggle]);

  return useMemo(
    () => ({
      open,
      maximized,
      floating,
      width,
      gates: stableGates,
      fail: failStable,
      setOpen,
      toggle,
      close,
      setMaximized,
      setFloating,
      resize,
      commitWidth,
      resetWidth,
    }),
    [open, maximized, floating, width, stableGates, failStable, setOpen, toggle, close, setFloating, resize, commitWidth, resetWidth],
  );
}

/**
 * The tabs and the preview selection. Every change goes through `commit`, which keeps the latest
 * tabs in a ref; settings writes and browser commands run after the change, never inside a
 * state updater (React may run an updater twice).
 */
function useRightPanelTabs(shell: RightPanelShell): RightPanelValue {
  const { gates, open, setOpen, fail } = shell;
  const builtinBrowser = gates.builtinBrowser === true;
  const [tabs, setTabs] = useState<RightPanelTabs>(defaultRightPanelTabs);
  const tabsRef = useRef(tabs);
  const [preview, setPreview] = useState<PreviewSelection | null>(null);
  const dirty = useRef(false);
  const openRef = useRef(open);
  openRef.current = open;
  const setOpenRef = useRef(setOpen);
  setOpenRef.current = setOpen;
  const failRef = useRef(fail);
  failRef.current = fail;
  const report = useCallback((err: unknown) => failRef.current(errorText(err)), []);

  const commit = useCallback((next: RightPanelTabs) => {
    if (next === tabsRef.current) return false;
    tabsRef.current = next;
    setTabs(next);
    return true;
  }, []);

  /** A change the user made. Saves the fixed tabs when their kinds, order or selection changed. */
  const saveTabs = useCallback(
    (update: (current: RightPanelTabs) => RightPanelTabs) => {
      dirty.current = true;
      const before = tabsRef.current;
      const after = update(before);
      if (!commit(after)) return { before, after: before };
      const saved = serializeRightPanelTabs(after);
      if (serializeRightPanelTabs(before) !== saved) void api.updateSettings({ rightPanelTabs: saved }).catch(report);
      return { before, after };
    },
    [commit, report],
  );

  useEffect(() => {
    let alive = true;
    void api.settings().then(
      (settings) => {
        if (!alive || dirty.current) return;
        const current = tabsRef.current;
        const pages = current.tabs.flatMap((tab) =>
          tab.kind === "web" && tab.tabId ? [{ tabId: tab.tabId, title: tab.title ?? "ページ" }] : [],
        );
        const restored = mergePanelWebTabs(parseRightPanelTabs(settings.rightPanelTabs), pages);
        const selected = current.tabs.find((tab) => tab.id === current.activeId && tab.kind === "web");
        commit(selected ? selectTab(restored, selected.id) : restored);
      },
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, [commit]);

  const panelCanDock = useRef(true);
  const applyBrowser = useCallback(
    (state: BrowserState) => {
      panelCanDock.current = state.panelSupported;
      const host = state.hosts.find((item) => item.id === "panel");
      const pages = (host?.tabIds ?? []).map((tabId) => ({
        tabId,
        title: state.tabs.find((tab) => tab.id === tabId)?.title || "ページ",
      }));
      commit(mergePanelWebTabs(tabsRef.current, pages));
    },
    [commit],
  );

  useEffect(() => {
    if (!builtinBrowser) {
      commit(mergePanelWebTabs(tabsRef.current, []));
      return;
    }
    let alive = true;
    let stopRequested = () => {};
    // The shared browser state of this window (src/browserStore.ts). Page titles change often;
    // mergePanelWebTabs keeps the same tabs object when nothing on the strip changed.
    const sync = () => {
      const { state } = getBrowserSnapshot();
      if (alive && state) applyBrowser(state);
    };
    const stopState = subscribeBrowserState(sync);
    sync();
    // A page was opened in the panel. Show that tab without closing the others.
    void listenBrowserPanelRequested((tabId) => {
      if (!alive) return;
      if (!openRef.current) setOpenRef.current(true);
      if (tabId) commit(focusWebTab(tabsRef.current, tabId));
    }).then(
      (unlisten) => {
        if (alive) stopRequested = unlisten;
        else unlisten();
      },
      () => undefined,
    );
    return () => {
      alive = false;
      stopState();
      stopRequested();
    };
  }, [applyBrowser, builtinBrowser, commit]);

  /** Tells the app the new page order. Moving a preview or file tab between pages changes nothing there. */
  const syncWebOrder = useCallback(
    (before: RightPanelTabs, after: RightPanelTabs) => {
      if (sameWebOrder(before, after)) return;
      const tabIds = webTabIds(after);
      if (!tabIds.length) return;
      void browserAction({ kind: "reorderTabs", hostId: "panel", tabIds }).catch(report);
    },
    [report],
  );

  const select = useCallback(
    (id: string) => {
      const tab = tabsRef.current.tabs.find((item) => item.id === id);
      saveTabs((current) => selectTab(current, id));
      if (tab?.kind === "web" && tab.tabId) void browserAction({ kind: "selectTab", tabId: tab.tabId }).catch(report);
    },
    [report, saveTabs],
  );

  const closeTabById = useCallback(
    (id: string) => {
      const tab = tabsRef.current.tabs.find((item) => item.id === id);
      const { after } = saveTabs((current) => closeTab(current, id));
      if (tab?.kind !== "web" || !tab.tabId) return;
      const selected = after.tabs.find((item) => item.id === after.activeId);
      void browserAction({ kind: "closeTab", tabId: tab.tabId })
        .then(() => {
          if (selected?.kind === "web" && selected.tabId) return browserAction({ kind: "selectTab", tabId: selected.tabId });
          return undefined;
        })
        .catch(report);
    },
    [report, saveTabs],
  );

  const move = useCallback(
    (id: string, offset: number) => {
      const { before, after } = saveTabs((current) => moveTab(current, id, offset));
      syncWebOrder(before, after);
    },
    [saveTabs, syncWebOrder],
  );

  const arrange = useCallback(
    (id: string, index: number) => {
      const { before, after } = saveTabs((current) => placeTab(current, id, index));
      syncWebOrder(before, after);
    },
    [saveTabs, syncWebOrder],
  );

  const openKind = useCallback(
    (kind: RightPanelKind) => {
      saveTabs((current) => openTab(current, kind));
      if (!openRef.current) setOpenRef.current(true);
    },
    [saveTabs],
  );

  const openPage = useCallback(() => {
    const hostId = panelCanDock.current ? "panel" : undefined;
    void browserAction({ kind: "newTab", hostId })
      .then(() => {
        if (!openRef.current) setOpenRef.current(true);
      })
      .catch(report);
  }, [report]);

  const detachPage = useCallback(
    (tabId: string, x: number, y: number) => {
      void browserAction({ kind: "detachTab", tabId, x, y }).catch(report);
    },
    [report],
  );

  const active = activeTab(tabs, gates);
  const previewActive = open && active?.kind === "preview";

  const togglePreview = useCallback(() => {
    if (previewActive) setOpenRef.current(false);
    else openKind("preview");
  }, [openKind, previewActive]);

  const publishPreview = useCallback((selection: PreviewSelection | null) => {
    setPreview((current) => {
      if (current === selection) return current;
      if (current && selection && current.item === selection.item && current.clear === selection.clear) return current;
      return selection;
    });
  }, []);

  return useMemo(
    () => ({
      ...shell,
      tabs,
      active,
      previewActive,
      preview,
      publishPreview,
      togglePreview,
      openKind,
      select,
      closeTabById,
      move,
      arrange,
      openPage,
      detachPage,
    }),
    [shell, tabs, active, previewActive, preview, publishPreview, togglePreview, openKind, select, closeTabById, move, arrange, openPage, detachPage],
  );
}

/**
 * Owns the tabs and gives them to the panel. `children` are drawn by the app shell, so a tab
 * change here draws only the components that read the panel, not the pages.
 */
export function RightPanelProvider({ value: shell, children }: { value: RightPanelShell; children?: ReactNode }) {
  const panel = useRightPanelTabs(shell);
  const { previewActive, publishPreview, togglePreview } = panel;
  const preview = useMemo<RightPanelPreview>(
    () => ({ previewActive, publishPreview, togglePreview }),
    [previewActive, publishPreview, togglePreview],
  );
  return (
    <RightPanelContext.Provider value={panel}>
      <RightPanelPreviewContext.Provider value={preview}>{children}</RightPanelPreviewContext.Provider>
    </RightPanelContext.Provider>
  );
}

function ExpandGlyph({ expanded }: { expanded: boolean }) {
  return expanded ? (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M4.5 1.5v3h-3M7.5 1.5v3h3M4.5 10.5v-3h-3M7.5 10.5v-3h3" />
    </svg>
  ) : (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M1.5 4.5v-3h3M10.5 4.5v-3h-3M1.5 7.5v3h3M10.5 7.5v3h-3" />
    </svg>
  );
}

function GripGlyph() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M4 2.5h.01M8 2.5h.01M4 6h.01M8 6h.01M4 9.5h.01M8 9.5h.01" strokeWidth="2.2" />
    </svg>
  );
}

/** A window over a frame: float the panel. */
function PopOutGlyph() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M1.5 4.5v6h6M4 1.5h6.5v6.5H4z" />
    </svg>
  );
}

/** A frame with its right column filled: put the panel back on the right. */
function DockGlyph() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M1.5 1.5h9v9h-9zM7.5 1.5v9" />
    </svg>
  );
}

function TabGlyph({ kind }: { kind: RightPanelTab["kind"] }) {
  if (kind === "web") return <GlobeGlyph />;
  if (kind === "files") return <FolderGlyph />;
  return <PictureGlyph />;
}

/** A window whose right column is the panel: the launcher's main mark. */
function PanelGlyph() {
  return (
    <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden="true">
      <rect x="2" y="3" width="14" height="12" rx="2.5" />
      <path d="M11 3v12" />
      <path d="M12.8 6.5h1.4M12.8 9h1.4" />
    </svg>
  );
}

/**
 * While the right panel is closed, a button in the top-right corner of the page opens it again.
 * The panel mark says what it opens; the small number in its corner is how many tabs it holds.
 * `panel` is the app shell's handle (open and toggle); the tabs come from `RightPanelProvider`.
 */
export function RightPanelLauncher({ panel }: { panel: RightPanelShell }) {
  const { tabs, gates } = useRightPanel();
  if (panel.open) return null;
  const count = visibleTabs(tabs, gates).length;
  return (
    <button
      className="right-panel-launcher"
      type="button"
      aria-label={`右パネルを開く（タブ ${count} 件）`}
      title={`右パネルを開く（${RIGHT_PANEL_SHORTCUT_LABEL}）`}
      aria-keyshortcuts="Control+Shift+B"
      onClick={panel.toggle}
    >
      <PanelGlyph />
      {count > 0 && (
        <span className="right-panel-launcher-count" aria-hidden="true">
          {launcherCountLabel(tabs, gates)}
        </span>
      )}
    </button>
  );
}

function AddTabMenu({ panel }: { panel: RightPanelValue }) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const kinds = RIGHT_PANEL_KINDS.filter((kind) => kindAvailable(kind, panel.gates));

  useEffect(() => {
    if (!open) return;
    rootRef.current?.querySelector<HTMLButtonElement>("[role='menuitem']")?.focus();
    const onPointer = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      setOpen(false);
      rootRef.current?.querySelector<HTMLButtonElement>(".right-panel-add")?.focus();
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  return (
    <div className="right-panel-menu" ref={rootRef}>
      <button
        className="right-panel-icon right-panel-add"
        type="button"
        aria-label="タブを追加"
        title="タブを追加"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        <PlusGlyph />
      </button>
      {open && (
        <div className="account-popover right-panel-popover" role="menu" aria-label="追加するタブ">
          {kinds.map((kind) => {
            const exists = panel.tabs.tabs.some((tab) => tab.kind === kind);
            const info = RIGHT_PANEL_KIND_INFO[kind];
            return (
              <button
                key={kind}
                type="button"
                role="menuitem"
                disabled={!canAddTab(panel.tabs, kind, panel.gates)}
                onClick={() => {
                  setOpen(false);
                  panel.openKind(kind);
                }}
              >
                {info.label}
                {info.singleton && exists && <span className="meta">表示中</span>}
              </button>
            );
          })}
          {pageTabsAllowed(panel.gates) && (
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                setOpen(false);
                panel.openPage();
              }}
            >
              ブラウザ
            </button>
          )}
        </div>
      )}
    </div>
  );
}

function PanelHeader({
  panel,
  floating,
  dockable = false,
  onDismiss,
  start,
}: {
  panel: RightPanelValue;
  floating: boolean;
  /** A floating panel can go back to the right column (the window is wide enough). */
  dockable?: boolean;
  onDismiss: () => void;
  /** Controls before the tab strip (the floating panel's move grip). */
  start?: ReactNode;
}) {
  const shown = visibleTabs(panel.tabs, panel.gates);
  const activeId = panel.active?.id ?? null;
  const listRef = useRef<HTMLDivElement>(null);
  // あふれたタブ列のどちら側に続きがあるか。端をぼかして示す。
  const [edges, setEdges] = useState({ start: false, end: false });
  const drag = useRef<{
    id: string;
    tabId?: string;
    detachable: boolean;
    startX: number;
    startY: number;
    moved: boolean;
    cancelled: boolean;
    pointerId: number;
    insertAt: number | null;
    /** The strip's left edge on screen and each tab in its scrolled content, measured once when the press becomes a drag. */
    origin: number;
    edges: { left: number; width: number }[];
  } | null>(null);
  // Where the 2px insert line goes. It is drawn over the strip and does not move the tabs.
  const [insert, setInsert] = useState<{ index: number; left: number } | null>(null);

  // タブは最低幅を持つので、狭いパネルでは横にあふれる。あふれた分は横スクロールで出し、
  // ホイールでも動かせるようにする。選んだタブは帯の中に見える位置へ寄せる。
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const measure = () => {
      const next = overflowEdges(true, list);
      setEdges((current) => (current.start === next.start && current.end === next.end ? current : next));
    };
    const onWheel = (event: WheelEvent) => {
      const delta = horizontalWheelDelta(event, list);
      if (delta === null) return;
      event.preventDefault();
      list.scrollLeft += delta;
    };
    measure();
    list.addEventListener("scroll", measure, { passive: true });
    list.addEventListener("wheel", onWheel, { passive: false });
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(measure) : null;
    observer?.observe(list);
    window.addEventListener("resize", measure);
    return () => {
      list.removeEventListener("scroll", measure);
      list.removeEventListener("wheel", onWheel);
      observer?.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [shown.length]);

  useEffect(() => {
    const list = listRef.current;
    const tab = activeId ? [...(list?.querySelectorAll<HTMLElement>(".right-panel-tab") ?? [])].find((node) => node.dataset.tabId === activeId) : null;
    if (!list || !tab) return;
    const box = list.getBoundingClientRect();
    const rect = tab.getBoundingClientRect();
    const next = revealScrollLeft({ left: rect.left - box.left + list.scrollLeft, width: rect.width }, list);
    if (next !== null) list.scrollLeft = next;
    setEdges(overflowEdges(true, list));
  }, [activeId, shown.length]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || !drag.current) return;
      // A press that has not become a drag yet leaves Escape to the rest of the app.
      if (!drag.current.moved) {
        drag.current = null;
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      drag.current.cancelled = true;
      drag.current = null;
      setInsert(null);
      document.body.classList.remove("tab-dragging");
    };
    // A native dragend is not a drop outside the window.
    const onDragEnd = () => {
      dragOutcome({ moved: true, cancelled: false, insideWindow: false, detachable: true, dragEndOnly: true });
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("dragend", onDragEnd);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("dragend", onDragEnd);
    };
  }, []);

  function clearDrag() {
    drag.current = null;
    setInsert(null);
    document.body.classList.remove("tab-dragging");
  }

  function onPointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    const target = event.target;
    if (!(target instanceof Element) || target.closest(".right-panel-tab-close")) return;
    const row = target.closest<HTMLElement>(".right-panel-tab");
    const id = row?.dataset.tabId;
    const tab = shown.find((item) => item.id === id);
    if (!row || !tab) return;
    drag.current = {
      id: tab.id,
      tabId: tab.kind === "web" ? tab.tabId : undefined,
      detachable: tab.kind === "web" && Boolean(tab.tabId),
      startX: event.clientX,
      startY: event.clientY,
      moved: false,
      cancelled: false,
      pointerId: event.pointerId,
      insertAt: null,
      origin: 0,
      edges: [],
    };
    // Capture only once the press becomes a drag. Chromium (WebView2) sends the click of a
    // captured press to the capturing tab list instead of the tab button, so capturing here
    // would stop plain clicks from selecting tabs.
  }

  function capture(pointerId: number) {
    try {
      listRef.current?.setPointerCapture?.(pointerId);
    } catch {
      // The pointer is already released; pointerup clears the drag.
    }
  }

  /**
   * The press became a drag. Everything a move needs is measured here once: a move then reads
   * no layout, and the body class (which hides the native page) is added once, not per move.
   */
  function startDrag(current: NonNullable<typeof drag.current>, pointerId: number) {
    capture(pointerId);
    current.moved = true;
    const list = listRef.current;
    if (list) {
      const box = list.getBoundingClientRect();
      current.origin = box.left;
      current.edges = [...list.querySelectorAll<HTMLElement>(".right-panel-tab")].map((node) => {
        const rect = node.getBoundingClientRect();
        return { left: rect.left - box.left + list.scrollLeft, width: rect.width };
      });
    }
    document.body.classList.add("tab-dragging");
  }

  function showInsert(current: NonNullable<typeof drag.current>, at: number | null) {
    if (current.insertAt === at) return;
    current.insertAt = at;
    setInsert(at === null ? null : { index: at, left: insertMarkerLeft(at, current.edges) });
  }

  function onPointerLeave(event: ReactPointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || current.moved || event.pointerId !== current.pointerId) return;
    // Leaving the strip while pressed is a drag even below the threshold; keep following it.
    if (event.buttons & 1) startDrag(current, event.pointerId);
    else clearDrag();
  }

  function onPointerMove(event: ReactPointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || event.pointerId !== current.pointerId) return;
    if (!current.moved && Math.hypot(event.clientX - current.startX, event.clientY - current.startY) < TAB_DRAG_THRESHOLD_PX) return;
    if (!current.moved) startDrag(current, event.pointerId);
    if (!pointerInsideWindow(event.clientX, event.clientY, window.innerWidth, window.innerHeight)) {
      showInsert(current, null);
      return;
    }
    const scroll = listRef.current?.scrollLeft ?? 0;
    showInsert(current, insertionIndex(event.clientX - current.origin + scroll, current.edges));
  }

  function onPointerUp(event: ReactPointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || event.pointerId !== current.pointerId) return;
    const outcome = dragOutcome({
      moved: current.moved,
      cancelled: current.cancelled,
      insideWindow: pointerInsideWindow(event.clientX, event.clientY, window.innerWidth, window.innerHeight),
      detachable: current.detachable,
    });
    const at = current.insertAt;
    const tabId = current.tabId;
    const id = current.id;
    clearDrag();
    if (outcome === "reorder" && at != null) {
      const target = shown[at];
      const index = target ? panel.tabs.tabs.findIndex((item) => item.id === target.id) : panel.tabs.tabs.length;
      panel.arrange(id, index < 0 ? panel.tabs.tabs.length : index);
    } else if (outcome === "detach" && tabId) {
      panel.detachPage(tabId, event.screenX, event.screenY);
    }
  }

  return (
    <div className="right-panel-head">
      {start}
      <div
        className="right-panel-tabs"
        role="tablist"
        aria-label="右パネルのタブ"
        ref={listRef}
        data-more-start={edges.start ? "" : undefined}
        data-more-end={edges.end ? "" : undefined}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerLeave={onPointerLeave}
        onPointerCancel={clearDrag}
        onLostPointerCapture={() => {
          if (!drag.current?.moved) return;
          clearDrag();
        }}
      >
        {shown.map((tab, index) => {
          const label = tabLabel(tab);
          const selected = tab.id === activeId;
          return (
            <div className={selected ? "right-panel-tab active" : "right-panel-tab"} data-tab-id={tab.id} key={tab.id}>
              <button
                type="button"
                role="tab"
                id={`right-panel-tab-${tab.id}`}
                aria-selected={selected}
                aria-controls="right-panel-body"
                tabIndex={selected ? 0 : -1}
                title={label}
                onClick={() => panel.select(tab.id)}
                onKeyDown={(event) => {
                  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
                  event.preventDefault();
                  event.stopPropagation();
                  const offset = event.key === "ArrowRight" ? 1 : -1;
                  if (event.altKey) {
                    panel.move(tab.id, offset);
                    return;
                  }
                  const next = shown[(index + offset + shown.length) % shown.length];
                  if (!next) return;
                  panel.select(next.id);
                  window.setTimeout(() => document.getElementById(`right-panel-tab-${next.id}`)?.focus(), 0);
                }}
              >
                <span className="right-panel-tab-icon" aria-hidden="true">
                  <TabGlyph kind={tab.kind} />
                </span>
                <span className="right-panel-tab-label">{label}</span>
              </button>
              <button
                className="right-panel-tab-close"
                type="button"
                aria-label={`${label}のタブを閉じる`}
                title="タブを閉じる"
                onClick={() => panel.closeTabById(tab.id)}
              >
                <CloseGlyph />
              </button>
            </div>
          );
        })}
        {insert && <span className="right-panel-tab-insert" aria-hidden="true" style={{ left: insert.left }} />}
      </div>
      <AddTabMenu panel={panel} />
      <div className="right-panel-actions">
        {floating ? (
          dockable && (
            <button
              className="right-panel-icon"
              type="button"
              aria-label="右側に戻す"
              title="右側に戻す"
              onClick={() => panel.setFloating(false)}
            >
              <DockGlyph />
            </button>
          )
        ) : (
          <button
            className="right-panel-icon"
            type="button"
            aria-label="ウィンドウとして浮かせる"
            title="ウィンドウとして浮かせる"
            onClick={() => panel.setFloating(true)}
          >
            <PopOutGlyph />
          </button>
        )}
        {!floating && (
          <button
            className="right-panel-icon"
            type="button"
            aria-pressed={panel.maximized}
            aria-label={panel.maximized ? "全面表示をやめる" : "全面表示"}
            title={panel.maximized ? "全面表示をやめる" : "全面表示"}
            onClick={() => panel.setMaximized(!panel.maximized)}
          >
            <ExpandGlyph expanded={panel.maximized} />
          </button>
        )}
        <button
          className="right-panel-icon right-panel-close"
          type="button"
          aria-label="右パネルを閉じる"
          title={`閉じる（${RIGHT_PANEL_SHORTCUT_LABEL}）`}
          onClick={onDismiss}
        >
          <CloseGlyph />
        </button>
      </div>
    </div>
  );
}

function PanelBody({ panel, floating }: { panel: RightPanelValue; floating: boolean }) {
  const active = panel.active;
  if (active?.kind === "web") {
    return (
      <div
        className="right-panel-body right-panel-body-web"
        id="right-panel-body"
        role="tabpanel"
        aria-labelledby={`right-panel-tab-${active.id}`}
      >
        <BrowserPanelTab panel={panel} unified />
      </div>
    );
  }
  if (!active) {
    return (
      <div className="right-panel-body" id="right-panel-body">
        <div className="right-panel-empty">
          <p className="meta">タブがありません。</p>
          <button className="button" type="button" onClick={() => panel.openKind("preview")}>
            プレビューを開く
          </button>
        </div>
      </div>
    );
  }
  const Render = RIGHT_PANEL_RENDERERS[active.kind];
  return (
    <div
      className={`right-panel-body right-panel-body-${active.kind}`}
      id="right-panel-body"
      role="tabpanel"
      aria-labelledby={`right-panel-tab-${active.id}`}
    >
      {Render ? <Render panel={panel} floating={floating} /> : null}
    </div>
  );
}

/** Skipped while the floating panel moves or resizes: only its frame changes then. */
const PanelSurface = memo(function PanelSurface({
  panel,
  floating,
  dockable = false,
  onDismiss,
  headStart,
}: {
  panel: RightPanelValue;
  floating: boolean;
  dockable?: boolean;
  onDismiss: () => void;
  headStart?: ReactNode;
}) {
  return (
    <aside
      className={floating ? "right-panel floating" : panel.maximized ? "right-panel maximized" : "right-panel"}
      id="right-panel"
      aria-label="右パネル"
    >
      <PanelHeader panel={panel} floating={floating} dockable={dockable} onDismiss={onDismiss} start={headStart} />
      <PanelBody panel={panel} floating={floating} />
    </aside>
  );
});

/** The viewport of the app window, or a common size where there is none to measure (DOM stubs). */
function viewportSize() {
  const width = typeof window !== "undefined" ? window.innerWidth : NaN;
  const height = typeof window !== "undefined" ? window.innerHeight : NaN;
  return {
    width: Number.isFinite(width) && width > 0 ? width : 1280,
    height: Number.isFinite(height) && height > 0 ? height : 800,
  };
}

/** Where the user put the floating panel. Kept while the app runs, not saved (null: top-right). */
let floatPlacement: FloatRect | null = null;

/** Head parts that keep their own pointer handling instead of moving the panel. */
const FLOAT_HEAD_CONTROLS = "button, a, input, select, textarea, [role='menu'], .right-panel-tab";

type FloatGesture = { kind: "move" | FloatEdge; pointerId: number; startX: number; startY: number; start: FloatRect };

/**
 * The right panel as a small window over the page inside the app window: always in a narrow
 * window, and by choice in a wide one. It moves by the empty part of its tab strip or the grip,
 * resizes from its sides and lower corners, and leaves the list usable. Docked pages show inside
 * it and hide while it moves.
 */
function FloatingPanel({ panel, dockable }: { panel: RightPanelValue; dockable: boolean }) {
  const [view, setView] = useState(viewportSize);
  const [placed, setPlaced] = useState<FloatRect | null>(floatPlacement);
  const [live, setLive] = useState<FloatRect | null>(null);
  const gesture = useRef<FloatGesture | null>(null);
  // 何も選んでいないプレビューは、これから選ぶ一覧を覆わない小さな案内にする。
  const compact = panel.active?.kind === "preview" && !panel.preview?.item;
  const rect = compact ? compactFloatRect(view.width, view.height) : live ?? placeFloatRect(placed, view.width, view.height);
  const rectRef = useRef(rect);
  rectRef.current = rect;

  useEffect(() => {
    const onResize = () => setView(viewportSize());
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      document.body.classList.remove(FLOAT_PANEL_MOVING_CLASS);
    };
  }, []);

  // A docked page is placed from the measured viewport; a move without a size change is only
  // seen by its safety timer, so tell it right away.
  useEffect(() => {
    window.dispatchEvent(new Event(FLOAT_PANEL_MOVED_EVENT));
  }, [rect.left, rect.top, rect.width, rect.height]);

  const settle = useCallback((next: FloatRect | null) => {
    floatPlacement = next;
    setPlaced(next);
    setLive(null);
  }, []);

  function begin(kind: FloatGesture["kind"], event: ReactPointerEvent<HTMLElement>) {
    if (event.button !== 0 || gesture.current || compact) return;
    event.preventDefault();
    event.stopPropagation();
    gesture.current = { kind, pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, start: rectRef.current };
    try {
      event.currentTarget.setPointerCapture?.(event.pointerId);
    } catch {
      // The pointer is already up; pointerup ends the gesture.
    }
    document.body.classList.add(FLOAT_PANEL_MOVING_CLASS);
  }

  function follow(event: ReactPointerEvent<HTMLElement>) {
    const current = gesture.current;
    if (!current || event.pointerId !== current.pointerId) return;
    const dx = event.clientX - current.startX;
    const dy = event.clientY - current.startY;
    const size = viewportSize();
    setLive(
      current.kind === "move"
        ? moveFloatRect(current.start, dx, dy, size.width, size.height)
        : resizeFloatRect(current.start, current.kind, dx, dy, size.width, size.height),
    );
  }

  function finish(event: ReactPointerEvent<HTMLElement>) {
    const current = gesture.current;
    if (!current || event.pointerId !== current.pointerId) return;
    gesture.current = null;
    document.body.classList.remove(FLOAT_PANEL_MOVING_CLASS);
    // A press without a move keeps the default top-right placement.
    if (live) settle(rectRef.current);
  }

  function onHeadPointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    const target = event.target;
    if (!(target instanceof Element) || !target.closest(".right-panel-head") || target.closest(FLOAT_HEAD_CONTROLS)) return;
    begin("move", event);
  }

  function onHeadDoubleClick(event: ReactMouseEvent<HTMLElement>) {
    if (compact) return;
    const target = event.target;
    if (!(target instanceof Element) || !target.closest(".right-panel-head")) return;
    if (target.closest(FLOAT_HEAD_CONTROLS) && !target.closest(".right-panel-float-grip")) return;
    settle(null);
  }

  function onGripKey(event: ReactKeyboardEvent<HTMLButtonElement>) {
    const offsets: Record<string, [number, number]> = {
      ArrowLeft: [-FLOAT_PANEL_STEP, 0],
      ArrowRight: [FLOAT_PANEL_STEP, 0],
      ArrowUp: [0, -FLOAT_PANEL_STEP],
      ArrowDown: [0, FLOAT_PANEL_STEP],
    };
    if (event.key === "Home") {
      event.preventDefault();
      settle(null);
      return;
    }
    const offset = offsets[event.key];
    if (!offset) return;
    event.preventDefault();
    const size = viewportSize();
    const [dx, dy] = offset;
    settle(
      event.shiftKey
        ? resizeFloatRect(rectRef.current, "se", dx, dy, size.width, size.height)
        : moveFloatRect(rectRef.current, dx, dy, size.width, size.height),
    );
  }

  // The same grip element on every move keeps PanelSurface (memo) from drawing the tabs again.
  const handlers = useRef({ begin, onGripKey });
  handlers.current = { begin, onGripKey };
  const grip = useMemo(
    () => (
      <button
        className="right-panel-icon right-panel-float-grip"
        type="button"
        aria-label="右パネルを移動"
        title="ドラッグで移動。矢印キーで移動、Shift+矢印キーで大きさを変更、ダブルクリックか Home で右上に戻す"
        onPointerDown={(event) => handlers.current.begin("move", event)}
        onKeyDown={(event) => handlers.current.onGripKey(event)}
      >
        <GripGlyph />
      </button>
    ),
    [],
  );

  if (typeof document === "undefined") return null;
  return createPortal(
    <div
      className={["right-panel-float", live ? "moving" : "", compact ? "compact" : ""].filter(Boolean).join(" ")}
      style={{ left: rect.left, top: rect.top, width: rect.width, height: rect.height }}
      data-floating-panel=""
      onPointerDown={onHeadPointerDown}
      onPointerMove={follow}
      onPointerUp={finish}
      onPointerCancel={finish}
      onLostPointerCapture={finish}
      onDoubleClick={onHeadDoubleClick}
    >
      <PanelSurface panel={panel} floating dockable={dockable} onDismiss={panel.close} headStart={compact ? undefined : grip} />
      {!compact && FLOAT_PANEL_EDGES.map((edge) => (
        <div
          key={edge}
          className={`right-panel-float-edge edge-${edge}`}
          data-edge={edge}
          aria-hidden="true"
          onPointerDown={(event) => begin(edge, event)}
        />
      ))}
    </div>,
    document.body,
  );
}

/**
 * Right column of the app grid. Wide windows dock it with a resize handle unless the user floats
 * it; narrow windows always float it over the page as a movable window. The list stays usable.
 * `panel` is the app shell's handle; the tabs come from `RightPanelProvider`.
 */
export function RightPanelColumn({
  panel: shell,
  docked,
  canDock = false,
  width,
  maxWidth,
}: {
  panel: RightPanelShell;
  docked: boolean;
  /** The window is wide enough for the column, so a floating panel offers to go back. */
  canDock?: boolean;
  width: number;
  maxWidth: number;
}) {
  const panel = useRightPanel();
  if (!shell.open) return null;
  if (!docked) return <FloatingPanel panel={panel} dockable={canDock} />;
  return (
    <>
      {!panel.maximized && (
        <ResizeSeparator
          label="右パネルの幅"
          value={width}
          min={PREVIEW_PANE_WIDTH.min}
          max={maxWidth}
          grows="left"
          onChange={shell.resize}
          onCommit={shell.commitWidth}
          onReset={shell.resetWidth}
        />
      )}
      <PanelSurface panel={panel} floating={false} onDismiss={panel.close} />
    </>
  );
}
