import { shallowRef, readonly } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface KnockScopeSnapshot {
  connected: boolean;
  scopeEnabled: boolean;
  polling: boolean;
  captureCount: number;
  sampleCount: number;
  samples: number[];
  sampleMin: number;
  sampleMax: number;
  lastByteLen: number;
  sampleRateHz: number;
  bufferDurationMs: number;
  lastError?: string | null;
}

const emptySnapshot = (): KnockScopeSnapshot => ({
  connected: false,
  scopeEnabled: false,
  polling: false,
  captureCount: 0,
  sampleCount: 0,
  samples: [],
  sampleMin: 0,
  sampleMax: 0,
  lastByteLen: 0,
  sampleRateHz: 218_750,
  bufferDurationMs: 0,
});

const snapshot = shallowRef<KnockScopeSnapshot>(emptySnapshot());

let unlisten: UnlistenFn | null = null;
let initPromise: Promise<void> | null = null;

/** Подписка на `knock-scope`; опрос ECU — только через `knock_scope_set_enabled`. */
export async function initKnockScope(): Promise<void> {
  if (initPromise) return initPromise;

  initPromise = (async () => {
    try {
      snapshot.value = await invoke<KnockScopeSnapshot>("knock_scope_get_snapshot");
    } catch {
      /* not in tauri */
    }

    if (!unlisten) {
      unlisten = await listen<KnockScopeSnapshot>("knock-scope", (event) => {
        snapshot.value = event.payload;
      });
    }
  })();

  return initPromise;
}

export async function setKnockScopeEnabled(
  enabled: boolean,
): Promise<KnockScopeSnapshot> {
  const snap = await invoke<KnockScopeSnapshot>("knock_scope_set_enabled", {
    enabled,
  });
  snapshot.value = snap;
  return snap;
}

export function useKnockScope() {
  return {
    snapshot: readonly(snapshot),
    setScopeEnabled: setKnockScopeEnabled,
  };
}
