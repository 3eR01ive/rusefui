import { shallowRef, readonly } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Одно событие логического анализатора (engine sniffer). */
export interface SnifferEvent {
  tUs: number;
  /** Имя канала (`t1`, `inj1`, …) или `r` для TDC. */
  name: string;
  /** Фронт вверх (`true`) / вниз (`false`). */
  up: boolean;
  /** TDC-маркер. */
  tdc: boolean;
  /** RPM из TDC-события. */
  rpm?: number | null;
}

/** Группа канала для визуальной группировки. */
export type SnifferGroup = "trigger" | "ignition" | "injector" | "other";

export interface SnifferChannel {
  name: string;
  group: SnifferGroup;
}

export interface EngineSnifferSnapshot {
  connected: boolean;
  polling: boolean;
  /** Каналы, сгруппированные: триггеры → зажигание → форсунки → прочее. */
  channels: SnifferChannel[];
  events: SnifferEvent[];
  frameSpanUs: number;
  framesReceived: number;
  rpm?: number | null;
  lastError?: string | null;
}

function emptySnapshot(): EngineSnifferSnapshot {
  return {
    connected: false,
    polling: false,
    channels: [],
    events: [],
    frameSpanUs: 0,
    framesReceived: 0,
  };
}

const snapshot = shallowRef<EngineSnifferSnapshot>(emptySnapshot());

let unlisten: UnlistenFn | null = null;
let initPromise: Promise<void> | null = null;

/** Подписка на события; опрос ECU стартует только через `setEngineSnifferEnabled`. */
export async function initEngineSniffer(): Promise<void> {
  if (initPromise) return initPromise;

  initPromise = (async () => {
    try {
      snapshot.value = await invoke<EngineSnifferSnapshot>("engine_sniffer_get_snapshot");
    } catch {
      /* not in tauri */
    }

    if (!unlisten) {
      unlisten = await listen<EngineSnifferSnapshot>("engine-sniffer", (event) => {
        snapshot.value = event.payload;
      });
    }
  })();

  return initPromise;
}

export async function setEngineSnifferEnabled(
  enabled: boolean,
): Promise<EngineSnifferSnapshot> {
  const snap = await invoke<EngineSnifferSnapshot>("engine_sniffer_set_enabled", {
    enabled,
  });
  snapshot.value = snap;
  return snap;
}

export function useEngineSniffer() {
  return {
    snapshot: readonly(snapshot),
    setEnabled: setEngineSnifferEnabled,
  };
}
