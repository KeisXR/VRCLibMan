import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { formatBytes } from "../media";
import type { OllamaLoadedModel } from "../types";
import { errorText } from "../ui";

function modelMatches(selected: string, loaded: OllamaLoadedModel): boolean {
  return (
    loaded.name === selected
    || loaded.name.startsWith(`${selected}:`)
    || selected.startsWith(`${loaded.name}:`)
  );
}

export function OllamaMemoryControls({
  model,
  enabled,
  onStatus,
}: {
  model: string;
  enabled: boolean;
  onStatus?: (message: string) => void;
}) {
  const [loaded, setLoaded] = useState<OllamaLoadedModel[] | null>(null);
  const [pending, setPending] = useState(false);

  const refresh = useCallback(() => {
    if (!enabled || !model.trim()) {
      setLoaded([]);
      return;
    }
    void api
      .ollamaLoadedModels()
      .then(setLoaded)
      .catch(() => setLoaded([]));
  }, [enabled, model]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  if (!enabled) return null;

  const active = loaded?.find((item) => modelMatches(model, item));
  const stateLabel =
    loaded === null ? "未確認" : active ? "メモリ上" : "未読み込み";

  async function unload() {
    setPending(true);
    try {
      await api.unloadOllamaModel(model);
      onStatus?.("モデルをアンロードしました");
      refresh();
    } catch (err) {
      onStatus?.(errorText(err));
    } finally {
      setPending(false);
    }
  }

  return (
    <div className="ollama-memory">
      <p className="meta" role="status">
        読み込み状態: {stateLabel}
        {active && active.sizeBytes > 0 ? `（${formatBytes(active.sizeBytes)}）` : ""}
      </p>
      <p className="meta">同じ PC のほかのアプリが同じモデルを使っている場合も影響します。</p>
      <button className="button" type="button" disabled={pending || !model.trim()} onClick={() => void unload()}>
        モデルをアンロード
      </button>
    </div>
  );
}
