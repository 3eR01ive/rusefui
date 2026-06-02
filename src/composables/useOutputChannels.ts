import { shallowRef, readonly, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type OutputValuesSource = "live" | "logCursor";

export interface OutputSnapshot {
  connected: boolean;
  pollHz: number;
  rawLen: number;
  values: Record<string, number>;
  iniFieldCount?: number;
  lastError?: string | null;
  /** CSV-лог output channels для текущей сессии ECU. */
  sessionLogPath?: string | null;
  /** elapsed_sec головы timeline (та же ось, что CSV и query_view). */
  timelineLiveSec?: number;
  /** live ECU или интерполяция по логу в курсоре. */
  valuesSource?: OutputValuesSource;
  /** Момент на оси лога для `values` (сек). */
  sampleSec?: number | null;
}

const snapshot = shallowRef<OutputSnapshot>({
  connected: false,
  pollHz: 0,
  rawLen: 0,
  values: {},
});

let unlisten: UnlistenFn | null = null;
let initPromise: Promise<void> | null = null;

export async function initOutputChannels(): Promise<void> {
  if (initPromise) return initPromise;

  initPromise = (async () => {
    try {
      snapshot.value = await invoke<OutputSnapshot>("output_get_snapshot");
    } catch {
      /* not in tauri yet */
    }

    await invoke("output_start_listener").catch(() => {});

    if (!unlisten) {
      unlisten = await listen<OutputSnapshot>("output-channels", (event) => {
        snapshot.value = event.payload;
      });
    }
  })();

  return initPromise;
}

/** Текущие output-параметры: live с ECU или срез лога в курсоре (см. `valuesSource`). */
export function useOutputChannels() {
  return {
    snapshot: readonly(snapshot),
    valuesSource: computed(
      () => snapshot.value.valuesSource ?? (snapshot.value.connected ? "live" : "logCursor"),
    ),
    sampleSec: computed(() => snapshot.value.sampleSec ?? null),
    getField: (name: string): number | null => {
      const v = snapshot.value.values[name];
      return v === undefined ? null : v;
    },
  };
}

export const useCurrentOutputValues = useOutputChannels;
