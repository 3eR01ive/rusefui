<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useRustComponent } from "../../composables/useRustComponent";
import { useInstanceBind } from "../../composables/useInstanceBind";
import { initConfig, useConfig } from "../../composables/useConfig";
import { dispatchConfigTableWithHistory } from "../../composables/configTableDispatch";
import { readClipboardText, writeClipboardText } from "../../composables/clipboardText";
import { useComponentBinding } from "../../composables/useKeyboardRouter";
import { initOutputChannels, useOutputChannels } from "../../composables/useOutputChannels";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

void initConfig();
void initOutputChannels();

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
  const xOutputChannel = paramString("xOutputChannel");
  const yOutputChannel = paramString("yOutputChannel");
  return {
    title: propsRef.value.title,
    xLabel: propsRef.value.xLabel,
    yLabel: propsRef.value.yLabel,
    xBins: paramString("xBins") ?? "",
    yBins: paramString("yBins") ?? "",
    zBins: paramString("zBins") ?? "",
    ...(nudgeStep !== undefined ? { nudgeStep } : {}),
    ...(xOutputChannel !== undefined ? { xOutputChannel } : {}),
    ...(yOutputChannel !== undefined ? { yOutputChannel } : {}),
  };
}

const { state, dispatch, ready, error } = useRustComponent(
  props.instance,
  props.path,
  buildBindPayload,
);

const zField = computed(() => paramString("zBins") ?? "");
const xField = computed(() => paramString("xBins") ?? "");
const yField = computed(() => paramString("yBins") ?? "");
const title = computed(() => String(state.value.title ?? propsRef.value.title ?? ""));

type EditFocus = "grid" | "x" | "y";

function parseEditFocus(raw: unknown): EditFocus {
  if (raw === "grid" || raw === "x" || raw === "y") return raw;
  const s = String(raw ?? "grid").toLowerCase();
  if (s === "x") return "x";
  if (s === "y") return "y";
  return "grid";
}

async function dispatchWrite(
  action: string,
  payload: Record<string, unknown> = {},
): Promise<void> {
  await dispatchConfigTableWithHistory(
    dispatch,
    () => state.value,
    zField.value,
    title.value,
    action,
    payload,
    { xField: xField.value || undefined, yField: yField.value || undefined },
  );
}

function onConfigUndoRedo() {
  if (ready.value) void dispatch("reload");
}

onMounted(() => {
  window.addEventListener("config-undo-redo", onConfigUndoRedo);
  window.addEventListener("mouseup", onGlobalMouseUp);
});

onBeforeUnmount(() => {
  window.removeEventListener("config-undo-redo", onConfigUndoRedo);
  window.removeEventListener("mouseup", onGlobalMouseUp);
});

const gridRef = ref<HTMLElement | null>(null);
const isMouseSelecting = ref(false);
const clipboardError = ref("");

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
const editBuffer = computed(() => String(state.value.editBuffer ?? ""));
const statusText = computed(() => String(state.value.statusText ?? ""));
const localError = computed(() =>
  clipboardError.value ||
  (state.value.localError ? String(state.value.localError) : error.value),
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
  cursorRow?: number;
  cursorCol?: number;
  selection?: {
    r0: number;
    r1: number;
    c0: number;
    c1: number;
  };
}

interface AxisCellView {
  index: number;
  value: number;
  display: string;
  selected: boolean;
  cursor: boolean;
}

interface AxisBarView {
  cells: AxisCellView[];
  selI0: number;
  selI1: number;
  editable: boolean;
}

const editFocus = computed((): EditFocus => parseEditFocus(state.value.editFocus));
const xAxis = computed(() => state.value.xAxis as AxisBarView | undefined);
const yAxis = computed(() => state.value.yAxis as AxisBarView | undefined);
const canEditX = computed(() => Boolean(state.value.canEditX));
const canEditY = computed(() => Boolean(state.value.canEditY));

// ── Live cell (текущее положение по осям из OutputParams) ────────
const { snapshot: outputSnapshot } = useOutputChannels();
const xOutputChannel = computed(() => state.value.xOutputChannel as string | undefined);
const yOutputChannel = computed(() => state.value.yOutputChannel as string | undefined);

function findLiveIndex(bins: number[], value: number): number {
  if (bins.length === 0) return -1;
  if (bins.length === 1) return 0;
  const desc = bins[bins.length - 1]! < bins[0]!;
  if (desc) {
    // убывающий ряд: переваливаем если value > середины между i и i+1
    for (let i = 0; i < bins.length - 1; i++) {
      if (value > (bins[i]! + bins[i + 1]!) / 2) return i;
    }
    return bins.length - 1;
  }
  // возрастающий ряд: переваливаем если value >= середины между i и i+1
  for (let i = 0; i < bins.length - 1; i++) {
    if (value < (bins[i]! + bins[i + 1]!) / 2) return i;
  }
  return bins.length - 1;
}

const liveCol = computed(() => {
  const ch = xOutputChannel.value;
  if (!ch) return -1;
  const v = outputSnapshot.value.values[ch];
  if (v === undefined) return -1;
  return findLiveIndex(xValues.value, v);
});

const liveRow = computed(() => {
  const ch = yOutputChannel.value;
  if (!ch) return -1;
  const v = outputSnapshot.value.values[ch];
  if (v === undefined) return -1;
  return findLiveIndex(yValues.value, v);
});

function xAxisCell(col: number): AxisCellView | undefined {
  return xAxis.value?.cells.find((c) => c.index === col);
}

function yAxisCell(row: number): AxisCellView | undefined {
  return yAxis.value?.cells.find((c) => c.index === row);
}

function axisDisplay(axis: "x" | "y", index: number, fallback: number): string {
  const cell = axis === "x" ? xAxisCell(index) : yAxisCell(index);
  const isCursor =
    editFocus.value === axis && cell?.cursor === true;
  if (isCursor && editBuffer.value !== "") {
    return editBuffer.value;
  }
  if (cell) return cell.display;
  const vals = axis === "x" ? xValues.value : yValues.value;
  return fmtAxis(vals[index], fallback);
}

function isAxisSelectionEdge(
  axis: "x" | "y",
  index: number,
  edge: "start" | "end",
): boolean {
  if (editFocus.value !== axis) return false;
  const bar = axis === "x" ? xAxis.value : yAxis.value;
  if (!bar) return false;
  if (index < bar.selI0 || index > bar.selI1) return false;
  return edge === "start" ? index === bar.selI0 : index === bar.selI1;
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
  if (editFocus.value !== "grid") return false;
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

watch(
  () =>
    [
      grid.value?.cursorRow,
      grid.value?.cursorCol,
      editFocus.value,
      xAxis.value?.cells.find((c) => c.cursor)?.index,
      yAxis.value?.cells.find((c) => c.cursor)?.index,
    ] as const,
  () => {
    const sel =
      editFocus.value === "x"
        ? ".axis-head--cursor-x"
        : editFocus.value === "y"
          ? ".axis-head--cursor-y"
          : ".cell-td--cursor";
    const el = gridRef.value?.querySelector(sel);
    el?.scrollIntoView({ block: "nearest", inline: "nearest" });
  },
);


async function copySelection(): Promise<void> {
  clipboardError.value = "";
  const next = await dispatch("copy_selection");
  const text = String(next?.copyText ?? state.value.copyText ?? "");
  if (!text) {
    clipboardError.value = "Нечего копировать";
    return;
  }
  try {
    await writeClipboardText(text);
  } catch (e) {
    clipboardError.value = e instanceof Error ? e.message : String(e);
  }
}

async function pasteSelection(): Promise<void> {
  clipboardError.value = "";
  if (editFocus.value === "x" && !canEditX.value) return;
  if (editFocus.value === "y" && !canEditY.value) return;
  if (editFocus.value === "grid" && disabled.value) return;
  let text = "";
  try {
    text = await readClipboardText();
  } catch (e) {
    clipboardError.value = e instanceof Error ? e.message : String(e);
    return;
  }
  if (!text.trim()) {
    clipboardError.value = "Буфер обмена пуст";
    return;
  }
  await dispatchWrite("paste", { text });
}

function onComponentKeydown(e: KeyboardEvent): boolean {
  if (!ready.value) return false;
  const key = e.key;
  const code = e.code;
  const isCopy = (e.ctrlKey || e.metaKey) && !e.shiftKey && code === "KeyC";
  const isPaste = (e.ctrlKey || e.metaKey) && !e.shiftKey && code === "KeyV";
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
    !isCopy &&
    !isPaste &&
    !isInterpolate &&
    !isTypeChar &&
    !isTypeControl
  ) {
    return false;
  }
  if (isCopy) {
    void copySelection();
    return true;
  }
  if (isPaste) {
    void pasteSelection();
    return true;
  }
  if (isTypeChar || isTypeControl) {
    if (editFocus.value === "x" && !canEditX.value) return false;
    if (editFocus.value === "y" && !canEditY.value) return false;
    if (editFocus.value === "grid" && disabled.value) return false;
    if (isTypeControl) {
      if ((key === "Enter" || key === "Escape") && !editBuffer.value) {
        return false;
      }
      const kind = key === "Backspace" ? "backspace" : key === "Enter" ? "commit" : "cancel";
      void dispatchWrite("type_key", { kind });
      return true;
    }
    let ch = key;
    if (code === "NumpadDecimal" || key === "," || key === ".") ch = ".";
    if (code === "NumpadSubtract" || key === "-") ch = "-";
    void dispatchWrite("type_key", { kind: "char", ch });
    return true;
  }
  if (isInterpolate) {
    if (editFocus.value === "x" && !canEditX.value) return false;
    if (editFocus.value === "y" && !canEditY.value) return false;
    if (editFocus.value === "grid" && disabled.value) return false;
    void dispatchWrite("interpolate");
    return true;
  }
  const arrowPayload = { key, shift: e.shiftKey, ctrl: e.ctrlKey };
  const isNudge =
    e.ctrlKey &&
    (key === "ArrowUp" ||
      key === "ArrowDown" ||
      key === "ArrowLeft" ||
      key === "ArrowRight");
  if (isNudge) {
    void dispatchWrite("keydown", arrowPayload);
  } else {
    void dispatch("keydown", arrowPayload);
  }
  return true;
}

defineExpose({ handleKeydown: onComponentKeydown });

useComponentBinding(props.path, onComponentKeydown);

function onCellMouseDown(row: number, col: number, e: MouseEvent) {
  if (!ready.value) return;
  if (e.button !== 0) return;
  const isInputTarget = e.target instanceof HTMLInputElement;
  if (!isInputTarget) {
    e.preventDefault();
  }
  isMouseSelecting.value = true;
  void dispatch("select_cell", { row, col, extend: e.shiftKey });
}

function onCellMouseEnter(row: number, col: number) {
  if (!ready.value || !isMouseSelecting.value || editFocus.value !== "grid") return;
  void dispatch("select_cell", { row, col, extend: true });
}

function onXAxisMouseDown(col: number, e: MouseEvent) {
  if (!ready.value || !canEditX.value) return;
  if (e.button !== 0) return;
  e.preventDefault();
  isMouseSelecting.value = true;
  void dispatch("select_x", { col, extend: e.shiftKey });
}

function onYAxisMouseDown(row: number, e: MouseEvent) {
  if (!ready.value || !canEditY.value) return;
  if (e.button !== 0) return;
  e.preventDefault();
  isMouseSelecting.value = true;
  void dispatch("select_y", { row, extend: e.shiftKey });
}

function onXAxisMouseEnter(col: number) {
  if (!ready.value || !isMouseSelecting.value || editFocus.value !== "x") return;
  void dispatch("select_x", { col, extend: true });
}

function onYAxisMouseEnter(row: number) {
  if (!ready.value || !isMouseSelecting.value || editFocus.value !== "y") return;
  void dispatch("select_y", { row, extend: true });
}

function onGlobalMouseUp() {
  isMouseSelecting.value = false;
}

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
    <header v-if="title || localError || statusText" class="grid-head">
      <h4 v-if="title" class="grid-title">{{ title }}</h4>
      <span
        v-if="localError || statusText"
        class="grid-badge"
        :class="{ 'grid-badge--error': !!localError }"
      >
        {{ localError || statusText }}
      </span>
    </header>

    <div ref="gridRef" class="grid-scroll">
      <table class="grid">
        <thead>
          <tr>
            <th class="corner">{{ yLabel }} \ {{ xLabel }}</th>
            <th
              v-for="col in colIndices"
              :key="`x-${col}`"
              class="axis-head axis-head-x"
              :class="{
                'axis-head--editable': canEditX,
                'axis-head--selected': editFocus === 'x' && xAxisCell(col)?.selected,
                'axis-head--cursor-x': editFocus === 'x' && xAxisCell(col)?.cursor,
                'axis-head--sel-start': isAxisSelectionEdge('x', col, 'start'),
                'axis-head--sel-end': isAxisSelectionEdge('x', col, 'end'),
                'axis-head--live': liveCol >= 0 && col === liveCol,
              }"
              @mousedown="onXAxisMouseDown(col, $event)"
              @mouseenter="onXAxisMouseEnter(col)"
            >
              <input
                type="text"
                class="axis-input"
                readonly
                tabindex="-1"
                spellcheck="false"
                autocomplete="off"
                :disabled="!canEditX"
                :value="axisDisplay('x', col, col)"
              />
            </th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in rowIndices" :key="`row-${row}`">
            <th
              class="axis-head axis-head-y"
              :class="{
                'axis-head--editable': canEditY,
                'axis-head--selected': editFocus === 'y' && yAxisCell(row)?.selected,
                'axis-head--cursor-y': editFocus === 'y' && yAxisCell(row)?.cursor,
                'axis-head--sel-start': isAxisSelectionEdge('y', row, 'start'),
                'axis-head--sel-end': isAxisSelectionEdge('y', row, 'end'),
                'axis-head--live': liveRow >= 0 && row === liveRow,
              }"
              @mousedown="onYAxisMouseDown(row, $event)"
              @mouseenter="onYAxisMouseEnter(row)"
            >
              <input
                type="text"
                class="axis-input"
                readonly
                tabindex="-1"
                spellcheck="false"
                autocomplete="off"
                :disabled="!canEditY"
                :value="axisDisplay('y', row, row)"
              />
            </th>
            <td
              v-for="col in colIndices"
              :key="`c-${row}-${col}`"
              class="cell-td"
              :class="{
                'cell-td--selected': editFocus === 'grid' && cellAt(row, col)?.selected,
                'cell-td--cursor': editFocus === 'grid' && cellAt(row, col)?.cursor,
                'cell-td--corner': editFocus === 'grid' && cellAt(row, col)?.corner,
                'cell-td--sel-top': isSelectionEdge(row, col, 'top'),
                'cell-td--sel-bottom': isSelectionEdge(row, col, 'bottom'),
                'cell-td--sel-left': isSelectionEdge(row, col, 'left'),
                'cell-td--sel-right': isSelectionEdge(row, col, 'right'),
                'cell-td--live': liveRow >= 0 && liveCol >= 0 && row === liveRow && col === liveCol,
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
      Таблица: ↑↓←→ · Shift — выделение · Ctrl+↑↓ — ±шаг · Ctrl+C/V · Ctrl+I — интерполяция.
      Оси X/Y: клик по заголовку · ←→ (X) / ↑↓ (Y) · Shift — диапазон · Ctrl+↑↓ — ±шаг · цифры — замена · Ctrl+I — интерполяция.
      Из таблицы: ↑ на верхней строке → ось X; ← в первом столбце → ось Y.
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

.grid-title {
  margin: 0;
  font-size: 0.92rem;
  font-weight: 600;
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
  padding: 0;
  white-space: nowrap;
}

.axis-head--editable {
  cursor: cell;
}

.axis-head--selected {
  background-image: linear-gradient(
    rgba(59, 130, 246, 0.12),
    rgba(59, 130, 246, 0.12)
  );
}

.axis-head--cursor-x .axis-input,
.axis-head--cursor-y .axis-input {
  outline: 2px solid var(--color-accent, #3b82f6);
  outline-offset: -2px;
}

.axis-head--sel-start {
  border-left: 2px solid var(--color-accent, #3b82f6) !important;
}

.axis-head--sel-end {
  border-right: 2px solid var(--color-accent, #3b82f6) !important;
}

.axis-head-y.axis-head--sel-start {
  border-left: 1px solid var(--color-border) !important;
  border-top: 2px solid var(--color-accent, #3b82f6) !important;
}

.axis-head-y.axis-head--sel-end {
  border-right: 1px solid var(--color-border) !important;
  border-bottom: 2px solid var(--color-accent, #3b82f6) !important;
}

.axis-input {
  width: 4.5rem;
  padding: 0.35rem 0.45rem;
  border: none;
  background: transparent;
  color: var(--color-text-muted);
  text-align: right;
  font-weight: 500;
  user-select: none;
  pointer-events: none;
}

.axis-head--cursor-x .axis-input,
.axis-head--cursor-y .axis-input {
  color: var(--color-text);
  font-weight: 600;
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

.cell-input:disabled {
  color: var(--color-text-muted);
}

.grid-hint {
  margin: 0;
  font-size: 0.68rem;
  color: var(--color-text-subtle);
}

/* ── Live cell highlight ─────────────────────────────────────────── */
.axis-head--live {
  background-image: linear-gradient(
    rgba(34, 197, 94, 0.18),
    rgba(34, 197, 94, 0.18)
  );
}

.axis-head--live .axis-input {
  color: var(--color-success, #16a34a);
  font-weight: 600;
}

.cell-td--live {
  box-shadow: inset 0 0 0 2px var(--color-success, #16a34a);
  z-index: 1;
  position: relative;
}
</style>
