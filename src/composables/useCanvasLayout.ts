import { invoke } from "@tauri-apps/api/core";
import { computed, readonly, ref } from "vue";

export interface CanvasItemRect {
  x: number;
  y: number;
  w: number;
  h: number;
  z: number;
  floating?: boolean;
}

interface StoredState {
  items: Record<string, CanvasItemRect>;
}

const GRID = 8;
const GAP = 8;

export function snapGrid(v: number): number {
  return Math.round(v / GRID) * GRID;
}

function rectsOverlap(a: CanvasItemRect, b: CanvasItemRect): boolean {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
}

/**
 * Чистая функция — разрешает перекрытия не трогая stored state.
 * Используется как для drag-commit (→ сохраняем), так и для
 * live-вычисления позиций при росте контента (→ не сохраняем).
 */
function resolveOverlapsFor(
  items: Record<string, CanvasItemRect>,
): Record<string, CanvasItemRect> {
  const result: Record<string, CanvasItemRect> = {};
  for (const [k, v] of Object.entries(items)) result[k] = { ...v };

  let changed = true;
  let iter = 0;
  while (changed && iter++ < 15) {
    changed = false;
    const keys = Object.keys(result);
    for (const aId of keys) {
      const aRect = result[aId]!;
      if (aRect.floating) continue;
      for (const bId of keys) {
        if (aId === bId) continue;
        const bRect = result[bId]!;
        if (bRect.floating) continue;
        if (!rectsOverlap(aRect, bRect)) continue;

        const pushR = aRect.x + aRect.w - bRect.x;
        const pushL = bRect.x + bRect.w - aRect.x;
        const pushD = aRect.y + aRect.h - bRect.y;
        const pushU = bRect.y + bRect.h - aRect.y;
        const min = Math.min(pushR, pushL, pushD, pushU);

        let nx = bRect.x, ny = bRect.y;
        if (min === pushD)      ny = snapGrid(aRect.y + aRect.h + GAP);
        else if (min === pushU) ny = snapGrid(aRect.y - bRect.h - GAP);
        else if (min === pushR) nx = snapGrid(aRect.x + aRect.w + GAP);
        else                    nx = snapGrid(aRect.x - bRect.w - GAP);

        nx = Math.max(0, nx);
        ny = Math.max(0, ny);

        if (nx !== bRect.x || ny !== bRect.y) {
          result[bId] = { ...bRect, x: nx, y: ny };
          changed = true;
        }
      }
    }
  }
  return result;
}

export function useCanvasLayout(canvasId: string) {
  /** Постоянные "базовые" позиции: меняются только через drag/resize. */
  const stored = ref<StoredState>({ items: {} });

  /**
   * Фактические высоты окон от ResizeObserver — НЕ сохраняются на диск.
   * Когда контент растёт (открылись настройки) → высота растёт → соседи двигаются.
   * Когда контент схлопывается → высота падает → соседи возвращаются.
   */
  const actualHeights = ref<Record<string, number>>({});

  const editMode = ref(false);
  const storageKey = `canvas:${canvasId}`;

  async function load() {
    try {
      const raw = await invoke<StoredState>("project_ui_get", { key: storageKey });
      if (raw && typeof raw.items === "object") stored.value = raw;
    } catch { /* нет сохранённого layout */ }
  }

  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  function scheduleSave() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      void invoke("project_ui_set", { key: storageKey, value: stored.value }).catch(() => {});
    }, 400);
  }

  /**
   * Позиции для отображения = базовые + фактические высоты → overlap resolution.
   * Эти позиции НЕ сохраняются. Когда контент схлопывается — соседи возвращаются.
   */
  const computedRects = computed<Record<string, CanvasItemRect>>(() => {
    const effective: Record<string, CanvasItemRect> = {};
    for (const [id, rect] of Object.entries(stored.value.items)) {
      const actualH = actualHeights.value[id] ?? 0;
      effective[id] = { ...rect, h: Math.max(rect.h, actualH) };
    }
    return resolveOverlapsFor(effective);
  });

  function getRect(id: string, hint: Partial<CanvasItemRect>): CanvasItemRect {
    return stored.value.items[id] ?? {
      x: hint.x ?? 0, y: hint.y ?? 0,
      w: hint.w ?? 200, h: hint.h ?? 160,
      z: 1, floating: hint.floating ?? false,
    };
  }

  /** Обновляет базовую позицию (drag/resize). */
  function setRect(id: string, rect: CanvasItemRect) {
    stored.value = { ...stored.value, items: { ...stored.value.items, [id]: rect } };
    scheduleSave();
  }

  /**
   * Завершение drag/resize: сохраняем resolved позиции соседей в stored.
   * Это делает позиции permanentными (в отличие от роста контента).
   */
  function commitRect(_id: string) {
    const resolved = resolveOverlapsFor({ ...stored.value.items });
    stored.value = { ...stored.value, items: resolved };
    scheduleSave();
  }

  /**
   * Вызывается ResizeObserver'ом в CanvasWindow.
   * Обновляет фактическую высоту — НЕ меняет stored.
   * Автоматически пересчитывает computedRects.
   */
  function setActualHeight(id: string, h: number) {
    if ((actualHeights.value[id] ?? 0) === h) return;
    actualHeights.value = { ...actualHeights.value, [id]: h };
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
    actualHeights.value = {};
    void invoke("project_ui_set", { key: storageKey, value: { items: {} } }).catch(() => {});
  }

  return {
    editMode,
    load,
    getRect,
    setRect,
    commitRect,
    setActualHeight,
    computedRects,
    bringToFront,
    reset,
    stored: readonly(stored),
  };
}
