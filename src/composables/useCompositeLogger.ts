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
  /** Номер TDC с начала сессии (только на фронте `trg`). */
  tdcCycle?: number | null;
}

export interface CompositeSnapshot {
  connected: boolean;
  loggingEnabled: boolean;
  polling: boolean;
  events: CompositeEvent[];
  totalEvents: number;
  lastBatch: number;
  recordedSpanMs: number;
  lastChunkGapMs: number;
  chunksReceived: number;
  tdcCyclesTotal: number;
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
  recordedSpanMs: 0,
  lastChunkGapMs: 0,
  chunksReceived: 0,
  tdcCyclesTotal: 0,
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

export async function setCompositeMaxWindowMs(maxWindowMs: number): Promise<void> {
  try {
    await invoke("composite_set_max_window_ms", { maxWindowMs });
  } catch {
    /* not in tauri */
  }
}

export function useCompositeLogger() {
  return {
    snapshot: readonly(snapshot),
    setLoggingEnabled: setCompositeLoggingEnabled,
    setMaxWindowMs: setCompositeMaxWindowMs,
  };
}
