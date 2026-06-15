<script setup lang="ts">
import { computed, nextTick, ref } from "vue";
import type { ComponentInstance, ResolvedTab } from "../core/types";
import { childPath as makeChildPath } from "../core/instance";
import { useCanvasLayout, snapGrid } from "../composables/useCanvasLayout";
import type { CanvasItemRect } from "../composables/useCanvasLayout";
import ComponentHost from "./ComponentHost.vue";
import CanvasWindow from "./canvas/CanvasWindow.vue";

const props = defineProps<{ tab: ResolvedTab }>();

const emit = defineEmits<{
  (e: "select-path", path: string): void;
  (e: "activate-path", path: string): void;
}>();

const rootChildren = computed<ComponentInstance[]>(() => {
  const ch = props.tab.root.children;
  if (ch && ch.length > 0) return ch;
  return [props.tab.root];
});

function childKey(child: ComponentInstance, index: number): string {
  return child.id ?? `c${index}`;
}

function childPathFor(index: number): string {
  const child = rootChildren.value[index]!;
  return makeChildPath(`tab/${props.tab.id}`, index, child);
}

function isFloating(child: ComponentInstance): boolean {
  return Boolean(child.layout?.floating);
}

// ── Layout state ──────────────────────────────────────────────
const {
  editMode, load,
  getRect, setRect, commitRect, setActualHeight,
  computedRects,
  bringToFront, reset, stored,
} = useCanvasLayout(`tab-${props.tab.id}`);

const hasLayout = computed(() => Object.keys(stored.value.items).length > 0);

/** Display rect из computedRects (с учётом роста контента + выталкивания) */
function displayRect(child: ComponentInstance, i: number): CanvasItemRect {
  const key = childKey(child, i);
  return computedRects.value[key] ?? getRect(key, {
    x: child.layout?.x, y: child.layout?.y,
    w: child.layout?.w, h: child.layout?.h,
    floating: isFloating(child),
  });
}

/** Сохранённая базовая высота (для min-height на CanvasWindow) */
function storedH(child: ComponentInstance, i: number): number {
  return stored.value.items[childKey(child, i)]?.h ?? child.layout?.h ?? 160;
}

// ── Flow mode refs ─────────────────────────────────────────────
const flowRefs: (HTMLElement | null)[] = [];
function setFlowRef(i: number, el: unknown) { flowRefs[i] = el as HTMLElement | null; }
const containerRef = ref<HTMLElement | null>(null);

// Минимальная высота канваса
const CANVAS_PAD = 80;
const canvasMinH = computed(() => {
  let max = 400;
  rootChildren.value.forEach((child, i) => {
    const r = displayRect(child, i);
    max = Math.max(max, r.y + r.h + CANVAS_PAD);
  });
  return max;
});

// ── Toggle layout ──────────────────────────────────────────────
async function toggleLayout() {
  if (!hasLayout.value) {
    await nextTick();
    const cr = containerRef.value?.getBoundingClientRect();
    if (cr) {
      const scrollTop = containerRef.value?.scrollTop ?? 0;
      rootChildren.value.forEach((child, i) => {
        const el = flowRefs[i];
        if (!el) return;
        const r = el.getBoundingClientRect();
        const y = snapGrid(Math.max(0, r.top - cr.top + scrollTop));
        // Cap height so y + h + CANVAS_PAD <= cr.height → no initial scrollbar
        const maxH = Math.max(160, cr.height - y - CANVAS_PAD);
        setRect(childKey(child, i), {
          x: snapGrid(Math.max(0, r.left - cr.left)),
          y,
          w: snapGrid(Math.max(80, r.width)),
          h: snapGrid(Math.max(48, Math.min(r.height, maxH))),
          z: i + 1,
          floating: isFloating(child),
        });
      });
    }
    editMode.value = true;
  } else {
    editMode.value = !editMode.value;
  }
}

function resetLayout() { reset(); editMode.value = false; }

// ── Swap при перетаскивании ────────────────────────────────────
// swapSource = "виртуальный дом" тянимого элемента.
// При каждом свапе: другой компонент получает swapSource, swapSource ← позиция другого.
// Это позволяет цепочку свапов за один drag.
let activeDragKey: string | null = null;
let swapSource: { x: number; y: number } | null = null;

function onDragStart(child: ComponentInstance, i: number) {
  const key = childKey(child, i);
  activeDragKey = key;
  const base = stored.value.items[key];
  swapSource = base ? { x: base.x, y: base.y } : null;
}

function checkAndSwap(draggedId: string, draggedRect: CanvasItemRect) {
  if (!swapSource) return;
  const cx = draggedRect.x + draggedRect.w / 2;
  const cy = draggedRect.y + draggedRect.h / 2;

  for (const [otherId, otherRect] of Object.entries(stored.value.items)) {
    if (otherId === draggedId || otherRect.floating) continue;
    if (
      cx >= otherRect.x && cx < otherRect.x + otherRect.w &&
      cy >= otherRect.y && cy < otherRect.y + otherRect.h
    ) {
      // Перемещаем другой компонент на позицию swapSource
      setRect(otherId, { ...otherRect, x: swapSource.x, y: swapSource.y });
      // swapSource обновляется — теперь там где был другой
      swapSource = { x: otherRect.x, y: otherRect.y };
      break; // один свап за кадр
    }
  }
}

// ── Events from CanvasWindow ────────────────────────────────────
function onUpdateRect(child: ComponentInstance, i: number, rect: CanvasItemRect) {
  const key = childKey(child, i);
  setRect(key, rect);
  // Swap-проверка только для move (не resize) и non-floating
  if (activeDragKey === key && swapSource && !isFloating(child)) {
    checkAndSwap(key, rect);
  }
}

function onCommit(child: ComponentInstance, i: number) {
  activeDragKey = null;
  swapSource = null;
  commitRect(childKey(child, i));
}

function onActualHeight(child: ComponentInstance, i: number, h: number) {
  setActualHeight(childKey(child, i), h);
}

let loaded = false;
if (!loaded) { loaded = true; void load(); }
</script>

<template>
  <!-- ── Flow mode ── -->
  <div v-if="!hasLayout" ref="containerRef" class="tcl-flow">
    <div
      v-for="(child, i) in rootChildren"
      :key="childKey(child, i)"
      :ref="(el) => setFlowRef(i, el)"
    >
      <ComponentHost
        :instance="child"
        :path="childPathFor(i)"
        @select-path="emit('select-path', $event)"
        @activate-path="emit('activate-path', $event)"
      />
    </div>
    <button class="tcl-fab" @click="toggleLayout">
      <svg viewBox="0 0 16 16" fill="none" class="tcl-fab-icon">
        <rect x="1" y="1" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
        <rect x="9" y="1" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
        <rect x="1" y="9" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
        <rect x="9" y="9" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
      </svg>
      Layout
    </button>
  </div>

  <!-- ── Canvas mode ── -->
  <div
    v-else
    ref="containerRef"
    class="tcl-canvas"
    :class="{ 'tcl-canvas--edit': editMode }"
    :style="editMode ? { minHeight: `${canvasMinH}px` } : undefined"
  >
    <CanvasWindow
      v-for="(child, i) in rootChildren"
      :key="childKey(child, i)"
      :id="childKey(child, i)"
      :rect="displayRect(child, i)"
      :stored-h="storedH(child, i)"
      :edit-mode="editMode"
      :locked="Boolean(child.layout?.locked)"
      :min-w="child.layout?.minW"
      :min-h="child.layout?.minH"
      @drag-start="onDragStart(child, i)"
      @update:rect="onUpdateRect(child, i, $event)"
      @commit="onCommit(child, i)"
      @actual-height="onActualHeight(child, i, $event)"
      @activate="bringToFront(childKey(child, i))"
    >
      <ComponentHost
        :instance="child"
        :path="childPathFor(i)"
        @select-path="emit('select-path', $event)"
        @activate-path="emit('activate-path', $event)"
      />
    </CanvasWindow>

    <div class="tcl-fab-row">
      <button class="tcl-fab" :class="{ 'tcl-fab--edit': editMode }" @click="toggleLayout">
        <svg viewBox="0 0 16 16" fill="none" class="tcl-fab-icon">
          <rect x="1" y="1" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
          <rect x="9" y="1" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
          <rect x="1" y="9" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
          <rect x="9" y="9" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
        </svg>
        {{ editMode ? "Готово" : "Layout" }}
      </button>
      <button v-if="editMode" class="tcl-reset" @click="resetLayout">Сброс</button>
    </div>
  </div>
</template>

<style scoped>
.tcl-flow { position: relative; width: 100%; }

.tcl-canvas {
  position: relative; width: 100%;
  height: 100%; min-height: 0;
  overflow: hidden auto;
  scrollbar-gutter: stable;
  background: var(--color-bg);
}
.tcl-canvas--edit {
  background-image: radial-gradient(circle, var(--color-border) 1px, transparent 1px);
  background-size: 16px 16px; background-position: 8px 8px;
}

.tcl-fab-row {
  position: fixed; bottom: 1.5rem; right: 1.5rem;
  z-index: 1000; display: flex; align-items: center; gap: 0.4rem;
}
.tcl-flow .tcl-fab { position: fixed; bottom: 1.5rem; right: 1.5rem; z-index: 1000; }
.tcl-fab {
  display: flex; align-items: center; gap: 0.35rem;
  padding: 0.45rem 0.85rem; font-size: 0.78rem; font-weight: 500;
  border: 1.5px solid var(--color-border); border-radius: var(--radius-md);
  background: var(--color-bg-elevated); color: var(--color-text-muted);
  cursor: pointer; box-shadow: 0 2px 8px rgba(0,0,0,.15);
  transition: border-color 0.1s, color 0.1s;
}
.tcl-fab:hover { border-color: var(--color-text-muted); color: var(--color-text); }
.tcl-fab--edit {
  border-color: var(--color-accent, #3b82f6); color: var(--color-accent, #3b82f6);
  background: color-mix(in srgb, var(--color-accent, #3b82f6) 12%, var(--color-bg-elevated));
}
.tcl-fab-icon { width: 16px; height: 16px; }
.tcl-reset {
  padding: 0.45rem 0.7rem; font-size: 0.75rem;
  border: 1.5px solid var(--color-border); border-radius: var(--radius-md);
  background: var(--color-bg-elevated); color: var(--color-text-muted);
  cursor: pointer; box-shadow: 0 2px 8px rgba(0,0,0,.15);
}
.tcl-reset:hover { border-color: var(--color-danger, #dc2626); color: var(--color-danger, #dc2626); }
</style>
