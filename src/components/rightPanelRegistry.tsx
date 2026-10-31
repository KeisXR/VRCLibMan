import { useEffect } from "react";
import type { ComponentType } from "react";
import type { RightPanelKind } from "../rightPanel";
import type { RightPanelValue } from "./RightPanel";
import { BrowserPanelTab } from "./BrowserPanel";
import { PreviewPane, useLargePreview } from "./PreviewPane";

/** `overlay`: the panel shows over the page in a narrow window instead of as a column. */
export type RightPanelRenderer = ComponentType<{ panel: RightPanelValue; overlay: boolean }>;

function PreviewTab({ panel }: { panel: RightPanelValue }) {
  const item = panel.preview?.item ?? null;
  const preview = useLargePreview(item, item != null);
  useEffect(() => {
    if (!item) return;
    document.getElementById("right-panel-body")?.scrollTo({ top: 0 });
  }, [item]);
  return (
    <PreviewPane
      item={item}
      largePath={preview.largePath}
      phase={preview.phase}
      onReveal={preview.reveal}
      emptyText={
        panel.preview
          ? "商品を選ぶと、ここに画像を表示します。"
          : "アセットかワールドの一覧で商品を選ぶと、ここに画像を表示します。"
      }
    />
  );
}

/**
 * One renderer per tab kind. The file tab is added by a later change; until then its kind
 * stays hidden (`available: false` in `src/rightPanel.ts`).
 */
export const RIGHT_PANEL_RENDERERS: Partial<Record<RightPanelKind, RightPanelRenderer>> = {
  preview: PreviewTab,
  browser: BrowserPanelTab,
};
