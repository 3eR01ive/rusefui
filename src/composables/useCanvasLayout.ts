import { invoke } from "@tauri-apps/api/core";
import { readonly, ref } from "vue";

export interface CanvasItemRect {
  x: number;
  y: number;
  w: number;
  h: number;
  z: number;
}

interface StoredState {
  items: Record<string, CanvasItemRect>;
}

const GRID = 8;

export function snapGrid(v: number): number {
  return Math.round(v / GRID) * GRID;
}

export function useCanvasLayout(canvasId: string) {
  const stored = ref<StoredState>({ items: {} });
  const editMode = ref(false);

  const storageKey = `canvas:${canvasId}`;

  async function load() {
    try {
      const raw = await invoke<StoredState>("project_ui_get", { key: storageKey });
      if (raw && typeof raw.items === "object") stored.value = raw;
    } catch {
      // no saved layout yet — use defaults from layout hints
    }
  }

  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  function scheduleSave() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      void invoke("project_ui_set", { key: storageKey, value: stored.value }).catch(() => {});
    }, 400);
  }

  function getRect(id: string, hint: Partial<CanvasItemRect>): CanvasItemRect {
    const saved = stored.value.items[id];
    if (saved) return saved;
    return {
      x: hint.x ?? 0,
      y: hint.y ?? 0,
      w: hint.w ?? 200,
      h: hint.h ?? 160,
      z: 1,
    };
  }

  function setRect(id: string, rect: CanvasItemRect) {
    stored.value = {
      ...stored.value,
      items: { ...stored.value.items, [id]: rect },
    };
    scheduleSave();
  }

  function bringToFront(id: string) {
    const entries = Object.entries(stored.value.items);
    if (!entries.length) return;
    // Re-normalize all z-indices, target goes to top
    const others = entries
      .filter(([k]) => k !== id)
      .sort(([, a], [, b]) => a.z - b.z)
      .map(([k, v], i): [string, CanvasItemRect] => [k, { ...v, z: i + 1 }]);
    const target = stored.value.items[id];
    if (target) others.push([id, { ...target, z: others.length + 1 }]);
    stored.value = { ...stored.value, items: Object.fromEntries(others) };
    scheduleSave();
  }

  function reset() {
    stored.value = { items: {} };
    void invoke("project_ui_set", { key: storageKey, value: { items: {} } }).catch(() => {});
  }

  return { editMode, load, getRect, setRect, bringToFront, reset, stored: readonly(stored) };
}
