import { shallowRef, readonly } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface CompositeEvent {
  tUs: number;
  pri: boolean;
  sec: boolean;
  trg: boolean;
  sync: boolean;
  coil: boolean;
  inj: boolean;
}

export interface CompositeSnapshot {
  connected: boolean;
  loggingEnabled: boolean;
  polling: boolean;
  events: CompositeEvent[];
  totalEvents: number;
  lastBatch: number;
  lastError?: string | null;
  rpm?: number | null;
}

const snapshot = shallowRef<CompositeSnapshot>({
  connected: false,
  loggingEnabled: false,
  polling: false,
  events: [],
  totalEvents: 0,
  lastBatch: 0,
});

let unlisten: UnlistenFn | null = null;
let initPromise: Promise<void> | null = null;

/** Подписка на события; опрос ECU не стартует — только `composite_set_enabled`. */
export async function initCompositeLogger(): Promise<void> {
  if (initPromise) return initPromise;

  initPromise = (async () => {
    try {
      snapshot.value = await invoke<CompositeSnapshot>("composite_get_snapshot");
    } catch {
      /* not in tauri */
    }

    if (!unlisten) {
      unlisten = await listen<CompositeSnapshot>("composite-logger", (event) => {
        snapshot.value = event.payload;
      });
    }
  })();

  return initPromise;
}

export async function setCompositeLoggingEnabled(
  enabled: boolean,
): Promise<CompositeSnapshot> {
  const snap = await invoke<CompositeSnapshot>("composite_set_enabled", { enabled });
  snapshot.value = snap;
  return snap;
}

export function useCompositeLogger() {
  return {
    snapshot: readonly(snapshot),
    setLoggingEnabled: setCompositeLoggingEnabled,
  };
}
