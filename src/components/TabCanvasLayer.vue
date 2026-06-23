<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { ComponentInstance, ResolvedTab } from "../core/types";
import { panelsEpoch } from "../composables/useIniPanels";
import { childPath as makeChildPath } from "../core/instance";
import { useCanvasLayout, snapGrid } from "../composables/useCanvasLayout";
import { setNavExtension } from "../composables/useWorkspaceNav";
import type { CanvasItemRect } from "../composables/useCanvasLayout";
import { useCanvasContextMenu, listMenuTypes } from "../composables/useCanvasContextMenu";
import ComponentHost from "./ComponentHost.vue";
import CanvasWindow from "./canvas/CanvasWindow.vue";
import CanvasContextMenu from "./canvas/CanvasContextMenu.vue";

const props = defineProps<{ tab: ResolvedTab }>();

const emit = defineEmits<{
  (e: "select-path", path: string): void;
  (e: "activate-path", path: string): void;
}>();

const rootChildren = computed<ComponentInstance[]>(() => {
  const ch = props.tab.root.children;
  // Для кастомных табов children явно [], возвращаем пустой массив
  if (ch !== undefined) return ch;
  return [props.tab.root];
});

const hasYamlLayoutHints = computed(() =>
  rootChildren.value.some(c => c.layout?.x != null || c.layout?.y != null),
);

function childKey(child: ComponentInstance, index: number): string {
  return child.id ?? `c${index}`;
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
  addExtraInstance, updateExtraInstanceBind, hideInstance, removeExtraInstance,
} = useCanvasLayout(`tab-${props.tab.id}`);

const hasLayout = computed(() => Object.keys(stored.value.items).length > 0);

// Кастомный таб или YAML-хинты → canvas-режим
const showCanvas = computed(() => hasLayout.value || hasYamlLayoutHints.value || Boolean(props.tab.isCustom));

// ── All visible canvas items ──────────────────────────────────
const allItems = computed(() => {
  const hiddenSet = new Set(stored.value.hidden ?? []);

  const yaml = rootChildren.value
    .map((child, i) => ({
      child,
      key: childKey(child, i),
      isExtra: false as const,
      path: makeChildPath(`tab/${props.tab.id}`, i, child),
    }))
    .filter(({ key }) => !hiddenSet.has(key));

  const extra = (stored.value.extra ?? []).map(child => ({
    child: child as ComponentInstance,
    key: child.id!,
    isExtra: true as const,
    path: `tab/${props.tab.id}/extra/${child.id}`,
  }));

  return [...yaml, ...extra];
});

// ── Rect helpers ──────────────────────────────────────────────
function displayRect(key: string, child: ComponentInstance): CanvasItemRect {
  return computedRects.value[key] ?? getRect(key, {
    x: child.layout?.x, y: child.layout?.y,
    w: child.layout?.w, h: child.layout?.h,
    floating: isFloating(child),
  });
}

function storedHFor(key: string, child: ComponentInstance): number {
  return stored.value.items[key]?.h ?? child.layout?.h ?? 160;
}

// ── Flow mode refs ────────────────────────────────────────────
const flowRefs: (HTMLElement | null)[] = [];
function setFlowRef(i: number, el: unknown) { flowRefs[i] = el as HTMLElement | null; }
const containerRef = ref<HTMLElement | null>(null);

const CANVAS_PAD = 300;
const canvasMinH = computed(() => {
  let max = 400;
  allItems.value.forEach(({ key, child }) => {
    const r = displayRect(key, child);
    max = Math.max(max, r.y + r.h + CANVAS_PAD);
  });
  return max;
});

// ── Toggle layout ──────────────────────────────────────────────
async function toggleLayout() {
  if (!hasLayout.value) {
    if (hasYamlLayoutHints.value) {
      // Уже в canvas-режиме по YAML-хинтам — просто переключить edit
      editMode.value = !editMode.value;
    } else if (rootChildren.value.length > 0) {
      // Flow mode → снять позиции с DOM-элементов
      await nextTick();
      const cr = containerRef.value?.getBoundingClientRect();
      if (cr) {
        const scrollTop = containerRef.value?.scrollTop ?? 0;
        rootChildren.value.forEach((child, i) => {
          const el = flowRefs[i];
          if (!el) return;
          const r = el.getBoundingClientRect();
          const y = snapGrid(Math.max(0, r.top - cr.top + scrollTop));
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
    }
  } else {
    editMode.value = !editMode.value;
  }
}

async function resetLayout() {
  // Сбрасываем вкладку к версии из бандла софта: удаляем локальную копию её
  // UI-config из проекта (если проект сделан в старой версии), затем чистим
  // позиции канваса и перечитываем дерево вкладок из бандла.
  try {
    await invoke("project_reset_tab_config", { tabId: props.tab.id });
  } catch (e) {
    console.warn("[layout] reset tab config:", e);
  }
  reset();
  editMode.value = false;
  panelsEpoch.value += 1;
}

// ── Drag / swap ───────────────────────────────────────────────
let activeDragKey: string | null = null;
let swapSource: { x: number; y: number } | null = null;

function onDragStart(key: string) {
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
    if (cx >= otherRect.x && cx < otherRect.x + otherRect.w &&
        cy >= otherRect.y && cy < otherRect.y + otherRect.h) {
      setRect(otherId, { ...otherRect, x: swapSource.x, y: swapSource.y });
      swapSource = { x: otherRect.x, y: otherRect.y };
      break;
    }
  }
}

function onUpdateRect(key: string, rect: CanvasItemRect) {
  setRect(key, rect);
  const item = allItems.value.find(it => it.key === key);
  if (activeDragKey === key && swapSource && item && !isFloating(item.child)) {
    checkAndSwap(key, rect);
  }
}

function onCommit(key: string) {
  activeDragKey = null;
  swapSource = null;
  commitRect(key);
}

function onActualHeight(key: string, h: number) {
  setActualHeight(key, h);
}

function onRemoveItem(key: string, isExtra: boolean) {
  if (isExtra) removeExtraInstance(key);
  else hideInstance(key);
}

// ── Context menu ──────────────────────────────────────────────
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

let loaded = false;
if (!loaded) { loaded = true; void load(); }

// Экстра-инстансы не в YAML-дереве — регистрируем как nav-extension.
// basePath `tab/${id}/extra` + childPath(basePath, _, child) = `tab/${id}/extra/${child.id}`,
// что совпадает с path в шаблоне для extra-items.
const extraNavKey = `tab/${props.tab.id}/extra`;
watch(
  () => stored.value.extra ?? [],
  (extras) => {
    setNavExtension(extraNavKey,
      extras.length ? { type: 'composite', id: 'canvas-extra', children: extras as unknown as ComponentInstance[] } : null
    );
  },
  { immediate: true }
);
onUnmounted(() => { setNavExtension(extraNavKey, null); });
</script>

<template>
  <!-- ── Flow mode ── -->
  <div v-if="!showCanvas" ref="containerRef" class="tcl-flow">
    <div
      v-for="(child, i) in rootChildren"
      :key="childKey(child, i)"
      :ref="(el) => setFlowRef(i, el)"
    >
      <ComponentHost
        :instance="child"
        :path="makeChildPath(`tab/${tab.id}`, i, child)"
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
    v-else-if="showCanvas"
    ref="containerRef"
    class="tcl-canvas"
    :class="{ 'tcl-canvas--edit': editMode }"
    :style="{ minHeight: `${canvasMinH}px` }"
    @contextmenu.self="onCanvasContextMenu"
  >
    <CanvasWindow
      v-for="item in allItems"
      :key="item.key"
      :id="item.key"
      :rect="displayRect(item.key, item.child)"
      :stored-h="storedHFor(item.key, item.child)"
      :edit-mode="editMode"
      :locked="Boolean(item.child.layout?.locked)"
      :min-w="item.child.layout?.minW"
      :min-h="item.child.layout?.minH"
      :removable="editMode"
      @drag-start="onDragStart(item.key)"
      @update:rect="onUpdateRect(item.key, $event)"
      @commit="onCommit(item.key)"
      @actual-height="onActualHeight(item.key, $event)"
      @activate="bringToFront(item.key)"
      @remove="onRemoveItem(item.key, item.isExtra)"
      @contextmenu="onComponentContextMenu(item.key, item, $event)"
    >
      <ComponentHost
        :instance="item.child"
        :path="item.path"
        @select-path="emit('select-path', $event)"
        @activate-path="emit('activate-path', $event)"
      />
    </CanvasWindow>

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

    <!-- Подсказка для пустого кастомного таба -->
    <div v-if="props.tab.isCustom && allItems.length === 0" class="tcl-empty-hint">
      Правый клик по канвасу — добавить компонент
    </div>

    <div class="tcl-fab-row">
      <button v-if="editMode" class="tcl-reset" @click="resetLayout">Сброс</button>
      <button class="tcl-fab" :class="{ 'tcl-fab--edit': editMode }" @click="toggleLayout">
        <svg viewBox="0 0 16 16" fill="none" class="tcl-fab-icon">
          <rect x="1" y="1" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
          <rect x="9" y="1" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
          <rect x="1" y="9" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
          <rect x="9" y="9" width="6" height="6" rx="1" stroke="currentColor" stroke-width="1.3"/>
        </svg>
        {{ editMode ? "Готово" : "Layout" }}
      </button>
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
  height: auto;
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

.tcl-empty-hint {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  font-size: 0.82rem;
  color: var(--color-text-subtle);
  text-align: center;
  pointer-events: none;
  user-select: none;
}
</style>

