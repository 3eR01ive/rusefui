<script setup lang="ts">
import { computed, nextTick, onMounted, onBeforeUnmount, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { ComponentInstance, DataBinding, ResolvedTab } from "../core/types";
import { childPath as makeChildPath } from "../core/instance";
import { useCanvasLayout, snapGrid } from "../composables/useCanvasLayout";
import type { CanvasItemRect } from "../composables/useCanvasLayout";
import { listRegisteredComponents } from "../core/registry";
import ComponentHost from "./ComponentHost.vue";
import CanvasWindow from "./canvas/CanvasWindow.vue";

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

// Кастомный таб всегда показывает canvas-режим (даже без layout)
const showCanvas = computed(() => hasLayout.value || Boolean(props.tab.isCustom));

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
  addExtraInstance, hideInstance, removeExtraInstance,
} = useCanvasLayout(`tab-${props.tab.id}`);

const hasLayout = computed(() => Object.keys(stored.value.items).length > 0);

// ── Addable types ─────────────────────────────────────────────
const EXCLUDE_FROM_MENU = new Set(['stack', 'row', 'section', 'composite', 'canvas']);
const menuTypes = listRegisteredComponents().filter(m => !EXCLUDE_FROM_MENU.has(m.type));

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

const CANVAS_PAD = 80;
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
    if (rootChildren.value.length > 0) {
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
    }
    editMode.value = true;
  } else {
    editMode.value = !editMode.value;
  }
}

// Для кастомных табов сразу входим в edit-режим после загрузки layout
watch(
  () => stored.value,
  () => {
    if (props.tab.isCustom && !editMode.value) editMode.value = true;
  },
  { immediate: true },
);

function resetLayout() { reset(); editMode.value = false; }

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
interface ConfigFieldEntry { name: string; units?: string; ty: string; }

interface OutputFieldEntry { name: string; units?: string; kind: string; }

interface CtxState {
  menuX: number;
  menuY: number;
  canvasX: number;
  canvasY: number;
  stage: 'types' | 'table' | 'curve' | 'field' | 'output-field';
  selectedType: string | null;
  tables: Array<{ id: string; title: string; zBins: string; xBins?: string; yBins?: string }>;
  tableFilter: string;
  curves: Array<{ id: string; title: string; xBins: string; yBins: string }>;
  curveFilter: string;
  configFields: ConfigFieldEntry[];
  fieldFilter: string;
  outputFields: OutputFieldEntry[];
  outputFilter: string;
  loading: boolean;
}

/** Какой ty нужен для данного типа компонента */
const CONFIG_FIELD_TYPE: Record<string, string> = {
  'scalar-field': 'scalar',
  'string-field': 'string',
  'enum-field': 'enum',
};

const ctx = ref<CtxState | null>(null);

const filteredConfigFields = computed<ConfigFieldEntry[]>(() => {
  if (!ctx.value || ctx.value.stage !== 'field') return [];
  const q = ctx.value.fieldFilter.toLowerCase();
  return q
    ? ctx.value.configFields.filter(f => f.name.toLowerCase().includes(q) || (f.units ?? '').toLowerCase().includes(q))
    : ctx.value.configFields;
});

const filteredTables = computed(() => {
  if (!ctx.value || ctx.value.stage !== 'table') return [];
  const q = ctx.value.tableFilter.toLowerCase();
  return q ? ctx.value.tables.filter(t => t.title.toLowerCase().includes(q) || t.id.toLowerCase().includes(q)) : ctx.value.tables;
});

const filteredCurves = computed(() => {
  if (!ctx.value || ctx.value.stage !== 'curve') return [];
  const q = ctx.value.curveFilter.toLowerCase();
  return q ? ctx.value.curves.filter(c => c.title.toLowerCase().includes(q) || c.id.toLowerCase().includes(q)) : ctx.value.curves;
});

const filteredOutputFields = computed<OutputFieldEntry[]>(() => {
  if (!ctx.value || ctx.value.stage !== 'output-field') return [];
  const q = ctx.value.outputFilter.toLowerCase();
  return q
    ? ctx.value.outputFields.filter(f => f.name.toLowerCase().includes(q) || (f.units ?? '').toLowerCase().includes(q))
    : ctx.value.outputFields;
});

function onCanvasContextMenu(e: MouseEvent) {
  if (!editMode.value) return;
  e.preventDefault();
  const cr = containerRef.value?.getBoundingClientRect();
  if (!cr) return;
  const scrollTop = containerRef.value?.scrollTop ?? 0;

  const menuW = 240;
  const menuH = Math.min(460, menuTypes.length * 32 + 48);
  const x = e.clientX + menuW > window.innerWidth ? e.clientX - menuW : e.clientX;
  const y = e.clientY + menuH > window.innerHeight ? e.clientY - menuH : e.clientY;

  ctx.value = {
    menuX: x, menuY: y,
    canvasX: snapGrid(Math.max(0, e.clientX - cr.left)),
    canvasY: snapGrid(Math.max(0, e.clientY - cr.top + scrollTop)),
    stage: 'types', selectedType: null,
    tables: [], tableFilter: '',
    curves: [], curveFilter: '',
    configFields: [], fieldFilter: '',
    outputFields: [], outputFilter: '',
    loading: false,
  };
}

async function onSelectType(type: string) {
  if (!ctx.value) return;
  const meta = menuTypes.find(m => m.type === type);
  const bm = meta?.bindMeta;

  if (!bm || (bm.autoSource && !bm.needsTable && !bm.needsCurve && !bm.needsConfigField)) {
    // Нет bind или только autoSource → добавляем сразу
    const bind: DataBinding | undefined = bm?.autoSource
      ? { source: bm.autoSource }
      : undefined;
    addExtraInstance({ type, bind }, { x: ctx.value.canvasX, y: ctx.value.canvasY });
    ctx.value = null;
    return;
  }

  ctx.value.selectedType = type;

  if (bm.needsTable) {
    ctx.value.loading = true;
    ctx.value.stage = 'table';
    try {
      ctx.value.tables = await invoke<CtxState['tables']>('ini_list_tables');
    } finally {
      ctx.value.loading = false;
    }
    return;
  }

  if (bm.needsCurve) {
    ctx.value.loading = true;
    ctx.value.stage = 'curve';
    try {
      ctx.value.curves = await invoke<CtxState['curves']>('ini_list_curves');
    } finally {
      ctx.value.loading = false;
    }
    return;
  }

  if (bm.needsConfigField) {
    ctx.value.loading = true;
    ctx.value.stage = 'field';
    ctx.value.fieldFilter = '';
    ctx.value.configFields = [];
    try {
      type RawField = { name: string; units?: string; ty: string };
      const all = await invoke<RawField[]>('config_list_fields');
      const needTy = CONFIG_FIELD_TYPE[type] ?? '';
      ctx.value.configFields = needTy ? all.filter(f => f.ty === needTy) : all;
    } finally {
      ctx.value.loading = false;
    }
    return;
  }

  if (bm.needsOutputField) {
    ctx.value.loading = true;
    ctx.value.stage = 'output-field';
    ctx.value.outputFilter = '';
    ctx.value.outputFields = [];
    try {
      ctx.value.outputFields = await invoke<OutputFieldEntry[]>('output_list_fields');
    } finally {
      ctx.value.loading = false;
    }
  }
}

function onSelectTable(t: CtxState['tables'][0]) {
  if (!ctx.value?.selectedType) return;
  addExtraInstance({
    type: ctx.value.selectedType,
    bind: {
      source: 'config',
      params: { zBins: t.zBins, xBins: t.xBins, yBins: t.yBins },
    },
  }, { x: ctx.value.canvasX, y: ctx.value.canvasY });
  ctx.value = null;
}

function onSelectCurve(c: CtxState['curves'][0]) {
  if (!ctx.value?.selectedType) return;
  addExtraInstance({
    type: ctx.value.selectedType,
    bind: {
      source: 'config',
      params: { xBins: c.xBins, yBins: c.yBins },
    },
  }, { x: ctx.value.canvasX, y: ctx.value.canvasY });
  ctx.value = null;
}

function onSelectConfigField(name: string) {
  if (!ctx.value?.selectedType) return;
  const bm = menuTypes.find(m => m.type === ctx.value!.selectedType)?.bindMeta;
  addExtraInstance({
    type: ctx.value.selectedType,
    bind: { source: bm?.autoSource ?? 'config', field: name },
  }, { x: ctx.value.canvasX, y: ctx.value.canvasY });
  ctx.value = null;
}

function onSelectOutputField(name: string) {
  if (!ctx.value?.selectedType) return;
  const bm = menuTypes.find(m => m.type === ctx.value!.selectedType)?.bindMeta;
  addExtraInstance({
    type: ctx.value.selectedType,
    bind: { source: bm?.autoSource ?? 'outputChannels', field: name },
  }, { x: ctx.value.canvasX, y: ctx.value.canvasY });
  ctx.value = null;
}

function ctxBack() {
  if (ctx.value) { ctx.value.stage = 'types'; ctx.value.selectedType = null; }
}

function onDocPointerDown(e: PointerEvent) {
  if (!ctx.value) return;
  const menu = document.querySelector('.tcl-ctx-menu');
  if (menu && menu.contains(e.target as Node)) return;
  ctx.value = null;
}

function onDocKeydown(e: KeyboardEvent) {
  if (!ctx.value) return;
  if (e.key === 'Escape') {
    if (ctx.value.stage !== 'types') ctxBack();
    else ctx.value = null;
  }
}

onMounted(() => {
  document.addEventListener('pointerdown', onDocPointerDown, true);
  document.addEventListener('keydown', onDocKeydown, true);
});
onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onDocPointerDown, true);
  document.removeEventListener('keydown', onDocKeydown, true);
});

let loaded = false;
if (!loaded) { loaded = true; void load(); }
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
    :style="editMode ? { minHeight: `${canvasMinH}px` } : undefined"
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
    >
      <ComponentHost
        :instance="item.child"
        :path="item.path"
        @select-path="emit('select-path', $event)"
        @activate-path="emit('activate-path', $event)"
      />
    </CanvasWindow>

    <!-- Context menu (teleport, чтобы не обрезался overflow) -->
    <Teleport to="body">
      <div
        v-if="ctx"
        class="tcl-ctx-menu"
        :style="{ left: `${ctx.menuX}px`, top: `${ctx.menuY}px` }"
        @contextmenu.prevent
      >
        <!-- Stage: список типов -->
        <template v-if="ctx.stage === 'types'">
          <div class="tcl-ctx-header">Добавить компонент</div>
          <div class="tcl-ctx-scroll">
            <button
              v-for="m in menuTypes"
              :key="m.type"
              type="button"
              class="tcl-ctx-item"
              :class="{ 'tcl-ctx-item--has-bind': m.bindMeta?.needsTable || m.bindMeta?.needsCurve || m.bindMeta?.needsConfigField }"
              @pointerdown.stop
              @click="onSelectType(m.type)"
            >
              <span class="tcl-ctx-item-label">{{ m.label }}</span>
              <span
                v-if="m.bindMeta?.needsTable || m.bindMeta?.needsCurve || m.bindMeta?.needsConfigField"
                class="tcl-ctx-item-arrow"
              >›</span>
            </button>
          </div>
        </template>

        <!-- Stage: выбор таблицы -->
        <template v-else-if="ctx.stage === 'table'">
          <div class="tcl-ctx-header tcl-ctx-header--nav">
            <button class="tcl-ctx-back" @pointerdown.stop @click="ctxBack">‹</button>
            Выберите таблицу
          </div>
          <div v-if="ctx.loading" class="tcl-ctx-hint">Загрузка…</div>
          <template v-else-if="ctx.tables.length">
            <div class="tcl-ctx-field-search" @pointerdown.stop>
              <input v-model="ctx.tableFilter" class="tcl-ctx-field-input" placeholder="Поиск…" autofocus @keydown.stop />
            </div>
            <div v-if="!filteredTables.length" class="tcl-ctx-hint">Нет совпадений</div>
            <div v-else class="tcl-ctx-scroll">
              <button
                v-for="t in filteredTables"
                :key="t.id"
                type="button"
                class="tcl-ctx-item"
                @pointerdown.stop
                @click="onSelectTable(t)"
              >{{ t.title }}</button>
            </div>
          </template>
          <div v-else class="tcl-ctx-hint">INI не загружен или таблиц нет</div>
        </template>

        <!-- Stage: выбор кривой -->
        <template v-else-if="ctx.stage === 'curve'">
          <div class="tcl-ctx-header tcl-ctx-header--nav">
            <button class="tcl-ctx-back" @pointerdown.stop @click="ctxBack">‹</button>
            Выберите кривую
          </div>
          <div v-if="ctx.loading" class="tcl-ctx-hint">Загрузка…</div>
          <template v-else-if="ctx.curves.length">
            <div class="tcl-ctx-field-search" @pointerdown.stop>
              <input v-model="ctx.curveFilter" class="tcl-ctx-field-input" placeholder="Поиск…" autofocus @keydown.stop />
            </div>
            <div v-if="!filteredCurves.length" class="tcl-ctx-hint">Нет совпадений</div>
            <div v-else class="tcl-ctx-scroll">
              <button
                v-for="c in filteredCurves"
                :key="c.id"
                type="button"
                class="tcl-ctx-item"
                @pointerdown.stop
                @click="onSelectCurve(c)"
              >{{ c.title }}</button>
            </div>
          </template>
          <div v-else class="tcl-ctx-hint">INI не загружен или кривых нет</div>
        </template>

        <!-- Stage: выбор поля конфига -->
        <template v-else-if="ctx.stage === 'field'">
          <div class="tcl-ctx-header tcl-ctx-header--nav">
            <button class="tcl-ctx-back" @pointerdown.stop @click="ctxBack">‹</button>
            Выберите параметр
          </div>
          <div class="tcl-ctx-field-search" @pointerdown.stop>
            <input
              v-model="ctx.fieldFilter"
              class="tcl-ctx-field-input"
              placeholder="Поиск…"
              autofocus
              @keydown.stop
            />
          </div>
          <div v-if="ctx.loading" class="tcl-ctx-hint">Загрузка…</div>
          <div v-else-if="!ctx.configFields.length" class="tcl-ctx-hint">INI не загружен или параметров нет</div>
          <div v-else-if="!filteredConfigFields.length" class="tcl-ctx-hint">Нет совпадений</div>
          <div v-else class="tcl-ctx-scroll">
            <button
              v-for="f in filteredConfigFields"
              :key="f.name"
              type="button"
              class="tcl-ctx-item"
              @pointerdown.stop
              @click="onSelectConfigField(f.name)"
            >
              <span class="tcl-ctx-item-label">{{ f.name }}</span>
              <span v-if="f.units" class="tcl-ctx-item-units">{{ f.units }}</span>
            </button>
          </div>
        </template>

        <!-- Stage: выбор output-канала -->
        <template v-else-if="ctx.stage === 'output-field'">
          <div class="tcl-ctx-header tcl-ctx-header--nav">
            <button class="tcl-ctx-back" @pointerdown.stop @click="ctxBack">‹</button>
            Выберите канал
          </div>
          <div class="tcl-ctx-field-search" @pointerdown.stop>
            <input
              v-model="ctx.outputFilter"
              class="tcl-ctx-field-input"
              placeholder="Поиск…"
              autofocus
              @keydown.stop
            />
          </div>
          <div v-if="ctx.loading" class="tcl-ctx-hint">Загрузка…</div>
          <div v-else-if="!ctx.outputFields.length" class="tcl-ctx-hint">INI не загружен</div>
          <div v-else-if="!filteredOutputFields.length" class="tcl-ctx-hint">Нет совпадений</div>
          <div v-else class="tcl-ctx-scroll">
            <button
              v-for="f in filteredOutputFields"
              :key="f.name"
              type="button"
              class="tcl-ctx-item"
              @pointerdown.stop
              @click="onSelectOutputField(f.name)"
            >
              <span class="tcl-ctx-item-label">{{ f.name }}</span>
              <span v-if="f.units" class="tcl-ctx-item-units">{{ f.units }}</span>
            </button>
          </div>
        </template>
      </div>
    </Teleport>

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

<style>
.tcl-ctx-menu {
  position: fixed;
  z-index: 9999;
  width: 240px;
  max-height: 460px;
  display: flex;
  flex-direction: column;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  box-shadow: 0 6px 24px rgba(0,0,0,.3);
  overflow: hidden;
}
.tcl-ctx-header {
  padding: 0.45rem 0.75rem 0.35rem;
  font-size: 0.7rem;
  font-weight: 600;
  color: var(--color-text-subtle);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}
.tcl-ctx-header--nav {
  display: flex;
  align-items: center;
  gap: 0.35rem;
}
.tcl-ctx-back {
  padding: 0 0.3rem;
  font-size: 1rem;
  line-height: 1;
  background: none;
  border: none;
  color: var(--color-text-muted);
  cursor: pointer;
  flex-shrink: 0;
}
.tcl-ctx-back:hover { color: var(--color-text); }
.tcl-ctx-scroll {
  overflow-y: auto;
  flex: 1;
  padding: 0.2rem 0;
}
.tcl-ctx-item {
  display: flex;
  align-items: center;
  width: 100%;
  padding: 0.36rem 0.75rem;
  text-align: left;
  font-size: 0.82rem;
  color: var(--color-text);
  background: transparent;
  border: none;
  cursor: pointer;
  gap: 0.3rem;
}
.tcl-ctx-item:hover {
  background: color-mix(in srgb, var(--color-accent, #3b82f6) 10%, var(--color-bg-elevated));
  color: var(--color-accent, #3b82f6);
}
.tcl-ctx-item-label { flex: 1; }
.tcl-ctx-item-arrow {
  font-size: 1rem;
  color: var(--color-text-subtle);
  flex-shrink: 0;
}
.tcl-ctx-hint {
  padding: 0.6rem 0.75rem;
  font-size: 0.78rem;
  color: var(--color-text-subtle);
  flex-shrink: 0;
}
.tcl-ctx-field-search {
  padding: 0.4rem 0.6rem;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}
.tcl-ctx-field-input {
  width: 100%;
  box-sizing: border-box;
  padding: 0.3rem 0.5rem;
  font-size: 0.82rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: var(--color-bg);
  color: var(--color-text);
  outline: none;
}
.tcl-ctx-field-input:focus { border-color: var(--color-accent, #3b82f6); }
.tcl-ctx-item-units {
  font-size: 0.72rem;
  color: var(--color-text-subtle);
  flex-shrink: 0;
  margin-left: auto;
  padding-left: 0.4rem;
}
</style>
