import { invoke } from "@tauri-apps/api/core";
import { readonly, ref } from "vue";

export interface CanvasItemRect {
  x: number;
  y: number;
  w: number;
  h: number;
  z: number;
  /** true — может перекрываться с другими (парит поверх) */
  floating?: boolean;
}

interface StoredState {
  items: Record<string, CanvasItemRect>;
}

const GRID = 8;
const GAP = 8; // зазор между окнами при разрешении перекрытий

export function snapGrid(v: number): number {
  return Math.round(v / GRID) * GRID;
}

function rectsOverlap(a: CanvasItemRect, b: CanvasItemRect): boolean {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
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
      floating: hint.floating ?? false,
    };
  }

  function setRect(id: string, rect: CanvasItemRect) {
    stored.value = {
      ...stored.value,
      items: { ...stored.value.items, [id]: rect },
    };
    scheduleSave();
  }

  /**
   * Разрешает перекрытия после завершения drag/resize для non-floating элементов.
   * Алгоритм: повторяем проходы пока есть перекрытия (макс. 15 итераций).
   * Двигаем перекрывающийся элемент в направлении наименьшего выхода.
   */
  function resolveOverlaps(movedId: string) {
    const movedRect = stored.value.items[movedId];
    if (!movedRect || movedRect.floating) return;

    const items = { ...stored.value.items };
    let changed = true;
    let iter = 0;

    while (changed && iter++ < 15) {
      changed = false;
      for (const [aId, aRect] of Object.entries(items)) {
        if (aRect.floating) continue;
        for (const [bId, bRect] of Object.entries(items)) {
          if (aId === bId || bRect.floating) continue;
          if (!rectsOverlap(aRect, bRect)) continue;

          // Глубина перекрытия по каждой оси
          const overlapRight = aRect.x + aRect.w - bRect.x;  // сколько B надо сдвинуть вправо
          const overlapLeft  = bRect.x + bRect.w - aRect.x;  // сколько B надо сдвинуть влево
          const overlapDown  = aRect.y + aRect.h - bRect.y;  // сколько B надо сдвинуть вниз
          const overlapUp    = bRect.y + bRect.h - aRect.y;  // сколько B надо сдвинуть вверх

          // Двигаем B в направлении минимального выхода
          const min = Math.min(overlapRight, overlapLeft, overlapDown, overlapUp);

          let newX = bRect.x, newY = bRect.y;
          if (min === overlapDown)       newY = snapGrid(aRect.y + aRect.h + GAP);
          else if (min === overlapUp)    newY = snapGrid(aRect.y - bRect.h - GAP);
          else if (min === overlapRight) newX = snapGrid(aRect.x + aRect.w + GAP);
          else                           newX = snapGrid(aRect.x - bRect.w - GAP);

          newX = Math.max(0, newX);
          newY = Math.max(0, newY);

          if (newX !== bRect.x || newY !== bRect.y) {
            items[bId] = { ...bRect, x: newX, y: newY };
            changed = true;
          }
        }
      }
    }

    stored.value = { ...stored.value, items };
    scheduleSave();
  }

  function bringToFront(id: string) {
    const entries = Object.entries(stored.value.items);
    if (!entries.length) return;
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

  return {
    editMode,
    load,
    getRect,
    setRect,
    resolveOverlaps,
    bringToFront,
    reset,
    stored: readonly(stored),
  };
}
