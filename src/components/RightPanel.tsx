import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent, ReactNode } from "react";
import { createPortal } from "react-dom";
import { api, listenBrowserChanged, listenBrowserPanelRequested } from "../api";
import {
  FLOAT_PANEL_EDGES,
  FLOAT_PANEL_MOVED_EVENT,
  FLOAT_PANEL_MOVING_CLASS,
  FLOAT_PANEL_STEP,
  moveFloatRect,
  placeFloatRect,
  resizeFloatRect,
} from "../floatingPanel";
import type { FloatEdge, FloatRect } from "../floatingPanel";
import { dragOutcome, insertionIndex, pointerInsideWindow, TAB_DRAG_THRESHOLD_PX } from "../tabStrip";
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
  selectTab,
  serializeRightPanelTabs,
  tabLabel,
  visibleTabs,
  webTabIds,
} from "../rightPanel";
import type { RightPanelGates, RightPanelKind, RightPanelTab, RightPanelTabs } from "../rightPanel";
import type { BrowserState } from "../types";
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

export interface RightPanelValue {
  open: boolean;
  maximized: boolean;
  /** The user chose to float the panel as a window in a wide window too. Not saved, like maximized. */
  floating: boolean;
  width: number;
  tabs: RightPanelTabs;
  /** Settings that show or hide tab kinds (the browser needs the built-in browser). */
  gates: RightPanelGates;
  active: RightPanelTab | null;
  /** The preview tab is on screen, so list rows can be selected. */
  previewActive: boolean;
  preview: PreviewSelection | null;
  publishPreview: (selection: PreviewSelection | null) => void;
  toggle: () => void;
  close: () => void;
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
  setMaximized: (value: boolean) => void;
  setFloating: (value: boolean) => void;
  resize: (width: number) => void;
  commitWidth: (width: number) => void;
  resetWidth: () => void;
}

const RightPanelContext = createContext<RightPanelValue | null>(null);

export const RightPanelProvider = RightPanelContext.Provider;

export function useRightPanel(): RightPanelValue {
  const value = useContext(RightPanelContext);
  if (!value) throw new Error("right panel missing");
  return value;
}

/** Panel state lives in the app shell, so it survives page changes. */
export function useRightPanelState(fail: (message: string) => void, gates: RightPanelGates = {}): RightPanelValue {
  const builtinBrowser = gates.builtinBrowser === true;
  const stableGates = useMemo<RightPanelGates>(() => ({ builtinBrowser }), [builtinBrowser]);
  const [open, setOpen] = useState(false);
  const [maximized, setMaximized] = useState(false);
  const [floating, setFloatingState] = useState(false);
  const [width, setWidth] = useState<number>(PREVIEW_PANE_WIDTH.default);
  const [tabs, setTabs] = useState<RightPanelTabs>(defaultRightPanelTabs);
  const [preview, setPreview] = useState<PreviewSelection | null>(null);
  const dirty = useRef({ open: false, width: false, tabs: false });
  const failRef = useRef(fail);
  failRef.current = fail;

  useEffect(() => {
    let alive = true;
    void api.settings().then(
      (settings) => {
        if (!alive) return;
        if (!dirty.current.open) setOpen(settings.previewPane);
        if (!dirty.current.width) setWidth(clampStoredPreviewPaneWidth(settings.previewPaneWidth));
        setTabs((current) => {
          if (dirty.current.tabs) return current;
          const pages = current.tabs.flatMap((tab) =>
            tab.kind === "web" && tab.tabId ? [{ tabId: tab.tabId, title: tab.title ?? "ページ" }] : [],
          );
          const restored = mergePanelWebTabs(parseRightPanelTabs(settings.rightPanelTabs), pages);
          const selected = current.tabs.find((tab) => tab.id === current.activeId && tab.kind === "web");
          return selected ? selectTab(restored, selected.id) : restored;
        });
      },
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, []);

  const report = useCallback((err: unknown) => failRef.current(errorText(err)), []);

  const saveOpen = useCallback(
    (next: boolean) => {
      dirty.current.open = true;
      setOpen(next);
      if (!next) setMaximized(false);
      void api.updateSettings({ previewPane: next }).catch(report);
    },
    [report],
  );

  const saveTabs = useCallback(
    (update: (current: RightPanelTabs) => RightPanelTabs) => {
      dirty.current.tabs = true;
      setTabs((current) => {
        const next = update(current);
        if (next === current) return current;
        const before = serializeRightPanelTabs(current);
        const after = serializeRightPanelTabs(next);
        if (before !== after) void api.updateSettings({ rightPanelTabs: after }).catch(report);
        return next;
      });
    },
    [report],
  );

  const publishWidth = useCallback(
    (next: number, persist: boolean) => {
      const stored = clampStoredPreviewPaneWidth(next);
      dirty.current.width = true;
      setWidth(stored);
      if (persist) void api.updateSettings({ previewPaneWidth: stored }).catch(report);
    },
    [report],
  );

  const active = activeTab(tabs, stableGates);
  const previewActive = open && active?.kind === "preview";

  const toggle = useCallback(() => saveOpen(!open), [open, saveOpen]);

  const openKind = useCallback(
    (kind: RightPanelKind) => {
      saveTabs((current) => openTab(current, kind));
      if (!open) saveOpen(true);
    },
    [open, saveOpen, saveTabs],
  );

  const openRef = useRef(open);
  openRef.current = open;
  const saveOpenRef = useRef(saveOpen);
  saveOpenRef.current = saveOpen;

  const panelCanDock = useRef(true);
  const applyBrowser = useCallback((state: BrowserState) => {
    panelCanDock.current = state.panelSupported;
    const host = state.hosts.find((item) => item.id === "panel");
    const pages = (host?.tabIds ?? []).map((tabId) => ({
      tabId,
      title: state.tabs.find((tab) => tab.id === tabId)?.title || "ページ",
    }));
    setTabs((current) => mergePanelWebTabs(current, pages));
  }, []);

  useEffect(() => {
    if (!builtinBrowser) {
      setTabs((current) => mergePanelWebTabs(current, []));
      return;
    }
    let alive = true;
    let stopChanged = () => {};
    let stopRequested = () => {};
    const load = () => {
      void api.browserState().then(
        (state) => {
          if (alive) applyBrowser(state);
        },
        () => undefined,
      );
    };
    load();
    void listenBrowserChanged(() => {
      if (alive) load();
    }).then(
      (unlisten) => {
        if (alive) stopChanged = unlisten;
        else unlisten();
      },
      () => undefined,
    );
    // A page was opened in the panel. Show that tab without closing the others.
    void listenBrowserPanelRequested((tabId) => {
      if (!openRef.current) saveOpenRef.current(true);
      if (tabId) setTabs((current) => focusWebTab(current, tabId));
    }).then(
      (unlisten) => {
        if (alive) stopRequested = unlisten;
        else unlisten();
      },
      () => undefined,
    );
    return () => {
      alive = false;
      stopChanged();
      stopRequested();
    };
  }, [applyBrowser, builtinBrowser]);

  const syncWebOrder = useCallback(
    (next: RightPanelTabs) => {
      const tabIds = webTabIds(next);
      if (!tabIds.length) return;
      void api.browserAction({ kind: "reorderTabs", hostId: "panel", tabIds }).catch(report);
    },
    [report],
  );

  const closeTabById = useCallback(
    (id: string) => {
      saveTabs((current) => {
        const tab = current.tabs.find((item) => item.id === id);
        const next = closeTab(current, id);
        if (tab?.kind === "web" && tab.tabId) {
          const selected = next.tabs.find((item) => item.id === next.activeId);
          void api
            .browserAction({ kind: "closeTab", tabId: tab.tabId })
            .then(() => {
              if (selected?.kind === "web" && selected.tabId) {
                return api.browserAction({ kind: "selectTab", tabId: selected.tabId });
              }
              return undefined;
            })
            .catch(report);
        }
        return next;
      });
    },
    [report, saveTabs],
  );

  const togglePreview = useCallback(() => {
    if (previewActive) saveOpen(false);
    else openKind("preview");
  }, [openKind, previewActive, saveOpen]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || !isRightPanelShortcut(event)) return;
      event.preventDefault();
      toggle();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggle]);

  const publishPreview = useCallback((selection: PreviewSelection | null) => {
    setPreview((current) => {
      if (current === selection) return current;
      if (current && selection && current.item === selection.item && current.clear === selection.clear) return current;
      return selection;
    });
  }, []);

  return useMemo(
    () => ({
      open,
      maximized,
      floating,
      width,
      tabs,
      gates: stableGates,
      active,
      previewActive,
      preview,
      publishPreview,
      toggle,
      close: () => saveOpen(false),
      togglePreview,
      openKind,
      select: (id: string) =>
        saveTabs((current) => {
          const next = selectTab(current, id);
          const tab = current.tabs.find((item) => item.id === id);
          if (tab?.kind === "web" && tab.tabId) {
            void api.browserAction({ kind: "selectTab", tabId: tab.tabId }).catch(report);
          }
          return next;
        }),
      closeTabById,
      move: (id: string, offset: number) =>
        saveTabs((current) => {
          const next = moveTab(current, id, offset);
          if (next !== current) syncWebOrder(next);
          return next;
        }),
      arrange: (id: string, index: number) =>
        saveTabs((current) => {
          const next = placeTab(current, id, index);
          if (next !== current) syncWebOrder(next);
          return next;
        }),
      openPage: () => {
        const hostId = panelCanDock.current ? "panel" : undefined;
        void api
          .browserAction({ kind: "newTab", hostId })
          .then(() => {
            if (!openRef.current) saveOpenRef.current(true);
          })
          .catch(report);
      },
      detachPage: (tabId: string, x: number, y: number) => {
        void api.browserAction({ kind: "detachTab", tabId, x, y }).catch(report);
      },
      setMaximized,
      setFloating: (next: boolean) => {
        setFloatingState(next);
        if (next) setMaximized(false);
      },
      resize: (next: number) => publishWidth(next, false),
      commitWidth: (next: number) => publishWidth(next, true),
      resetWidth: () => publishWidth(PREVIEW_PANE_WIDTH.default, true),
    }),
    [open, maximized, floating, width, tabs, stableGates, active, previewActive, preview, publishPreview, toggle, saveOpen, togglePreview, openKind, closeTabById, saveTabs, publishWidth, syncWebOrder, report],
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

/**
 * While the right panel is closed, a small button in the top-right corner of the page shows how
 * many tabs it holds and opens it again (like the ChatGPT app's side panel).
 */
export function RightPanelLauncher({ panel }: { panel: RightPanelValue }) {
  if (panel.open) return null;
  const count = visibleTabs(panel.tabs, panel.gates).length;
  return (
    <button
      className="right-panel-launcher"
      type="button"
      aria-label={`右パネルを開く（タブ ${count} 件）`}
      title={`右パネルを開く（${RIGHT_PANEL_SHORTCUT_LABEL}）`}
      aria-keyshortcuts="Control+Shift+B"
      onClick={panel.toggle}
    >
      <span className="right-panel-launcher-count" aria-hidden="true">
        {launcherCountLabel(panel.tabs, panel.gates)}
      </span>
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
  } | null>(null);
  const [insertAt, setInsertAt] = useState<number | null>(null);

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
      setInsertAt(null);
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
    setInsertAt(null);
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

  function onPointerLeave(event: ReactPointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || current.moved || event.pointerId !== current.pointerId) return;
    // Leaving the strip while pressed is a drag even below the threshold; keep following it.
    if (event.buttons & 1) {
      current.moved = true;
      document.body.classList.add("tab-dragging");
      capture(event.pointerId);
    } else {
      clearDrag();
    }
  }

  function onPointerMove(event: ReactPointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || event.pointerId !== current.pointerId) return;
    if (!current.moved && Math.hypot(event.clientX - current.startX, event.clientY - current.startY) < TAB_DRAG_THRESHOLD_PX) return;
    if (!current.moved) capture(event.pointerId);
    current.moved = true;
    document.body.classList.add("tab-dragging");
    const inside = pointerInsideWindow(event.clientX, event.clientY, window.innerWidth, window.innerHeight);
    if (!inside) {
      current.insertAt = null;
      setInsertAt(null);
      return;
    }
    const rects = [...(listRef.current?.querySelectorAll<HTMLElement>(".right-panel-tab") ?? [])].map((node) => {
      const box = node.getBoundingClientRect();
      return { left: box.left, width: box.width };
    });
    const at = insertionIndex(event.clientX, rects);
    current.insertAt = at;
    setInsertAt(at);
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
            <div className="right-panel-tab-slot" key={tab.id} style={{ display: "contents" }}>
              {insertAt === index && <span className="right-panel-tab-insert" aria-hidden="true" />}
              <div className={selected ? "right-panel-tab active" : "right-panel-tab"} data-tab-id={tab.id}>
              <button
                type="button"
                role="tab"
                id={`right-panel-tab-${tab.id}`}
                aria-selected={selected}
                aria-controls="right-panel-body"
                tabIndex={selected ? 0 : -1}
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
            </div>
          );
        })}
        {insertAt === shown.length && <span className="right-panel-tab-insert" aria-hidden="true" />}
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

function PanelSurface({
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
}

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
  const rect = live ?? placeFloatRect(placed, view.width, view.height);
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
    if (event.button !== 0 || gesture.current) return;
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

  const grip = (
    <button
      className="right-panel-icon right-panel-float-grip"
      type="button"
      aria-label="右パネルを移動"
      title="ドラッグで移動。矢印キーで移動、Shift+矢印キーで大きさを変更、ダブルクリックか Home で右上に戻す"
      onPointerDown={(event) => begin("move", event)}
      onKeyDown={onGripKey}
    >
      <GripGlyph />
    </button>
  );

  if (typeof document === "undefined") return null;
  return createPortal(
    <div
      className={live ? "right-panel-float moving" : "right-panel-float"}
      style={{ left: rect.left, top: rect.top, width: rect.width, height: rect.height }}
      data-floating-panel=""
      onPointerDown={onHeadPointerDown}
      onPointerMove={follow}
      onPointerUp={finish}
      onPointerCancel={finish}
      onLostPointerCapture={finish}
      onDoubleClick={onHeadDoubleClick}
    >
      <PanelSurface panel={panel} floating dockable={dockable} onDismiss={panel.close} headStart={grip} />
      {FLOAT_PANEL_EDGES.map((edge) => (
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
 */
export function RightPanelColumn({
  panel,
  docked,
  canDock = false,
  width,
  maxWidth,
}: {
  panel: RightPanelValue;
  docked: boolean;
  /** The window is wide enough for the column, so a floating panel offers to go back. */
  canDock?: boolean;
  width: number;
  maxWidth: number;
}) {
  if (!panel.open) return null;
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
          onChange={panel.resize}
          onCommit={panel.commitWidth}
          onReset={panel.resetWidth}
        />
      )}
      <PanelSurface panel={panel} floating={false} onDismiss={panel.close} />
    </>
  );
}
