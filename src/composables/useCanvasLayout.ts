import { invoke } from "@tauri-apps/api/core";
import { computed, readonly, ref } from "vue";
import type { ComponentInstance } from "../core/types";

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
  /** Инстансы, добавленные пользователем через picker (не из YAML). */
  extra?: ComponentInstance[];
  /** ID YAML-детей, скрытых пользователем. */
  hidden?: string[];
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
 * Направление вытеснения определяется по вектору центр→центр:
 * если B в основном ниже/выше A → толкаем вертикально,
 * если правее/левее → горизонтально.
 */
function resolveOverlapsFor(
  items: Record<string, CanvasItemRect>,
): Record<string, CanvasItemRect> {
  const result: Record<string, CanvasItemRect> = {};
  for (const [k, v] of Object.entries(items)) result[k] = { ...v };

  let changed = true;
  let iter = 0;
  while (changed && iter++ < 20) {
    changed = false;
    // Сортируем по y→x: верхние/левые компоненты — «якоря»
    const keys = Object.keys(result).sort((a, b) => {
      const ra = result[a]!, rb = result[b]!;
      return ra.y !== rb.y ? ra.y - rb.y : ra.x - rb.x;
    });

    for (const aId of keys) {
      const aRect = result[aId]!;
      if (aRect.floating) continue;
      for (const bId of keys) {
        if (aId === bId) continue;
        const bRect = result[bId]!;
        if (bRect.floating) continue;
        if (!rectsOverlap(aRect, bRect)) continue;

        // Вектор от центра A к центру B
        const dx = (bRect.x + bRect.w / 2) - (aRect.x + aRect.w / 2);
        const dy = (bRect.y + bRect.h / 2) - (aRect.y + aRect.h / 2);

        let nx = bRect.x, ny = bRect.y;
        if (Math.abs(dy) >= Math.abs(dx)) {
          // Основное смещение вертикальное → толкаем вверх/вниз
          if (dy >= 0) ny = snapGrid(aRect.y + aRect.h + GAP);
          else         ny = snapGrid(aRect.y - bRect.h - GAP);
        } else {
          // Основное смещение горизонтальное → толкаем вправо/влево
          if (dx >= 0) nx = snapGrid(aRect.x + aRect.w + GAP);
          else         nx = snapGrid(aRect.x - bRect.w - GAP);
        }

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
   * Завершение drag/resize: сохраняем только перетаскиваемый компонент.
   * Соседи НЕ сохраняются — их визуальные позиции вычисляются в computedRects.
   */
  function commitRect(id: string) {
    const computed = computedRects.value[id];
    const storedRect = stored.value.items[id];
    if (!computed || !storedRect) { scheduleSave(); return; }
    // Если пользователь уменьшил окно — сбрасываем кеш actualHeights,
    // иначе max(stored.h, actualH) не даст окну схлопнуться.
    if (storedRect.h < (actualHeights.value[id] ?? 0)) {
      actualHeights.value = { ...actualHeights.value, [id]: 0 };
    }
    stored.value = {
      ...stored.value,
      items: {
        ...stored.value.items,
        [id]: { ...computed, h: storedRect.h },
      },
    };
    scheduleSave();
  }

  /**
   * Вызывается ResizeObserver'ом в CanvasWindow.
   * Обновляет фактическую высоту — НЕ меняет stored.
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

  // ── Добавление / скрытие / удаление инстансов ────────────────

  /** Возвращает позицию для нового компонента (ниже всех существующих). */
  function nextAddPosition(): { x: number; y: number } {
    const items = Object.values(stored.value.items);
    if (!items.length) return { x: snapGrid(16), y: snapGrid(16) };
    const maxBottom = items.reduce((m, r) => Math.max(m, r.y + r.h), 0);
    return { x: snapGrid(16), y: snapGrid(maxBottom + 16) };
  }

  /**
   * Добавляет экстра-инстанс (не из YAML). Присваивает уникальный id.
   * Возвращает назначенный id.
   */
  function addExtraInstance(instance: ComponentInstance, layout?: Partial<CanvasItemRect>): string {
    const usedIds = new Set([
      ...(stored.value.extra ?? []).map(e => e.id ?? ''),
      ...Object.keys(stored.value.items),
    ]);
    let n = 0;
    let id: string;
    do { id = `extra_${n++}`; } while (usedIds.has(id));

    const inst: ComponentInstance = { ...instance, id };
    const pos = nextAddPosition();
    const rect: CanvasItemRect = {
      x: layout?.x ?? pos.x,
      y: layout?.y ?? pos.y,
      w: layout?.w ?? 400,
      h: layout?.h ?? 240,
      z: Object.keys(stored.value.items).length + 1,
      floating: false,
    };

    stored.value = {
      ...stored.value,
      extra: [...(stored.value.extra ?? []), inst],
      items: { ...stored.value.items, [id]: rect },
    };
    scheduleSave();
    return id;
  }

  /** Скрывает YAML-ребёнка (добавляет в hidden list). */
  function hideInstance(id: string) {
    if ((stored.value.hidden ?? []).includes(id)) return;
    stored.value = { ...stored.value, hidden: [...(stored.value.hidden ?? []), id] };
    scheduleSave();
  }

  /** Обновляет bind у экстра-инстанса. */
  function updateExtraInstanceBind(id: string, bind: import("../core/types").DataBinding | undefined) {
    stored.value = {
      ...stored.value,
      extra: (stored.value.extra ?? []).map(e => e.id === id ? { ...e, bind } : e),
    };
    scheduleSave();
  }

  /** Полностью удаляет экстра-инстанс. */
  function removeExtraInstance(id: string) {
    const newItems = { ...stored.value.items };
    delete newItems[id];
    stored.value = {
      ...stored.value,
      extra: (stored.value.extra ?? []).filter(e => e.id !== id),
      items: newItems,
    };
    scheduleSave();
  }

  function reset() {
    stored.value = { items: {}, extra: [], hidden: [] };
    actualHeights.value = {};
    void invoke("project_ui_set", { key: storageKey, value: { items: {}, extra: [], hidden: [] } }).catch(() => {});
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
    addExtraInstance,
    updateExtraInstanceBind,
    hideInstance,
    removeExtraInstance,
  };
}
