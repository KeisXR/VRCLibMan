import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import { api } from "../api";
import { PREVIEW_PANE_WIDTH, clampStoredPreviewPaneWidth } from "../previewPane";
import {
  RIGHT_PANEL_KINDS,
  RIGHT_PANEL_KIND_INFO,
  RIGHT_PANEL_SHORTCUT_LABEL,
  activeTab,
  canAddTab,
  closeTab,
  defaultRightPanelTabs,
  isRightPanelShortcut,
  moveTab,
  openTab,
  parseRightPanelTabs,
  selectTab,
  serializeRightPanelTabs,
  visibleTabs,
} from "../rightPanel";
import type { RightPanelKind, RightPanelTab, RightPanelTabs } from "../rightPanel";
import type { ItemView } from "../types";
import { errorText } from "../ui";
import { ResizeSeparator } from "./ResizeSeparator";
import { RIGHT_PANEL_RENDERERS } from "./rightPanelRegistry";

/** What the asset or world list currently selects for the preview tab. */
export interface PreviewSelection {
  item: ItemView | null;
  clear: () => void;
}

export interface RightPanelValue {
  open: boolean;
  maximized: boolean;
  width: number;
  tabs: RightPanelTabs;
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
  setMaximized: (value: boolean) => void;
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
export function useRightPanelState(fail: (message: string) => void): RightPanelValue {
  const [open, setOpen] = useState(false);
  const [maximized, setMaximized] = useState(false);
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
        if (!dirty.current.tabs) setTabs(parseRightPanelTabs(settings.rightPanelTabs));
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

  const active = activeTab(tabs);
  const previewActive = open && active?.kind === "preview";

  const toggle = useCallback(() => saveOpen(!open), [open, saveOpen]);

  const openKind = useCallback(
    (kind: RightPanelKind) => {
      saveTabs((current) => openTab(current, kind));
      if (!open) saveOpen(true);
    },
    [open, saveOpen, saveTabs],
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
      width,
      tabs,
      active,
      previewActive,
      preview,
      publishPreview,
      toggle,
      close: () => saveOpen(false),
      togglePreview,
      openKind,
      select: (id: string) => saveTabs((current) => selectTab(current, id)),
      closeTabById: (id: string) => saveTabs((current) => closeTab(current, id)),
      move: (id: string, offset: number) => saveTabs((current) => moveTab(current, id, offset)),
      setMaximized,
      resize: (next: number) => publishWidth(next, false),
      commitWidth: (next: number) => publishWidth(next, true),
      resetWidth: () => publishWidth(PREVIEW_PANE_WIDTH.default, true),
    }),
    [open, maximized, width, tabs, active, previewActive, preview, publishPreview, toggle, saveOpen, togglePreview, openKind, saveTabs, publishWidth],
  );
}

function CloseGlyph() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <path d="M1.5 1.5 8.5 8.5M8.5 1.5 1.5 8.5" />
    </svg>
  );
}

function PlusGlyph() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M6 1.5v9M1.5 6h9" />
    </svg>
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

function AddTabMenu({ panel }: { panel: RightPanelValue }) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const kinds = RIGHT_PANEL_KINDS.filter((kind) => RIGHT_PANEL_KIND_INFO[kind].available);

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
                disabled={!canAddTab(panel.tabs, kind)}
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
        </div>
      )}
    </div>
  );
}

function PanelHeader({ panel, overlay, onDismiss }: { panel: RightPanelValue; overlay: boolean; onDismiss: () => void }) {
  const shown = visibleTabs(panel.tabs);
  const activeId = panel.active?.id ?? null;
  return (
    <div className="right-panel-head">
      <div className="right-panel-tabs" role="tablist" aria-label="右パネルのタブ">
        {shown.map((tab, index) => {
          const info = RIGHT_PANEL_KIND_INFO[tab.kind];
          const selected = tab.id === activeId;
          return (
            <div className={selected ? "right-panel-tab active" : "right-panel-tab"} key={tab.id}>
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
                {info.label}
              </button>
              <button
                className="right-panel-tab-close"
                type="button"
                aria-label={`${info.label}のタブを閉じる`}
                title="タブを閉じる"
                onClick={() => panel.closeTabById(tab.id)}
              >
                <CloseGlyph />
              </button>
            </div>
          );
        })}
      </div>
      <AddTabMenu panel={panel} />
      <div className="right-panel-actions">
        {!overlay && (
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

function PanelBody({ panel }: { panel: RightPanelValue }) {
  const active = panel.active;
  if (!active) {
    return (
      <div className="right-panel-body" id="right-panel-body">
        <div className="right-panel-empty">
          <p className="meta">タブがありません。＋から追加できます。</p>
          <button className="button" type="button" onClick={() => panel.openKind("preview")}>
            プレビューを開く
          </button>
        </div>
      </div>
    );
  }
  const Render = RIGHT_PANEL_RENDERERS[active.kind];
  return (
    <div className="right-panel-body" id="right-panel-body" role="tabpanel" aria-labelledby={`right-panel-tab-${active.id}`}>
      {Render ? <Render panel={panel} /> : null}
    </div>
  );
}

function PanelSurface({ panel, overlay, onDismiss }: { panel: RightPanelValue; overlay: boolean; onDismiss: () => void }) {
  return (
    <aside
      className={`right-panel${overlay ? " overlay" : ""}${panel.maximized ? " maximized" : ""}`}
      id="right-panel"
      aria-label="右パネル"
    >
      <PanelHeader panel={panel} overlay={overlay} onDismiss={onDismiss} />
      <PanelBody panel={panel} />
    </aside>
  );
}

function PanelOverlay({ children, onDismiss }: { children: ReactNode; onDismiss: () => void }) {
  useEffect(() => {
    document.querySelector<HTMLButtonElement>(".preview-overlay .right-panel-close")?.focus();
  }, []);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (document.querySelector(".product-actions-panel")) return;
      event.preventDefault();
      onDismiss();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onDismiss]);
  if (typeof document === "undefined") return null;
  return createPortal(
    <div
      className="preview-overlay right-panel-overlay"
      role="dialog"
      aria-modal="true"
      aria-label="右パネル"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onDismiss();
      }}
    >
      {children}
    </div>,
    document.body,
  );
}

/**
 * Right column of the app grid. Wide windows dock it with a resize handle; narrow windows show
 * it over the page like the earlier preview overlay, only once there is something to show.
 */
export function RightPanelColumn({
  panel,
  docked,
  width,
  maxWidth,
}: {
  panel: RightPanelValue;
  docked: boolean;
  width: number;
  maxWidth: number;
}) {
  if (!panel.open) return null;
  if (!docked) {
    const selection = panel.preview;
    const waitingForSelection = panel.active?.kind === "preview" && !selection?.item;
    if (waitingForSelection) return null;
    const dismiss = () => {
      if (panel.active?.kind === "preview" && selection?.item) selection.clear();
      else panel.close();
    };
    return (
      <PanelOverlay onDismiss={dismiss}>
        <PanelSurface panel={panel} overlay onDismiss={dismiss} />
      </PanelOverlay>
    );
  }
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
      <PanelSurface panel={panel} overlay={false} onDismiss={panel.close} />
    </>
  );
}
