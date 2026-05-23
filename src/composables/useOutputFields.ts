import { ref, readonly } from "vue";
import { invoke } from "@tauri-apps/api/core";

export interface OutputFieldInfo {
  name: string;
  units?: string | null;
  kind: string;
}

const fields = ref<OutputFieldInfo[]>([]);
let loadPromise: Promise<void> | null = null;

export async function loadOutputFields(): Promise<void> {
  if (loadPromise) return loadPromise;

  loadPromise = (async () => {
    try {
      fields.value = await invoke<OutputFieldInfo[]>("output_list_fields");
    } catch {
      fields.value = [];
    }
  })();

  return loadPromise;
}

export function useOutputFields() {
  return {
    fields: readonly(fields),
    reload: loadOutputFields,
  };
}
