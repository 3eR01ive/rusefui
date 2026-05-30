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
  commitCell,
  commitRowValue,
  commitXValue,
  setRowPreview,
  setXPreview,
  xValueAt,
  statusText,
  localError,
  xValues,
} = useConfigGrid({ kind: "curve", instance: instanceRef, props: propsRef });

const chartHeight = computed(() => {
  const h = Number(props.props.height ?? 220);
  return h >= 140 ? h : 220;
});

const dragRow = ref<number | null>(null);
const hoverRow = ref<number | null>(null);
const dragStartY = ref<number | null>(null);

const rootRef = ref<HTMLDivElement | null>(null);
const canvasRef = ref<HTMLCanvasElement | null>(null);

const yEditBuffers = ref<Record<number, string>>({});
const xEditBuffers = ref<Record<number, string>>({});

function displayY(col: number): number | null {
  return cellValue(col, 0);
}

function displayX(col: number): number | null {
  const v = xValueAt(col);
  if (v !== null) return v;
  return col;
}

function yBufferFor(col: number): string {
  if (yEditBuffers.value[col] !== undefined) return yEditBuffers.value[col]!;
  const v = displayY(col);
  return v === null ? "" : fmt(v);
}

function xBufferFor(col: number): string {
  if (xEditBuffers.value[col] !== undefined) return xEditBuffers.value[col]!;
  const v = displayX(col);
  return v === null ? "" : fmt(v);
}

function setYBuffer(col: number, raw: string): void {
  yEditBuffers.value = { ...yEditBuffers.value, [col]: raw };
}

function setXBuffer(col: number, raw: string): void {
  xEditBuffers.value = { ...xEditBuffers.value, [col]: raw };
}

function clearYBuffer(col: number): void {
  if (yEditBuffers.value[col] === undefined) return;
  const next = { ...yEditBuffers.value };
  delete next[col];
  yEditBuffers.value = next;
}

function clearXBuffer(col: number): void {
  if (xEditBuffers.value[col] === undefined) return;
  const next = { ...xEditBuffers.value };
  delete next[col];
  xEditBuffers.value = next;
}

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
    activeRow: dragRow.value,
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
  setYBuffer(dragRow.value, fmt(nextY));
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
  clearYBuffer(row);
  redraw();
}

function yStep(event: KeyboardEvent): number {
  return event.shiftKey ? 0.001 : 0.01;
}

function xStep(event: KeyboardEvent): number {
  return event.shiftKey ? 1 : 10;
}

function onYKeydown(col: number, event: KeyboardEvent): void {
  const current = displayY(col);
  if (current === null) return;
  if (event.key === "ArrowUp") {
    event.preventDefault();
    void commitRowValue(col, 0, current + yStep(event));
    clearYBuffer(col);
  } else if (event.key === "ArrowDown") {
    event.preventDefault();
    void commitRowValue(col, 0, Math.max(0, current - yStep(event)));
    clearYBuffer(col);
  } else if (event.key === "Enter") {
    event.preventDefault();
    void commitYInput(col, (event.target as HTMLInputElement).value);
  }
}

function onXKeydown(col: number, event: KeyboardEvent): void {
  const current = displayX(col);
  if (current === null) return;
  if (event.key === "ArrowUp") {
    event.preventDefault();
    void commitXValue(col, current + xStep(event));
    clearXBuffer(col);
  } else if (event.key === "ArrowDown") {
    event.preventDefault();
    void commitXValue(col, current - xStep(event));
    clearXBuffer(col);
  } else if (event.key === "Enter") {
    event.preventDefault();
    void commitXInput(col, (event.target as HTMLInputElement).value);
  }
}

async function commitYInput(col: number, raw: string): Promise<void> {
  await commitCell(col, 0, raw);
  clearYBuffer(col);
}

async function commitXInput(col: number, raw: string): Promise<void> {
  const parsed = Number(raw.trim().replace(",", "."));
  if (!Number.isFinite(parsed)) {
    return;
  }
  await commitXValue(col, parsed);
  clearXBuffer(col);
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

watch([curvePoints, chartHeight, dragRow, hoverRow], () => redraw(), { deep: true });

watch(rowIndices, () => {
  yEditBuffers.value = {};
  xEditBuffers.value = {};
  scheduleInitialRedraw();
});

onMounted(() => {
  window.addEventListener("mouseup", onWindowMouseUp);
  scheduleInitialRedraw();
});

onBeforeUnmount(() => {
  window.removeEventListener("mousemove", onWindowMouseMove);
  window.removeEventListener("mouseup", onWindowMouseUp);
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

    <p class="curve-hint">
      Перетащите точку на графике или отредактируйте {{ xLabel }} / {{ yLabel }} в таблице.
    </p>

    <div class="grid-scroll">
      <table class="grid grid--horizontal">
        <tbody>
          <tr>
            <th class="axis-label">{{ xLabel }}</th>
            <td
              v-for="col in rowIndices"
              :key="`x-${col}`"
              :class="{
                'grid-cell--active': dragRow === col,
                'grid-cell--hover': hoverRow === col,
              }"
            >
              <input
                type="text"
                class="cell-input"
                inputmode="decimal"
                :disabled="!xEditable"
                :value="xBufferFor(col)"
                @input="setXBuffer(col, ($event.target as HTMLInputElement).value)"
                @keydown="onXKeydown(col, $event)"
                @blur="commitXInput(col, ($event.target as HTMLInputElement).value)"
              />
            </td>
          </tr>
          <tr>
            <th class="axis-label">{{ yLabel }}</th>
            <td
              v-for="col in rowIndices"
              :key="`y-${col}`"
              :class="{
                'grid-cell--active': dragRow === col,
                'grid-cell--hover': hoverRow === col,
              }"
            >
              <input
                type="text"
                class="cell-input"
                inputmode="decimal"
                :disabled="disabled"
                :value="yBufferFor(col)"
                @input="setYBuffer(col, ($event.target as HTMLInputElement).value)"
                @keydown="onYKeydown(col, $event)"
                @blur="commitYInput(col, ($event.target as HTMLInputElement).value)"
              />
            </td>
          </tr>
        </tbody>
      </table>
    </div>
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
  font-size: 0.72rem;
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

.grid-cell--active,
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
}

.cell-input:focus {
  outline: 2px solid color-mix(in srgb, var(--color-accent) 55%, transparent);
  outline-offset: -2px;
  background: var(--color-bg);
}

.cell-input:disabled {
  background: var(--color-bg-muted);
  color: var(--color-text-muted);
}
</style>
