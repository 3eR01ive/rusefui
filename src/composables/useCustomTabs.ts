import { ref, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";

const UI_KEY = "custom_tabs";

export interface CustomTabDef {
  id: string;
  title: string;
}

const _defs = ref<CustomTabDef[]>([]);
let _nextId = 1;

function generateId(): string {
  while (_defs.value.some((t) => t.id === `custom_${_nextId}`)) _nextId++;
  return `custom_${_nextId++}`;
}

async function persist(): Promise<void> {
  await invoke("project_ui_set", { key: UI_KEY, value: _defs.value }).catch(() => {});
}

export async function loadCustomTabs(): Promise<void> {
  try {
    const raw = await invoke<CustomTabDef[] | null>("project_ui_get", { key: UI_KEY });
    _defs.value = Array.isArray(raw) ? raw : [];
    _nextId = 1;
    for (const t of _defs.value) {
      const m = t.id.match(/^custom_(\d+)$/);
      if (m) _nextId = Math.max(_nextId, parseInt(m[1]!) + 1);
    }
  } catch {
    _defs.value = [];
  }
}

export function useCustomTabs() {
  const customTabDefs = computed(() => _defs.value);

  async function addCustomTab(): Promise<string> {
    const id = generateId();
    _defs.value = [..._defs.value, { id, title: "Новый таб" }];
    await persist();
    return id;
  }

  async function removeCustomTab(id: string): Promise<void> {
    _defs.value = _defs.value.filter((t) => t.id !== id);
    await persist();
  }

  async function renameCustomTab(id: string, title: string): Promise<void> {
    _defs.value = _defs.value.map((t) => (t.id === id ? { ...t, title } : t));
    await persist();
  }

  function isCustomTab(id: string): boolean {
    return _defs.value.some((t) => t.id === id);
  }

  return { customTabDefs, addCustomTab, removeCustomTab, renameCustomTab, isCustomTab };
}
