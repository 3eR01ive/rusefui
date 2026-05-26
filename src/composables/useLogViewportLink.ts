import { invoke } from "@tauri-apps/api/core";
import { ref, readonly } from "vue";

const linked = ref(false);
let loaded = false;

async function ensureLoaded(): Promise<void> {
  if (loaded) return;
  try {
    linked.value = await invoke<boolean>("log_viewport_get_linked");
  } catch {
    linked.value = false;
  }
  loaded = true;
}

export async function setLogViewportLinked(value: boolean): Promise<boolean> {
  try {
    linked.value = await invoke<boolean>("log_viewport_set_linked", { linked: value });
  } catch {
    linked.value = value;
  }
  loaded = true;
  return linked.value;
}

export function useLogViewportLink() {
  void ensureLoaded();
  return {
    linked: readonly(linked),
    setLinked: setLogViewportLinked,
  };
}
