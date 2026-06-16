<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useCanvasLayout } from "../../composables/useCanvasLayout";
import { useCanvasContextMenu, listMenuTypes } from "../../composables/useCanvasContextMenu";
import type { CanvasItem } from "../../composables/useCanvasContextMenu";
import { childPath as makeChildPath } from "../../core/instance";
import ComponentHost from "../ComponentHost.vue";
import CanvasWindow from "../canvas/CanvasWindow.vue";
import CanvasContextMenu from "../canvas/CanvasContextMenu.vue";

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

const containerRef = ref<HTMLElement | null>(null);
const canvasId = computed(() => props.instance.id ?? props.path.replace(/\//g, "-"));

const {
  editMode, load,
  getRect, setRect, commitRect, setActualHeight,
  computedRects, bringToFront, reset, stored,
  addExtraInstance, updateExtraInstanceBind, hideInstance, removeExtraInstance,
} = useCanvasLayout(canvasId.value);

// ── Items ──────────────────────────────────────────────────────
const yamlChildren = computed(() => props.instance.children ?? []);

function childId(child: ComponentInstance, index: number): string {
  return child.id ?? `child-${index}`;
}

const allItems = computed<CanvasItem[]>(() => {
  const hiddenSet = new Set(stored.value.hidden ?? []);

  const yaml: CanvasItem[] = yamlChildren.value
    .map((child, i) => ({ key: childId(child, i), isExtra: false as const, child }))
    .filter(({ key }) => !hiddenSet.has(key));

  const extra: CanvasItem[] = (stored.value.extra ?? []).map((child) => ({
    key: child.id!,
    isExtra: true as const,
    child: child as ComponentInstance,
  }));

  return [...yaml, ...extra];
});

function childPath(item: CanvasItem, index: number): string {
  if (item.isExtra) return `${props.path}/extra/${item.key}`;
  const i = yamlChildren.value.findIndex((_, idx) => childId(_, idx) === item.key);
  return makeChildPath(props.path, i >= 0 ? i : index, item.child);
}

function getItemRect(item: CanvasItem) {
  const hint = item.child.layout ?? {};
  return getRect(item.key, { x: hint.x, y: hint.y, w: hint.w, h: hint.h });
}

function storedH(item: CanvasItem): number {
  return stored.value.items[item.key]?.h ?? item.child.layout?.h ?? 160;
}

const CANVAS_PAD = 64;
const canvasMinH = computed(() => {
  let max = 400;
  allItems.value.forEach((item) => {
    const r = computedRects.value[item.key] ?? getItemRect(item);
    max = Math.max(max, r.y + r.h + CANVAS_PAD);
  });
  return max;
});

function onRemoveItem(key: string, isExtra: boolean): void {
  if (isExtra) removeExtraInstance(key);
  else hideInstance(key);
}

// ── Context menu ───────────────────────────────────────────────
const menuTypes = listMenuTypes();

const {
  ctx,
  onCanvasContextMenu,
  onComponentContextMenu,
  onSelectType,
  onSelectTable,
  onSelectCurve,
  onSelectConfigField,
  onSelectOutputField,
  ctxBack,
} = useCanvasContextMenu({
  menuTypes,
  addExtraInstance,
  updateExtraInstanceBind,
  editMode,
  containerRef,
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
      <button v-if="editMode" class="canvas-reset-btn" @click="reset">Сброс</button>
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
      ref="containerRef"
      class="canvas"
      :class="{ 'canvas--edit': editMode }"
      :style="{ minHeight: `${canvasMinH}px` }"
      @contextmenu="onCanvasContextMenu"
    >
      <CanvasWindow
        v-for="(item, i) in allItems"
        :key="item.key"
        :id="item.key"
        :rect="computedRects[item.key] ?? getItemRect(item)"
        :stored-h="storedH(item)"
        :edit-mode="editMode"
        :locked="Boolean(item.child.layout?.locked)"
        :min-w="item.child.layout?.minW"
        :min-h="item.child.layout?.minH"
        :removable="editMode"
        @update:rect="setRect(item.key, $event)"
        @commit="commitRect(item.key)"
        @actual-height="setActualHeight(item.key, $event)"
        @activate="bringToFront(item.key)"
        @remove="onRemoveItem(item.key, item.isExtra)"
        @contextmenu="onComponentContextMenu(item.key, item, $event)"
      >
        <ComponentHost
          :instance="item.child"
          :path="childPath(item, i)"
        />
      </CanvasWindow>

      <div v-if="editMode && allItems.length === 0" class="canvas-empty-hint">
        Правый клик — добавить компонент
      </div>
    </div>

    <CanvasContextMenu
      :ctx="ctx"
      :menu-types="menuTypes"
      @select-type="onSelectType"
      @select-table="onSelectTable"
      @select-curve="onSelectCurve"
      @select-config-field="onSelectConfigField"
      @select-output-field="onSelectOutputField"
      @back="ctxBack"
    />
  </div>
</template>

<style scoped>
.canvas-root {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 0;
}

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

.canvas-reset-btn {
  padding: 0.2rem 0.5rem;
  font-size: 0.72rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: none;
  color: var(--color-text-muted);
  cursor: pointer;
  transition: border-color 0.1s, color 0.1s;
}
.canvas-reset-btn:hover { border-color: var(--color-danger, #dc2626); color: var(--color-danger, #dc2626); }

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
.canvas--edit {
  background-image: radial-gradient(circle, var(--color-border) 1px, transparent 1px);
  background-size: 16px 16px;
  background-position: 8px 8px;
}

.canvas-empty-hint {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  font-size: 0.82rem;
  color: var(--color-text-subtle);
  pointer-events: none;
  user-select: none;
}
</style>
