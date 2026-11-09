import type { Settings } from "./types";

export interface SettingsSaveState {
  settings: Settings | null;
  phase: "loading" | "saved" | "saving" | "error";
  error: string | null;
}

/** Serialize patches; a response to an older change cannot overwrite a newer draft. */
export function createSettingsAutosave(
  save: (patch: Partial<Settings>) => Promise<Settings>,
  applied: (settings: Settings, patch: Partial<Settings>) => void = () => {},
) {
  let state: SettingsSaveState = {
    settings: null,
    phase: "loading",
    error: null,
  };
  let dirty: Partial<Settings> = {};
  let generation = 0;
  let queued = false;
  let running: Promise<void> | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const listeners = new Set<(value: SettingsSaveState) => void>();
  const publish = () => {
    for (const listener of listeners) listener(state);
  };
  const clearTimer = () => {
    if (timer != null) clearTimeout(timer);
    timer = null;
  };

  function drain(): Promise<void> {
    if (running) return running;
    running = (async () => {
      while (queued) {
        queued = false;
        const patch = { ...dirty };
        if (Object.keys(patch).length === 0) continue;
        let saved: Settings;
        try {
          saved = await save(patch);
        } catch (err) {
          state = {
            ...state,
            phase: "error",
            error: err instanceof Error ? err.message : String(err),
          };
          publish();
          continue;
        }
        for (const key of Object.keys(patch) as (keyof Settings)[]) {
          if (Object.is(dirty[key], patch[key])) delete dirty[key];
        }
        generation++;
        state = {
          settings: { ...saved, ...dirty },
          phase: Object.keys(dirty).length ? "saving" : "saved",
          error: null,
        };
        publish();
        applied(saved, patch);
      }
    })().finally(() => {
      running = null;
      if (queued) void drain();
    });
    return running;
  }

  return {
    snapshot: () => state,
    ticket: () => generation,
    subscribe(listener: (value: SettingsSaveState) => void) {
      listeners.add(listener);
      listener(state);
      return () => {
        listeners.delete(listener);
      };
    },
    hydrate(settings: Settings, ticket: number) {
      if (ticket !== generation || running) return;
      state = {
        ...state,
        settings: { ...settings, ...dirty },
        phase: Object.keys(dirty).length ? state.phase : "saved",
      };
      publish();
    },
    change(patch: Partial<Settings>, delayMs = 0) {
      if (!state.settings) return;
      generation++;
      dirty = { ...dirty, ...patch };
      queued = true;
      state = {
        settings: { ...state.settings, ...patch },
        phase: "saving",
        error: null,
      };
      publish();
      clearTimer();
      if (delayMs > 0 && !running)
        timer = setTimeout(() => {
          timer = null;
          void drain();
        }, delayMs);
      else void drain();
    },
    async flush(): Promise<Settings | null> {
      clearTimer();
      if (Object.keys(dirty).length) queued = true;
      await drain();
      if (running) await running;
      if (state.error) throw new Error(state.error);
      return state.settings;
    },
  };
}
