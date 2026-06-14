<script setup lang="ts">
import { computed, onMounted } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useCanvasLayout } from "../../composables/useCanvasLayout";
import { childPath as makeChildPath } from "../../core/instance";
import ComponentHost from "../ComponentHost.vue";
import CanvasWindow from "../canvas/CanvasWindow.vue";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

defineEmits<{
  (e: "select-path", path: string): void;
  (e: "activate-path", path: string): void;
}>();

// canvas id из instance.id или path
const canvasId = computed(() => props.instance.id ?? props.path.replace(/\//g, "-"));

const {
  editMode, load, getRect, setRect, commitRect, setActualHeight,
  computedRects, bringToFront, stored,
} = useCanvasLayout(canvasId.value);

const children = computed(() => props.instance.children ?? []);

function childId(child: ComponentInstance, index: number): string {
  return child.id ?? `child-${index}`;
}

function getChildRect(child: ComponentInstance, index: number) {
  const hint = child.layout ?? {};
  return getRect(childId(child, index), {
    x: hint.x,
    y: hint.y,
    w: hint.w,
    h: hint.h,
  });
}

// Минимальная высота канваса: нижний край самого нижнего компонента + отступ
const MIN_CANVAS_H = 400;
const CANVAS_PAD = 64;
const canvasMinH = computed(() => {
  let max = MIN_CANVAS_H;
  children.value.forEach((child, i) => {
    const r = getChildRect(child, i);
    max = Math.max(max, r.y + r.h + CANVAS_PAD);
  });
  return max;
});

const canvasTitle = computed(() => String(props.props.title ?? ""));

onMounted(() => { void load(); });
</script>

<template>
  <div class="canvas-root">
    <!-- toolbar -->
    <div class="canvas-tb">
      <span v-if="canvasTitle" class="canvas-tb-title">{{ canvasTitle }}</span>
      <span class="canvas-tb-spacer" />
      <button
        class="canvas-edit-btn"
        :class="{ active: editMode }"
        @click="editMode = !editMode"
      >
        <svg viewBox="0 0 14 14" fill="none" class="canvas-edit-icon">
          <rect x="1" y="1" width="5" height="5" rx="1" stroke="currentColor" stroke-width="1.2"/>
          <rect x="8" y="1" width="5" height="5" rx="1" stroke="currentColor" stroke-width="1.2"/>
          <rect x="1" y="8" width="5" height="5" rx="1" stroke="currentColor" stroke-width="1.2"/>
          <rect x="8" y="8" width="5" height="5" rx="1" stroke="currentColor" stroke-width="1.2"/>
        </svg>
        {{ editMode ? "Готово" : "Layout" }}
      </button>
    </div>

    <!-- canvas area -->
    <div
      class="canvas"
      :class="{ 'canvas--edit': editMode }"
      :style="{ minHeight: `${canvasMinH}px` }"
    >
      <CanvasWindow
        v-for="(child, i) in children"
        :key="childId(child, i)"
        :id="childId(child, i)"
        :rect="computedRects[childId(child, i)] ?? getChildRect(child, i)"
        :stored-h="stored.items[childId(child, i)]?.h ?? child.layout?.h ?? 160"
        :edit-mode="editMode"
        :locked="Boolean(child.layout?.locked)"
        :min-w="child.layout?.minW"
        :min-h="child.layout?.minH"
        @update:rect="setRect(childId(child, i), $event)"
        @commit="commitRect(childId(child, i))"
        @actual-height="setActualHeight(childId(child, i), $event)"
        @activate="bringToFront(childId(child, i))"
      >
        <ComponentHost
          :instance="child"
          :path="makeChildPath(path, i, child)"
        />
      </CanvasWindow>
    </div>
  </div>
</template>

<style scoped>
.canvas-root {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 0;
}

/* toolbar */
.canvas-tb {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.35rem 0.6rem;
  border-bottom: 1px solid var(--color-border);
  background: var(--color-bg-muted);
  border-radius: var(--radius-md) var(--radius-md) 0 0;
}
.canvas-tb-title {
  font-size: 0.8rem;
  font-weight: 600;
  color: var(--color-text);
}
.canvas-tb-spacer { flex: 1; }

.canvas-edit-btn {
  display: flex;
  align-items: center;
  gap: 0.3rem;
  font-size: 0.72rem;
  padding: 0.2rem 0.55rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: none;
  color: var(--color-text-muted);
  cursor: pointer;
  transition: border-color 0.1s, color 0.1s;
}
.canvas-edit-btn:hover { border-color: var(--color-text-muted); color: var(--color-text); }
.canvas-edit-btn.active {
  border-color: var(--color-accent, #3b82f6);
  color: var(--color-accent, #3b82f6);
  background: color-mix(in srgb, var(--color-accent, #3b82f6) 10%, transparent);
}
.canvas-edit-icon { width: 14px; height: 14px; }

/* canvas */
.canvas {
  position: relative;
  width: 100%;
  overflow: auto;
  background: var(--color-bg);
  border: 1px solid var(--color-border);
  border-top: none;
  border-radius: 0 0 var(--radius-md) var(--radius-md);
  box-sizing: border-box;
}

/* dot-grid в edit mode */
.canvas--edit {
  background-image: radial-gradient(circle, var(--color-border) 1px, transparent 1px);
  background-size: 16px 16px;
  background-position: 8px 8px;
}
</style>
