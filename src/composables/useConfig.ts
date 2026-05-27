import { shallowRef, readonly } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isConfigMergeBlocking } from "./useConfigDiff";
import { workspaceSnapshot } from "./useWorkspaceState";

export interface ConfigSnapshot {
  connected: boolean;
  loaded: boolean;
  /** Снимок из файла проекта (редактирование offline, не live ECU). */
  readOnly?: boolean;
  loading: boolean;
  progress: number;
  bytesLoaded: number;
  bytesTotal: number;
  rawLen: number;
  values: Record<string, number>;
  stringValues?: Record<string, string>;
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
  arrayCols?: number | null;
  arrayRows?: number | null;
  arrayLength?: number | null;
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
      unlisten = await listen<ConfigSnapshot>("config-snapshot", async (event) => {
        snapshot.value = event.payload;
        if (event.payload.loaded || event.payload.fieldCount > 0) {
          try {
            const fields = await invoke<ConfigFieldInfo[]>("config_list_fields");
            fieldsByName.value = new Map(fields.map((f) => [f.name, f]));
          } catch {
            /* ignore */
          }
        }
      });
      await listen<{
        loading: boolean;
        progress: number;
        bytesLoaded: number;
        bytesTotal: number;
      }>("config-progress", (event) => {
        const p = event.payload;
        snapshot.value = {
          ...snapshot.value,
          loading: p.loading,
          progress: p.progress,
          bytesLoaded: p.bytesLoaded,
          bytesTotal: p.bytesTotal,
        };
      });
      await listen("workspace-reset", async () => {
        try {
          snapshot.value = await invoke<ConfigSnapshot>("config_get_snapshot");
          const fields = await invoke<ConfigFieldInfo[]>("config_list_fields");
          fieldsByName.value = new Map(fields.map((f) => [f.name, f]));
        } catch {
          /* ignore */
        }
      });
    }
  })();

  return initPromise;
}

export async function refreshConfigSnapshot(): Promise<void> {
  try {
    snapshot.value = await invoke<ConfigSnapshot>("config_get_snapshot");
    const fields = await invoke<ConfigFieldInfo[]>("config_list_fields");
    fieldsByName.value = new Map(fields.map((f) => [f.name, f]));
  } catch {
    /* not in tauri */
  }
}

export async function setConfigScalar(
  field: string,
  value: number,
): Promise<void> {
  snapshot.value = await invoke<ConfigSnapshot>("config_set_scalar", {
    params: { field, value },
  });
}

export async function setConfigString(
  field: string,
  value: string,
): Promise<void> {
  snapshot.value = await invoke<ConfigSnapshot>("config_set_string", {
    params: { field, value },
  });
}

export async function getConfigArray(field: string): Promise<number[]> {
  return invoke<number[]>("config_get_array", { params: { field } });
}

export async function setConfigArrayValue(
  field: string,
  index: number,
  value: number,
): Promise<void> {
  snapshot.value = await invoke<ConfigSnapshot>("config_set_array_value", {
    params: { field, index, value },
  });
}

/** Запись текущего конфига page 0 во flash (команда `B`). */
export async function burnConfig(): Promise<void> {
  await invoke("config_burn");
}

export function configCanView(s: ConfigSnapshot): boolean {
  return s.loaded && !s.loading;
}

export function configCanEdit(s: ConfigSnapshot): boolean {
  if (isConfigMergeBlocking()) return false;
  if (!s.loaded || s.loading) return false;
  const caps = workspaceSnapshot.value.capabilities;
  if (caps.editProjectConfig) return true;
  if (caps.writeConfigToEcu) return true;
  return false;
}

export function configIsProjectMode(s: ConfigSnapshot): boolean {
  return Boolean(s.loaded && s.readOnly);
}

export function useConfig() {
  return {
    snapshot: readonly(snapshot),
    configCanView,
    configCanEdit,
    configIsProjectMode,
    getField: (name: string): number | null => {
      const v = snapshot.value.values[name];
      return v === undefined ? null : v;
    },
    getStringField: (name: string): string | null => {
      const v = snapshot.value.stringValues?.[name];
      return v === undefined ? null : v;
    },
    getFieldInfo: (name: string): ConfigFieldInfo | null =>
      fieldsByName.value.get(name) ?? null,
    setField: setConfigScalar,
    setStringField: setConfigString,
    burn: burnConfig,
    getArray: getConfigArray,
    setArrayValue: setConfigArrayValue,
  };
}
