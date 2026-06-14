<script setup lang="ts">
import { computed, nextTick, ref } from "vue";
import type { ResolvedTab } from "../core/types";
import type { ComponentInstance } from "../core/types";
import { childPath as makeChildPath } from "../core/instance";
import { useCanvasLayout, snapGrid } from "../composables/useCanvasLayout";
import ComponentHost from "./ComponentHost.vue";
import CanvasWindow from "./canvas/CanvasWindow.vue";

const props = defineProps<{
  tab: ResolvedTab;
}>();

const emit = defineEmits<{
  (e: "select-path", path: string): void;
  (e: "activate-path", path: string): void;
}>();

// Дети root-компонента таба — это то что мы позиционируем.
// Если у root нет children (leaf), кладём сам root.
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

// ── Layout state ──────────────────────────────────────────────
const { editMode, load, getRect, setRect, bringToFront, reset, stored } =
  useCanvasLayout(`tab-${props.tab.id}`);

// Есть сохранённые позиции → canvas mode. Иначе → flow mode.
const hasLayout = computed(() => Object.keys(stored.value.items).length > 0);

// Refs на wrapper-div'ы в flow-режиме для захвата DOM-позиций
const flowRefs: (HTMLElement | null)[] = [];
function setFlowRef(i: number, el: unknown) {
  flowRefs[i] = (el as HTMLElement | null);
}

const containerRef = ref<HTMLElement | null>(null);

// Загружаем состояние при монтировании
let loaded = false;
function ensureLoaded() {
  if (!loaded) { loaded = true; void load(); }
}

// Высота канваса — нижний край самого нижнего окна + отступ
const CANVAS_PAD = 80;
const canvasMinH = computed(() => {
  let max = 400;
  rootChildren.value.forEach((child, i) => {
    const r = getRect(childKey(child, i), {});
    max = Math.max(max, r.y + r.h + CANVAS_PAD);
  });
  return max;
});

// ── Переключение Layout mode ───────────────────────────────────
async function toggleLayout() {
  if (!hasLayout.value) {
    // Первое включение: читаем реальные DOM-позиции flow-режима
    await nextTick(); // убедимся что DOM актуален
    const containerEl = containerRef.value;
    if (containerEl) {
      const cr = containerEl.getBoundingClientRect();
      const scrollTop = containerEl.scrollTop ?? 0;
      rootChildren.value.forEach((child, i) => {
        const el = flowRefs[i];
        if (!el) return;
        const r = el.getBoundingClientRect();
        setRect(childKey(child, i), {
          x: snapGrid(Math.max(0, r.left - cr.left)),
          y: snapGrid(Math.max(0, r.top - cr.top + scrollTop)),
          w: snapGrid(Math.max(80, r.width)),
          h: snapGrid(Math.max(48, r.height)),
          z: i + 1,
        });
      });
    }
    editMode.value = true;
  } else {
    editMode.value = !editMode.value;
  }
}

function resetLayout() {
  reset();
  editMode.value = false;
}

ensureLoaded();
</script>

<template>
  <!-- ── Flow mode (нет сохранённого layout) ── -->
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

    <!-- Layout FAB -->
    <button class="tcl-fab" :class="{ 'tcl-fab--edit': editMode }" @click="toggleLayout">
      <svg viewBox="0 0 16 16" fill="none" class="tcl-fab-icon">
        <rect x="1" y="1" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
        <rect x="9" y="1" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
        <rect x="1" y="9" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
        <rect x="9" y="9" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
      </svg>
      Layout
    </button>
  </div>

  <!-- ── Canvas mode (есть позиции) ── -->
  <div
    v-else
    ref="containerRef"
    class="tcl-canvas"
    :class="{ 'tcl-canvas--edit': editMode }"
    :style="{ minHeight: `${canvasMinH}px` }"
  >
    <CanvasWindow
      v-for="(child, i) in rootChildren"
      :key="childKey(child, i)"
      :id="childKey(child, i)"
      :rect="getRect(childKey(child, i), {})"
      :edit-mode="editMode"
      :locked="Boolean(child.layout?.locked)"
      :min-w="child.layout?.minW"
      :min-h="child.layout?.minH"
      @update:rect="setRect(childKey(child, i), $event)"
      @activate="bringToFront(childKey(child, i))"
    >
      <ComponentHost
        :instance="child"
        :path="childPathFor(i)"
        @select-path="emit('select-path', $event)"
        @activate-path="emit('activate-path', $event)"
      />
    </CanvasWindow>

    <!-- Layout FAB -->
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
      <button v-if="editMode" class="tcl-reset" @click="resetLayout">
        Сброс
      </button>
    </div>
  </div>
</template>

<style scoped>
/* ── Flow mode ── */
.tcl-flow {
  position: relative;
  width: 100%;
}

/* ── Canvas mode ── */
.tcl-canvas {
  position: relative;
  width: 100%;
  overflow: auto;
  background: var(--color-bg);
}

.tcl-canvas--edit {
  background-image: radial-gradient(circle, var(--color-border) 1px, transparent 1px);
  background-size: 16px 16px;
  background-position: 8px 8px;
}

/* ── FAB ── */
.tcl-fab-row {
  position: fixed;
  bottom: 1.5rem;
  right: 1.5rem;
  z-index: 1000;
  display: flex;
  align-items: center;
  gap: 0.4rem;
}

/* В flow mode FAB тоже фиксирован */
.tcl-flow .tcl-fab {
  position: fixed;
  bottom: 1.5rem;
  right: 1.5rem;
  z-index: 1000;
}

.tcl-fab {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  padding: 0.45rem 0.85rem;
  font-size: 0.78rem;
  font-weight: 500;
  border: 1.5px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
  color: var(--color-text-muted);
  cursor: pointer;
  box-shadow: 0 2px 8px rgba(0,0,0,.15);
  transition: border-color 0.1s, color 0.1s, background 0.1s;
}
.tcl-fab:hover { border-color: var(--color-text-muted); color: var(--color-text); }
.tcl-fab--edit {
  border-color: var(--color-accent, #3b82f6);
  color: var(--color-accent, #3b82f6);
  background: color-mix(in srgb, var(--color-accent, #3b82f6) 12%, var(--color-bg-elevated));
}

.tcl-fab-icon { width: 16px; height: 16px; }

.tcl-reset {
  padding: 0.45rem 0.7rem;
  font-size: 0.75rem;
  border: 1.5px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
  color: var(--color-text-muted);
  cursor: pointer;
  box-shadow: 0 2px 8px rgba(0,0,0,.15);
}
.tcl-reset:hover { border-color: var(--color-danger, #dc2626); color: var(--color-danger, #dc2626); }
</style>
