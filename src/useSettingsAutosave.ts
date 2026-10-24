import { useEffect, useRef, useState } from "react";
import { api } from "./api";
import { createSettingsAutosave } from "./settingsAutosave";
import type { Settings } from "./types";

export function useSettingsAutosave(
  onApplied: (settings: Settings, patch: Partial<Settings>) => void,
) {
  const applied = useRef(onApplied);
  applied.current = onApplied;
  const controller = useRef<ReturnType<typeof createSettingsAutosave> | null>(
    null,
  );
  if (!controller.current)
    controller.current = createSettingsAutosave(
      (patch) => api.updateSettings(patch),
      (settings, patch) => applied.current(settings, patch),
    );
  const current = controller.current;
  const [state, setState] = useState(current.snapshot);
  useEffect(() => {
    const unsubscribe = current.subscribe(setState);
    return () => {
      unsubscribe();
      void current.flush().catch(() => {});
    };
  }, [current]);
  return {
    ...state,
    hydrate: current.hydrate,
    ticket: current.ticket,
    change: current.change,
    flush: current.flush,
  };
}
