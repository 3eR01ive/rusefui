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
  polling: boolean;
  events: CompositeEvent[];
  totalEvents: number;
  lastBatch: number;
  lastError?: string | null;
  rpm?: number | null;
}

const snapshot = shallowRef<CompositeSnapshot>({
  connected: false,
  polling: false,
  events: [],
  totalEvents: 0,
  lastBatch: 0,
});

let unlisten: UnlistenFn | null = null;
let initPromise: Promise<void> | null = null;

function mapEvent(raw: {
  tUs: number;
  pri: boolean;
  sec: boolean;
  trg: boolean;
  sync: boolean;
  coil: boolean;
  inj: boolean;
}): CompositeEvent {
  return {
    tUs: raw.tUs,
    pri: raw.pri,
    sec: raw.sec,
    trg: raw.trg,
    sync: raw.sync,
    coil: raw.coil,
    inj: raw.inj,
  };
}

function mapSnapshot(raw: CompositeSnapshot): CompositeSnapshot {
  return {
    connected: raw.connected,
    polling: raw.polling,
    events: (raw.events ?? []).map(mapEvent),
    totalEvents: raw.totalEvents ?? 0,
    lastBatch: raw.lastBatch ?? 0,
    lastError: raw.lastError ?? null,
    rpm: raw.rpm ?? null,
  };
}

export async function initCompositeLogger(): Promise<void> {
  if (initPromise) return initPromise;

  initPromise = (async () => {
    try {
      const snap = await invoke<CompositeSnapshot>("composite_get_snapshot");
      snapshot.value = mapSnapshot(snap);
    } catch {
      /* not in tauri */
    }

    await invoke("composite_start_listener").catch(() => {});

    if (!unlisten) {
      unlisten = await listen<CompositeSnapshot>("composite-logger", (event) => {
        snapshot.value = mapSnapshot(event.payload);
      });
    }
  })();

  return initPromise;
}

export function useCompositeLogger() {
  return {
    snapshot: readonly(snapshot),
  };
}
