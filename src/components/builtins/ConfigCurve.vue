<script setup lang="ts">
import {
  computed,
  onMounted,
  onUnmounted,
  ref,
  toRef,
  watch,
} from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useConfigGrid } from "../../composables/useConfigGrid";
import {
  drawConfigCurveChart,
  type CurvePoint,
} from "../../composables/drawConfigCurveChart";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const propsRef = toRef(props, "props");

const {
  title,
  xLabel,
  yLabel,
  rowIndices,
  disabled,
  fmt,
  cellValue,
  commitCell,
  statusText,
  localError,
  xValues,
} = useConfigGrid({ kind: "curve", props: propsRef });

const chartHeight = computed(() => {
  const h = Number(props.props.height ?? 220);
  return h >= 140 ? h : 220;
});

const curvePoints = computed((): CurvePoint[] => {
  const pts: CurvePoint[] = [];
  for (const row of rowIndices.value) {
    const y = cellValue(row, 0);
    if (y === null || !Number.isFinite(y)) continue;
    const x = xValues.value[row];
    pts.push({
      x: x !== undefined && Number.isFinite(x) ? x : row,
      y,
    });
  }
  return pts;
});

const canvasRef = ref<HTMLCanvasElement | null>(null);
const containerRef = ref<HTMLDivElement | null>(null);
const canvasWidth = ref(480);

function redraw(): void {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const dpr = window.devicePixelRatio || 1;
  const w = canvasWidth.value;
  const h = chartHeight.value;
  canvas.width = Math.floor(w * dpr);
  canvas.height = Math.floor(h * dpr);
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  drawConfigCurveChart(ctx, w, h, curvePoints.value);
}

let resizeObserver: ResizeObserver | undefined;

onMounted(() => {
  redraw();
  const el = containerRef.value;
  if (!el || typeof ResizeObserver === "undefined") return;
  resizeObserver = new ResizeObserver((entries) => {
    const entry = entries[0];
    if (entry) {
      canvasWidth.value = Math.max(200, entry.contentRect.width);
    }
  });
  resizeObserver.observe(el);
});

onUnmounted(() => {
  resizeObserver?.disconnect();
});

watch([curvePoints, chartHeight, canvasWidth], () => redraw(), { deep: true });
</script>

<template>
  <div class="config-curve" ref="containerRef">
    <header v-if="title" class="grid-head">
      <h4 class="grid-title">{{ title }}</h4>
      <span class="grid-badge" :class="{ 'grid-badge--error': !!localError }">
        {{ statusText }}
      </span>
    </header>

    <div class="curve-chart-wrap">
      <div class="curve-axis-label curve-axis-label--y">{{ yLabel }}</div>
      <div class="curve-chart-main">
        <canvas ref="canvasRef" class="curve-canvas" />
        <div class="curve-axis-label curve-axis-label--x">{{ xLabel }}</div>
      </div>
    </div>

    <details class="curve-table-details" open>
      <summary>Точки</summary>
      <div class="grid-scroll">
        <table class="grid">
          <thead>
            <tr>
              <th>{{ xLabel }}</th>
              <th>{{ yLabel }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="row in rowIndices" :key="row">
              <td class="axis-cell">{{ fmt(xValues[row] ?? row) }}</td>
              <td>
                <input
                  type="text"
                  class="cell-input"
                  :disabled="disabled"
                  :value="fmt(cellValue(row, 0) ?? 0)"
                  @change="
                    commitCell(row, 0, ($event.target as HTMLInputElement).value)
                  "
                />
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </details>
  </div>
</template>

<style scoped>
.config-curve {
  display: flex;
  flex-direction: column;
  gap: 0.65rem;
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

.curve-chart-wrap {
  display: flex;
  align-items: stretch;
  gap: 0.35rem;
  width: 100%;
}

.curve-chart-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.curve-canvas {
  display: block;
  width: 100%;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
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

.curve-table-details summary {
  cursor: pointer;
  font-size: 0.75rem;
  color: var(--color-text-muted);
  user-select: none;
  margin-bottom: 0.35rem;
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

.axis-cell {
  background: var(--color-bg-muted);
  color: var(--color-text-muted);
  font-weight: 500;
  padding: 0.35rem 0.5rem;
  white-space: nowrap;
}

.cell-input {
  width: 4.5rem;
  padding: 0.35rem 0.45rem;
  border: none;
  background: var(--color-bg);
  color: var(--color-text);
  text-align: right;
}

.cell-input:disabled {
  background: var(--color-bg-muted);
  color: var(--color-text-muted);
}
</style>
