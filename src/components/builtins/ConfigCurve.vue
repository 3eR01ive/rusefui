<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useConfigGrid } from "../../composables/useConfigGrid";
import {
  computeCurveChartLayout,
  drawConfigCurveChart,
  hitTestCurvePoint,
  type CurvePoint,
} from "../../composables/drawConfigCurveChart";
import {
  measureChartWidth,
  useChartCanvasLayout,
} from "../../composables/useChartCanvasLayout";
import { useComponentBinding } from "../../composables/useKeyboardRouter";
import { activePath, navMode } from "../../composables/useWorkspaceNav";

type CurveAxis = "x" | "y";

const NUDGE_STEP = 0.1;

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const instanceRef = computed(() => props.instance);
const propsRef = computed(() => props.props);

const {
  title,
  xLabel,
  yLabel,
  rowIndices,
  disabled,
  xEditable,
  fmt,
  cellValue,
  commitRowValue,
  commitXValue,
  setRowPreview,
  setXPreview,
  xValueAt,
  statusText,
  localError,
} = useConfigGrid({ kind: "curve", instance: instanceRef, props: propsRef });

const chartHeight = computed(() => {
  const h = Number(props.props.height ?? 220);
  return h >= 140 ? h : 220;
});

const isActive = computed(
  () => navMode.value === "active" && activePath.value === props.path,
);

const dragRow = ref<number | null>(null);
const hoverRow = ref<number | null>(null);
const dragStartY = ref<number | null>(null);

const rootRef = ref<HTMLDivElement | null>(null);
const gridRef = ref<HTMLDivElement | null>(null);
const canvasRef = ref<HTMLCanvasElement | null>(null);

const cursorAxis = ref<CurveAxis>("y");
const cursorCol = ref(0);
const editBuffer = ref("");
const editStartValue = ref<number | null>(null);

function displayY(col: number): number | null {
  return cellValue(col, 0);
}

function displayX(col: number): number | null {
  const v = xValueAt(col);
  if (v !== null) return v;
  return col;
}

function axisEditable(axis: CurveAxis): boolean {
  return axis === "x" ? xEditable.value : !disabled.value;
}

function cellDisplay(axis: CurveAxis, col: number): string {
  const isCursor = isActive.value && cursorAxis.value === axis && cursorCol.value === col;
  if (isCursor && editBuffer.value !== "") {
    return editBuffer.value;
  }
  if (axis === "x") {
    const v = displayX(col);
    return v === null ? "" : fmt(v);
  }
  const v = displayY(col);
  return v === null ? "" : fmt(v);
}

function isCursorCell(axis: CurveAxis, col: number): boolean {
  return isActive.value && cursorAxis.value === axis && cursorCol.value === col;
}

function selectCell(axis: CurveAxis, col: number): void {
  cursorAxis.value = axis;
  cursorCol.value = col;
  editBuffer.value = "";
  editStartValue.value = null;
  scrollCursorIntoView();
}

function scrollCursorIntoView(): void {
  void nextTick(() => {
    const el = gridRef.value?.querySelector(".grid-cell--cursor");
    el?.scrollIntoView({ block: "nearest", inline: "nearest" });
  });
}

function revertPreview(): void {
  if (editStartValue.value === null) return;
  const value = editStartValue.value;
  if (cursorAxis.value === "x") {
    setXPreview(cursorCol.value, value);
  } else {
    setRowPreview(cursorCol.value, 0, value);
  }
  redraw();
}

function applyBufferPreview(): void {
  const parsed = Number(editBuffer.value.trim().replace(",", "."));
  if (!Number.isFinite(parsed)) return;
  if (cursorAxis.value === "x") {
    setXPreview(cursorCol.value, parsed);
  } else {
    setRowPreview(cursorCol.value, 0, parsed);
  }
  redraw();
}

async function commitEditBuffer(): Promise<void> {
  // Любой переход/коммит сперва досыпает отложенный нудж в ECU.
  await flushNudge();
  if (!editBuffer.value) return;
  const parsed = Number(editBuffer.value.trim().replace(",", "."));
  if (!Number.isFinite(parsed)) {
    revertPreview();
    editBuffer.value = "";
    editStartValue.value = null;
    return;
  }
  if (cursorAxis.value === "x") {
    await commitXValue(cursorCol.value, parsed);
  } else {
    await commitRowValue(cursorCol.value, 0, parsed);
  }
  editBuffer.value = "";
  editStartValue.value = null;
}

async function cancelEditBuffer(): Promise<void> {
  if (editBuffer.value) {
    revertPreview();
  }
  editBuffer.value = "";
  editStartValue.value = null;
}

async function onCellMouseDown(axis: CurveAxis, col: number, event: MouseEvent): Promise<void> {
  if (event.button !== 0) return;
  event.preventDefault();
  await commitEditBuffer();
  selectCell(axis, col);
}

async function handleArrow(key: string): Promise<void> {
  await commitEditBuffer();
  const last = rowIndices.value.length - 1;
  if (last < 0) return;

  let axis = cursorAxis.value;
  let col = cursorCol.value;

  if (key === "ArrowLeft") {
    col = Math.max(0, col - 1);
  } else if (key === "ArrowRight") {
    col = Math.min(last, col + 1);
  } else if (key === "ArrowUp") {
    if (axis === "y" && xEditable.value) axis = "x";
  } else if (key === "ArrowDown") {
    if (axis === "x") axis = "y";
  }

  selectCell(axis, col);
}

function currentCellNumeric(): number | null {
  if (editBuffer.value !== "") {
    const parsed = Number(editBuffer.value.trim().replace(",", "."));
    if (Number.isFinite(parsed)) return parsed;
  }
  return cursorAxis.value === "x" ? displayX(cursorCol.value) : displayY(cursorCol.value);
}

// Отложенная запись нуджа в ECU: на каждый шаг — живой предпросмотр локально, в
// ECU пишем разом, когда удержание прекратится (≈ отпускание клавиши).
let nudgeFlushTimer = 0;
let pendingNudge: { axis: CurveAxis; col: number; value: number } | null = null;
const NUDGE_FLUSH_IDLE_MS = 250;

function scheduleNudgeFlush(): void {
  if (nudgeFlushTimer !== 0) window.clearTimeout(nudgeFlushTimer);
  nudgeFlushTimer = window.setTimeout(() => {
    nudgeFlushTimer = 0;
    void flushNudge();
  }, NUDGE_FLUSH_IDLE_MS);
}

async function flushNudge(): Promise<void> {
  if (nudgeFlushTimer !== 0) {
    window.clearTimeout(nudgeFlushTimer);
    nudgeFlushTimer = 0;
  }
  const p = pendingNudge;
  if (!p) return;
  pendingNudge = null;
  if (p.axis === "x") {
    await commitXValue(p.col, p.value);
  } else {
    await commitRowValue(p.col, 0, p.value);
  }
}

async function nudgeCell(direction: "up" | "down"): Promise<void> {
  if (!axisEditable(cursorAxis.value)) return;
  const current = currentCellNumeric();
  if (current === null || !Number.isFinite(current)) return;

  const delta = direction === "up" ? NUDGE_STEP : -NUDGE_STEP;
  const next = current + delta;

  editBuffer.value = "";
  editStartValue.value = null;

  if (cursorAxis.value === "x") {
    setXPreview(cursorCol.value, next);
    pendingNudge = { axis: "x", col: cursorCol.value, value: next };
  } else {
    setRowPreview(cursorCol.value, 0, next);
    pendingNudge = { axis: "y", col: cursorCol.value, value: next };
  }
  scheduleNudgeFlush();
  redraw();
}

function handleTypeChar(key: string, code: string): void {
  if (!axisEditable(cursorAxis.value)) return;

  if (editBuffer.value === "") {
    const current =
      cursorAxis.value === "x" ? displayX(cursorCol.value) : displayY(cursorCol.value);
    editStartValue.value = current;
  }

  let ch = key;
  if (code === "NumpadDecimal" || key === "," || key === ".") ch = ".";
  if (code === "NumpadSubtract" || key === "-") ch = "-";

  if (/^[0-9]$/.test(ch)) {
    editBuffer.value += ch;
  } else if (ch === ".") {
    if (!editBuffer.value.includes(".")) {
      if (!editBuffer.value) editBuffer.value = "0";
      editBuffer.value += ".";
    }
  } else if (ch === "-") {
    if (!editBuffer.value) editBuffer.value = "-";
  } else {
    return;
  }

  applyBufferPreview();
}

async function handleTypeControl(key: string): Promise<void> {
  if (key === "Backspace") {
    editBuffer.value = editBuffer.value.slice(0, -1);
    if (editBuffer.value === "") {
      revertPreview();
      editStartValue.value = null;
    } else {
      applyBufferPreview();
    }
    return;
  }
  if (key === "Enter") {
    await commitEditBuffer();
    return;
  }
  if (key === "Escape") {
    await cancelEditBuffer();
  }
}

function onComponentKeydown(event: KeyboardEvent): boolean {
  if (!isActive.value) return false;

  const key = event.key;
  const code = event.code;

  // ,(Comma) декремент · .(Period) инкремент по ФИЗИЧЕСКОЙ позиции (event.code,
  // без Shift, не зависит от раскладки). Ctrl остаётся для навигации.
  const noCmd = !event.ctrlKey && !event.metaKey;
  if (noCmd && code === "Comma") {
    void nudgeCell("down");
    return true;
  }
  if (noCmd && code === "Period") {
    void nudgeCell("up");
    return true;
  }

  const isArrow = ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(key);
  // Десятичная точка — только Numpad `.` (основные `,`/`.` заняты ±шагом).
  const isTypeChar =
    (!event.ctrlKey && !event.metaKey && !event.altKey && /^[0-9]$/.test(key)) ||
    code === "NumpadDecimal" ||
    code === "NumpadSubtract" ||
    code === "Minus";
  const isTypeControl = key === "Backspace" || key === "Enter" || key === "Escape";

  if (!isArrow && !isTypeChar && !isTypeControl) return false;

  // Ctrl+стрелки отдаём навигации между панелями, а не кривой.
  if (isArrow && (event.ctrlKey || event.metaKey)) {
    return false;
  }

  if (isArrow && !event.altKey) {
    void handleArrow(key);
    return true;
  }

  if (isTypeControl) {
    if ((key === "Enter" || key === "Escape") && !editBuffer.value) {
      return false;
    }
    void handleTypeControl(key);
    return true;
  }

  if (isTypeChar) {
    handleTypeChar(key, code);
    return true;
  }

  return false;
}

useComponentBinding(props.path, onComponentKeydown);

const curvePoints = computed((): CurvePoint[] => {
  const pts: CurvePoint[] = [];
  for (const col of rowIndices.value) {
    const y = displayY(col);
    if (y === null || !Number.isFinite(y)) continue;
    const x = displayX(col);
    pts.push({
      x: x !== null && Number.isFinite(x) ? x : col,
      y,
      row: col,
    });
  }
  return pts;
});

function chartWidth(): number {
  return measureChartWidth(rootRef.value, 1);
}

function redraw(): void {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const w = chartWidth();
  const h = chartHeight.value;
  if (w < 1) return;
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.floor(w * dpr);
  canvas.height = Math.floor(h * dpr);
  canvas.style.width = "100%";
  canvas.style.height = `${h}px`;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  drawConfigCurveChart(ctx, w, h, curvePoints.value, undefined, {
    activeRow: dragRow.value ?? (isActive.value ? cursorCol.value : null),
    hoverRow: hoverRow.value,
  });
}

function chartLayout() {
  const w = chartWidth();
  const h = chartHeight.value;
  if (w < 1) return null;
  return computeCurveChartLayout(w, h, curvePoints.value);
}

function canvasPoint(event: MouseEvent): { x: number; y: number } | null {
  const canvas = canvasRef.value;
  if (!canvas) return null;
  const rect = canvas.getBoundingClientRect();
  return {
    x: event.clientX - rect.left,
    y: event.clientY - rect.top,
  };
}

function clampY(y: number): number {
  const layout = chartLayout();
  if (!layout) return y;
  return Math.min(layout.yMax, Math.max(layout.yMin, y));
}

function onCanvasMouseDown(event: MouseEvent): void {
  if (disabled.value || event.button !== 0) return;
  const pt = canvasPoint(event);
  const layout = chartLayout();
  if (!pt || !layout) return;
  const row = hitTestCurvePoint(layout, pt.x, pt.y);
  if (row === null) return;
  event.preventDefault();
  void commitEditBuffer().then(() => selectCell("y", row));
  dragRow.value = row;
  dragStartY.value = displayY(row);
  window.addEventListener("mousemove", onWindowMouseMove);
  window.addEventListener("mouseup", onWindowMouseUp);
}

function onCanvasMouseMove(event: MouseEvent): void {
  if (dragRow.value !== null) return;
  const pt = canvasPoint(event);
  const layout = chartLayout();
  if (!pt || !layout) {
    hoverRow.value = null;
    redraw();
    return;
  }
  hoverRow.value = hitTestCurvePoint(layout, pt.x, pt.y);
  redraw();
}

function onCanvasMouseLeave(): void {
  if (dragRow.value !== null) return;
  hoverRow.value = null;
  redraw();
}

function onWindowMouseMove(event: MouseEvent): void {
  if (dragRow.value === null) return;
  const pt = canvasPoint(event);
  const layout = chartLayout();
  if (!pt || !layout) return;
  const nextY = clampY(layout.fromY(pt.y));
  setRowPreview(dragRow.value, 0, nextY);
  editBuffer.value = "";
  redraw();
}

async function onWindowMouseUp(): Promise<void> {
  window.removeEventListener("mousemove", onWindowMouseMove);
  window.removeEventListener("mouseup", onWindowMouseUp);
  const row = dragRow.value;
  const startY = dragStartY.value;
  dragRow.value = null;
  dragStartY.value = null;
  hoverRow.value = null;
  if (row === null || startY === null) {
    redraw();
    return;
  }
  const nextY = displayY(row);
  if (nextY === null || Math.abs(nextY - startY) < 1e-9) {
    setRowPreview(row, 0, startY);
    redraw();
    return;
  }
  await commitRowValue(row, 0, nextY);
  editBuffer.value = "";
  editStartValue.value = null;
  redraw();
}

function scheduleInitialRedraw(): void {
  void nextTick(() => {
    requestAnimationFrame(() => {
      redraw();
      requestAnimationFrame(redraw);
    });
  });
}

useChartCanvasLayout(rootRef, redraw);

watch([curvePoints, chartHeight, dragRow, hoverRow, cursorAxis, cursorCol, editBuffer, isActive], () =>
  redraw(),
);

watch(rowIndices, (cols) => {
  editBuffer.value = "";
  editStartValue.value = null;
  if (cols.length === 0) return;
  cursorCol.value = Math.min(cursorCol.value, cols.length - 1);
  scheduleInitialRedraw();
});

watch(isActive, (active) => {
  if (active && rowIndices.value.length > 0) {
    selectCell("y", Math.min(cursorCol.value, rowIndices.value.length - 1));
  } else {
    void commitEditBuffer();
  }
});

onMounted(() => {
  window.addEventListener("mouseup", onWindowMouseUp);
  scheduleInitialRedraw();
});

onBeforeUnmount(() => {
  window.removeEventListener("mousemove", onWindowMouseMove);
  window.removeEventListener("mouseup", onWindowMouseUp);
  void flushNudge();
});
</script>

<template>
  <div ref="rootRef" class="config-curve">
    <header v-if="title" class="grid-head">
      <h4 class="grid-title">{{ title }}</h4>
      <span class="grid-badge" :class="{ 'grid-badge--error': !!localError }">
        {{ statusText }}
      </span>
    </header>

    <div class="curve-chart-wrap">
      <div class="curve-axis-label curve-axis-label--y">{{ yLabel }}</div>
      <div class="curve-chart-main">
        <canvas
          ref="canvasRef"
          class="curve-canvas"
          :class="{
            'curve-canvas--drag': dragRow !== null,
            'curve-canvas--hover': hoverRow !== null && dragRow === null,
          }"
          @mousedown="onCanvasMouseDown"
          @mousemove="onCanvasMouseMove"
          @mouseleave="onCanvasMouseLeave"
        />
        <div class="curve-axis-label curve-axis-label--x">{{ xLabel }}</div>
      </div>
    </div>

    <div ref="gridRef" class="grid-scroll">
      <table class="grid grid--horizontal">
        <tbody>
          <tr>
            <th class="axis-label">{{ xLabel }}</th>
            <td
              v-for="col in rowIndices"
              :key="`x-${col}`"
              class="grid-cell"
              :class="{
                'grid-cell--cursor': isCursorCell('x', col),
                'grid-cell--hover': hoverRow === col || (isActive && cursorCol === col && cursorAxis === 'y'),
              }"
              @mousedown="onCellMouseDown('x', col, $event)"
            >
              <input
                type="text"
                class="cell-input"
                readonly
                tabindex="-1"
                spellcheck="false"
                autocomplete="off"
                :disabled="!xEditable"
                :value="cellDisplay('x', col)"
              />
            </td>
          </tr>
          <tr>
            <th class="axis-label">{{ yLabel }}</th>
            <td
              v-for="col in rowIndices"
              :key="`y-${col}`"
              class="grid-cell"
              :class="{
                'grid-cell--cursor': isCursorCell('y', col),
                'grid-cell--hover': hoverRow === col || (isActive && cursorCol === col && cursorAxis === 'x'),
              }"
              @mousedown="onCellMouseDown('y', col, $event)"
            >
              <input
                type="text"
                class="cell-input"
                readonly
                tabindex="-1"
                spellcheck="false"
                autocomplete="off"
                :disabled="disabled"
                :value="cellDisplay('y', col)"
              />
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <p class="curve-hint">
      Enter — активировать · ↑↓←→ — ячейка · «,» −0.1 / «.» +0.1 · цифры — новое значение · график — перетаскивание Y
    </p>
  </div>
</template>

<style scoped>
.config-curve {
  display: flex;
  flex-direction: column;
  gap: 0.65rem;
  width: 100%;
  min-width: 0;
  align-self: stretch;
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

.curve-chart-wrap {
  display: flex;
  align-items: stretch;
  gap: 0.35rem;
  width: 100%;
  min-width: 0;
}

.curve-chart-main {
  flex: 1;
  min-width: 0;
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.curve-canvas {
  display: block;
  width: 100%;
  min-width: 0;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
  touch-action: none;
}

.curve-canvas--hover {
  cursor: pointer;
}

.curve-canvas--drag {
  cursor: grabbing;
}

.curve-axis-label {
  font-size: 0.68rem;
  font-weight: 500;
  color: var(--color-text-muted);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.curve-axis-label--y {
  writing-mode: vertical-rl;
  transform: rotate(180deg);
  align-self: center;
  flex-shrink: 0;
}

.curve-axis-label--x {
  text-align: center;
}

.curve-hint {
  margin: 0;
  font-size: 0.68rem;
  color: var(--color-text-muted);
}

.grid-scroll {
  overflow-x: auto;
  overflow-y: hidden;
  max-width: 100%;
  width: 100%;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
}

.grid {
  border-collapse: collapse;
  font-size: 0.78rem;
  width: 100%;
  table-layout: fixed;
}

.grid--horizontal td,
.grid--horizontal th {
  border: 1px solid var(--color-border);
  padding: 0;
  min-width: 4.25rem;
}

.axis-label {
  background: var(--color-bg-muted);
  color: var(--color-text-muted);
  font-weight: 600;
  padding: 0.35rem 0.5rem;
  text-align: left;
  white-space: nowrap;
  width: 5.5rem;
  position: sticky;
  left: 0;
  z-index: 1;
}

.grid-cell--cursor {
  outline: 2px solid var(--color-accent, #3b82f6);
  outline-offset: -2px;
  z-index: 1;
}

.grid-cell--hover {
  background: color-mix(in srgb, var(--color-accent) 8%, transparent);
}

.cell-input {
  box-sizing: border-box;
  width: 100%;
  padding: 0.35rem 0.45rem;
  border: none;
  background: transparent;
  color: var(--color-text);
  text-align: center;
  font-variant-numeric: tabular-nums;
  user-select: none;
  pointer-events: none;
}

.grid-cell--cursor .cell-input {
  font-weight: 600;
}

.cell-input:disabled {
  color: var(--color-text-muted);
}
</style>
