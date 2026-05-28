<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useRustComponent } from "../../composables/useRustComponent";
import { useInstanceBind } from "../../composables/useInstanceBind";
import { initConfig, useConfig } from "../../composables/useConfig";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

void initConfig();

const instanceRef = computed(() => props.instance);
const propsRef = computed(() => props.props);
const bindingRef = computed(() => props.binding);
const { paramString, bind } = useInstanceBind(instanceRef, bindingRef);
const { snapshot: configSnapshot } = useConfig();

function numericProp(name: string): number | undefined {
  const raw = propsRef.value[name];
  if (typeof raw === "number" && Number.isFinite(raw)) return raw;
  if (typeof raw === "string") {
    const parsed = Number(raw.replace(",", ".").trim());
    if (Number.isFinite(parsed)) return parsed;
  }
  return undefined;
}

function buildBindPayload(): Record<string, unknown> {
  const nudgeStep = numericProp("nudgeStep");
  return {
    title: propsRef.value.title,
    xLabel: propsRef.value.xLabel,
    yLabel: propsRef.value.yLabel,
    xBins: paramString("xBins") ?? "",
    yBins: paramString("yBins") ?? "",
    zBins: paramString("zBins") ?? "",
    ...(nudgeStep !== undefined ? { nudgeStep } : {}),
  };
}

const { state, dispatch, ready, error } = useRustComponent(
  props.instance,
  props.path,
  buildBindPayload,
);

const gridRef = ref<HTMLElement | null>(null);
const isMouseSelecting = ref(false);

const title = computed(() => String(state.value.title ?? propsRef.value.title ?? ""));
const xLabel = computed(() => String(state.value.xLabel ?? propsRef.value.xLabel ?? "X"));
const yLabel = computed(() => String(state.value.yLabel ?? propsRef.value.yLabel ?? "Y"));
const xValues = computed(() => (state.value.xValues as number[] | undefined) ?? []);
const yValues = computed(() => (state.value.yValues as number[] | undefined) ?? []);
const grid = computed(() => state.value.grid as TableGridView | undefined);
const cells = computed(() => grid.value?.cells ?? []);
const colIndices = computed(() =>
  Array.from({ length: grid.value?.cols ?? 0 }, (_, i) => i),
);
const rowIndices = computed(() =>
  Array.from({ length: grid.value?.rows ?? 0 }, (_, i) => i),
);

const disabled = computed(() => !state.value.canEdit);
const statusText = computed(() => String(state.value.statusText ?? ""));
const localError = computed(() =>
  state.value.localError ? String(state.value.localError) : error.value,
);

interface TableCellView {
  row: number;
  col: number;
  value: number;
  display: string;
  heatBg: string;
  selected: boolean;
  cursor: boolean;
  corner: boolean;
}

interface TableGridView {
  rows: number;
  cols: number;
  cells: TableCellView[];
  selection?: {
    r0: number;
    r1: number;
    c0: number;
    c1: number;
  };
}

function cellAt(row: number, col: number): TableCellView | undefined {
  return cells.value.find((c) => c.row === row && c.col === col);
}

const selectionRect = computed(() => grid.value?.selection);

function isSelectionEdge(
  row: number,
  col: number,
  edge: "top" | "bottom" | "left" | "right",
): boolean {
  const sel = selectionRect.value;
  if (!sel) return false;
  if (row < sel.r0 || row > sel.r1 || col < sel.c0 || col > sel.c1) return false;
  if (edge === "top") return row === sel.r0;
  if (edge === "bottom") return row === sel.r1;
  if (edge === "left") return col === sel.c0;
  return col === sel.c1;
}

function fmtAxis(v: number | undefined, fallback: number): string {
  if (v === undefined || !Number.isFinite(v)) return String(fallback);
  if (Number.isInteger(v)) return String(v);
  const s = v.toFixed(3);
  return s.replace(/\.?0+$/, "");
}

function bindFields() {
  if (!ready.value) return;
  const payload = buildBindPayload();
  if (!payload.zBins) {
    console.warn("[config-table] bind.params.zBins не задан", bind.value);
    return;
  }
  void dispatch("set_bind", payload);
}

watch(ready, (v) => {
  if (v) bindFields();
});

watch(
  () => bind.value?.params,
  () => {
    if (ready.value) bindFields();
  },
  { deep: true },
);

watch(
  () =>
    [
      configSnapshot.value.loaded,
      configSnapshot.value.loading,
      configSnapshot.value.readOnly,
      configSnapshot.value.rawLen,
    ] as const,
  ([loaded, , readOnly, rawLen], prev) => {
    if (!loaded || !ready.value) return;
    const changed =
      !prev ||
      prev[0] !== loaded ||
      prev[2] !== readOnly ||
      prev[3] !== rawLen;
    if (changed) void dispatch("reload");
  },
);

function onGridKeydown(e: KeyboardEvent) {
  if (!ready.value) return;
  const key = e.key;
  const code = e.code;
  const isInterpolate = e.ctrlKey && code === "KeyI";
  const isTypeChar =
    (!e.ctrlKey && !e.metaKey && !e.altKey && /^[0-9]$/.test(key)) ||
    code === "NumpadDecimal" ||
    code === "NumpadSubtract" ||
    key === "." ||
    key === "," ||
    key === "-";
  const isTypeControl = key === "Backspace" || key === "Enter" || key === "Escape";
  if (
    !["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(key) &&
    !isInterpolate &&
    !isTypeChar &&
    !isTypeControl
  ) {
    return;
  }
  e.preventDefault();
  if (isTypeChar || isTypeControl) {
    if (isTypeControl) {
      const kind = key === "Backspace" ? "backspace" : key === "Enter" ? "commit" : "cancel";
      void dispatch("type_key", { kind });
      return;
    }
    let ch = key;
    if (code === "NumpadDecimal" || key === "," || key === ".") ch = ".";
    if (code === "NumpadSubtract" || key === "-") ch = "-";
    void dispatch("type_key", { kind: "char", ch });
    return;
  }
  if (isInterpolate) {
    void dispatch("interpolate");
    return;
  }
  void dispatch("keydown", {
    key,
    shift: e.shiftKey,
    ctrl: e.ctrlKey,
  });
}

function onCellMouseDown(row: number, col: number, e: MouseEvent) {
  if (!ready.value) return;
  if (e.button !== 0) return;
  const isInputTarget = e.target instanceof HTMLInputElement;
  if (!isInputTarget) {
    e.preventDefault();
  }
  isMouseSelecting.value = true;
  void dispatch("select_cell", { row, col, extend: e.shiftKey });
  if (!isInputTarget) {
    gridRef.value?.focus();
  }
}

function onCellMouseEnter(row: number, col: number) {
  if (!ready.value || !isMouseSelecting.value) return;
  void dispatch("select_cell", { row, col, extend: true });
}

function onGlobalMouseUp() {
  isMouseSelecting.value = false;
}

if (typeof window !== "undefined") {
  window.addEventListener("mouseup", onGlobalMouseUp);
}

onBeforeUnmount(() => {
  if (typeof window !== "undefined") {
    window.removeEventListener("mouseup", onGlobalMouseUp);
  }
});

function onCellFocus(row: number, col: number, e: FocusEvent) {
  void dispatch("select_cell", { row, col });
  const input = e.target as HTMLInputElement;
  // Без выделения текста при фокусе (убираем "синюю подсветку" браузера).
  const end = input.value.length;
  input.setSelectionRange(end, end);
}

</script>

<template>
  <div class="config-table">
    <header v-if="title" class="grid-head">
      <h4 class="grid-title">{{ title }}</h4>
      <div class="grid-head-actions">
        <button
          type="button"
          class="btn-interp"
          :disabled="disabled"
          title="Интерполировать (Ctrl+I)"
          @click="dispatch('interpolate')"
        >
          Интерполировать
        </button>
        <span class="grid-badge" :class="{ 'grid-badge--error': !!localError }">
          {{ localError || statusText }}
        </span>
      </div>
    </header>

    <div
      ref="gridRef"
      class="grid-scroll"
      tabindex="0"
      @keydown="onGridKeydown"
    >
      <table class="grid">
        <thead>
          <tr>
            <th class="corner">{{ yLabel }} \ {{ xLabel }}</th>
            <th v-for="col in colIndices" :key="`x-${col}`" class="axis-head">
              {{ fmtAxis(xValues[col], col) }}
            </th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in rowIndices" :key="`row-${row}`">
            <th class="axis-head">{{ fmtAxis(yValues[row], row) }}</th>
            <td
              v-for="col in colIndices"
              :key="`c-${row}-${col}`"
              class="cell-td"
              :class="{
                'cell-td--selected': cellAt(row, col)?.selected,
                'cell-td--cursor': cellAt(row, col)?.cursor,
                'cell-td--corner': cellAt(row, col)?.corner,
                'cell-td--sel-top': isSelectionEdge(row, col, 'top'),
                'cell-td--sel-bottom': isSelectionEdge(row, col, 'bottom'),
                'cell-td--sel-left': isSelectionEdge(row, col, 'left'),
                'cell-td--sel-right': isSelectionEdge(row, col, 'right'),
              }"
              :style="{ background: cellAt(row, col)?.heatBg }"
              @mousedown="onCellMouseDown(row, col, $event)"
              @mouseenter="onCellMouseEnter(row, col)"
            >
              <input
                type="text"
                class="cell-input"
                :disabled="disabled"
                spellcheck="false"
                autocomplete="off"
                readonly
                tabindex="-1"
                :value="cellAt(row, col)?.display ?? ''"
                @focus="onCellFocus(row, col, $event)"
              />
            </td>
          </tr>
        </tbody>
      </table>
    </div>
    <p class="grid-hint">
      ↑↓←→ — курсор · Shift+стрелки — выделение · Ctrl+↑↓ — ±шаг · Ctrl+I — интерполяция
    </p>
  </div>
</template>

<style scoped>
.config-table {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  width: 100%;
  min-width: 0;
}

.grid-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 0.75rem;
}

.grid-head-actions {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.grid-title {
  margin: 0;
  font-size: 0.92rem;
  font-weight: 600;
}

.btn-interp {
  font-size: 0.72rem;
  padding: 0.2rem 0.5rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: var(--color-bg-muted);
  color: var(--color-text);
  cursor: pointer;
}

.btn-interp:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.grid-badge {
  font-size: 0.7rem;
  color: var(--color-text-subtle);
}

.grid-badge--error {
  color: var(--color-danger, #c0392b);
}

.grid-scroll {
  overflow: auto;
  max-width: 100%;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  outline: none;
}

.grid-scroll:focus-visible {
  box-shadow: 0 0 0 2px var(--color-accent, #3b82f6);
}

.grid {
  border-collapse: collapse;
  font-size: 0.78rem;
  min-width: 100%;
}

.grid th,
.grid td {
  border: 1px solid var(--color-border);
  padding: 0;
}

.axis-head,
.corner {
  background: var(--color-bg-muted);
  color: var(--color-text-muted);
  font-weight: 500;
  padding: 0.35rem 0.5rem;
  white-space: nowrap;
}

.corner {
  position: sticky;
  left: 0;
  z-index: 1;
}

.cell-td--selected {
  background-image: linear-gradient(
    rgba(59, 130, 246, 0.08),
    rgba(59, 130, 246, 0.08)
  );
}

.cell-td--sel-top {
  border-top: 2px solid var(--color-accent, #3b82f6) !important;
}

.cell-td--sel-bottom {
  border-bottom: 2px solid var(--color-accent, #3b82f6) !important;
}

.cell-td--sel-left {
  border-left: 2px solid var(--color-accent, #3b82f6) !important;
}

.cell-td--sel-right {
  border-right: 2px solid var(--color-accent, #3b82f6) !important;
}

.cell-td--corner .cell-input {
  font-weight: 600;
}

.cell-input {
  width: 4.5rem;
  padding: 0.35rem 0.45rem;
  border: none;
  background: transparent;
  color: var(--color-text);
  text-shadow: 0 0 2px rgba(255, 255, 255, 0.95);
  text-align: right;
  user-select: none;
  -webkit-user-select: none;
  pointer-events: none;
  -webkit-tap-highlight-color: transparent;
}

.cell-input:focus {
  outline: none !important;
  box-shadow: none !important;
}

.cell-input:focus-visible {
  outline: none !important;
  box-shadow: none !important;
}

.cell-input::selection {
  background: transparent;
}

.cell-input::-moz-selection {
  background: transparent;
}


.cell-input:disabled {
  color: var(--color-text-muted);
}

.grid-hint {
  margin: 0;
  font-size: 0.68rem;
  color: var(--color-text-subtle);
}
</style>
