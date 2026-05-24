import { shallowRef, readonly } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface ConfigSnapshot {
  connected: boolean;
  loaded: boolean;
  loading: boolean;
  progress: number;
  bytesLoaded: number;
  bytesTotal: number;
  rawLen: number;
  values: Record<string, number>;
  fieldCount: number;
  lastError?: string | null;
}

export interface ConfigEnumOption {
  value: number;
  label: string;
}

export interface ConfigFieldInfo {
  name: string;
  units?: string | null;
  ty: string;
  options?: ConfigEnumOption[] | null;
}

const snapshot = shallowRef<ConfigSnapshot>({
  connected: false,
  loaded: false,
  loading: false,
  progress: 0,
  bytesLoaded: 0,
  bytesTotal: 0,
  rawLen: 0,
  values: {},
  fieldCount: 0,
});

const fieldsByName = shallowRef<Map<string, ConfigFieldInfo>>(new Map());

let unlisten: UnlistenFn | null = null;
let initPromise: Promise<void> | null = null;

export async function initConfig(): Promise<void> {
  if (initPromise) return initPromise;

  initPromise = (async () => {
    try {
      snapshot.value = await invoke<ConfigSnapshot>("config_get_snapshot");
      const fields = await invoke<ConfigFieldInfo[]>("config_list_fields");
      fieldsByName.value = new Map(fields.map((f) => [f.name, f]));
    } catch {
      /* not in tauri yet */
    }

    await invoke("config_start_listener").catch(() => {});

    if (!unlisten) {
      unlisten = await listen<ConfigSnapshot>("config-snapshot", (event) => {
        snapshot.value = event.payload;
      });
    }
  })();

  return initPromise;
}

export async function setConfigScalar(
  field: string,
  value: number,
): Promise<void> {
  snapshot.value = await invoke<ConfigSnapshot>("config_set_scalar", {
    params: { field, value },
  });
}

export function useConfig() {
  return {
    snapshot: readonly(snapshot),
    getField: (name: string): number | null => {
      const v = snapshot.value.values[name];
      return v === undefined ? null : v;
    },
    getFieldInfo: (name: string): ConfigFieldInfo | null =>
      fieldsByName.value.get(name) ?? null,
    setField: setConfigScalar,
  };
}
