<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useDataContext } from "../../core/data-context";
import {
  initCompositeLogger,
  useCompositeLogger,
  type CompositeEvent,
} from "../../composables/useCompositeLogger";
import {
  PERSIST_KEY_COMPOSITE_CHART,
  useProject,
  type CompositeChartUiSettings,
} from "../../composables/useProject";
import {
  buildChartView,
  bufferSpanMs,
  channelValue,
  crankAngleDeg,
  CRANK_CYCLE_DEG,
  laneY,
  maxViewSpanMs,
  timeAtX,
  valueAtTime,
  xAtTime,
  type ChannelKey,
  type ChartTimeRange,
  type ChartView,
} from "./compositeChartGeometry";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const maxWindowMs = computed(() => Math.max(5, Number(props.props.windowMs ?? 300)));
const chartHeight = computed(() => Math.max(120, Number(props.props.height ?? 220)));

const MIN_VIEW_MS = 5;
const ZOOM_STEP = 1.12;

/** Ширина окна просмотра (мс), ≤ maxWindowMs и ≤ буфера. */
const viewSpanMs = ref(maxWindowMs.value);
/** `null` — хвост live; иначе фиксированный t0 (µs). */
const viewAnchorT0Us = ref<number | null>(null);

const { snapshot, setLoggingEnabled } = useCompositeLogger();
const { getProjectUi, setProjectUi } = useProject();
const dataCtx = useDataContext();
const connected = computed(() => dataCtx.connection.value.connected);
const loggingEnabled = computed(() => snapshot.value.loggingEnabled);
const autostart = ref(false);
const loggerBusy = ref(false);
const loggerError = ref<string | null>(null);

const plotWrapRef = ref<HTMLDivElement | null>(null);
const canvasRef = ref<HTMLCanvasElement | null>(null);
const hoverX = ref<number | null>(null);
const hoverInside = ref(false);
let ro: ResizeObserver | null = null;

const CHANNELS: { key: ChannelKey; label: string; color: string }[] = [
  { key: "pri", label: "Pri", color: "#3b82f6" },
  { key: "sec", label: "Sec", color: "#8b5cf6" },
  { key: "trg", label: "TDC", color: "#f59e0b" },
  { key: "sync", label: "Sync", color: "#10b981" },
  { key: "coil", label: "Coil", color: "#ef4444" },
  { key: "inj", label: "Inj", color: "#06b6d4" },
];

const LABEL_W = 44;
/** Табличка значений справа от crosshair, чтобы не перекрывать график. */
const TIP_OFFSET_X = 40;

function maxSpanMsFor(events: readonly CompositeEvent[]): number {
  return maxViewSpanMs(events, maxWindowMs.value, MIN_VIEW_MS);
}

function dataT0(events: readonly CompositeEvent[]): number {
  return events[0]!.tUs;
}

function currentTimeRange(events: readonly CompositeEvent[]): ChartTimeRange | null {
  if (events.length < 2) return null;
  const tFirst = dataT0(events);
  const dataEnd = events[events.length - 1]!.tUs;
  const spanUs = Math.round(viewSpanMs.value * 1000);

  if (viewAnchorT0Us.value == null) {
    const tEnd = dataEnd;
    return { t0: Math.max(tFirst, tEnd - spanUs), tEnd };
  }

  let t0 = Math.max(tFirst, viewAnchorT0Us.value);
  let tEnd = t0 + spanUs;
  if (tEnd > dataEnd) {
    tEnd = dataEnd;
    t0 = Math.max(tFirst, tEnd - spanUs);
  }
  return { t0, tEnd };
}

function resetViewWindow(events?: readonly CompositeEvent[]) {
  const ev = (events ?? snapshot.value.events) as CompositeEvent[];
  viewSpanMs.value =
    ev.length >= 2
      ? Math.min(maxWindowMs.value, Math.max(MIN_VIEW_MS, bufferSpanMs(ev)))
      : maxWindowMs.value;
  viewAnchorT0Us.value = null;
}

function clampViewToBuffer(events: readonly CompositeEvent[]) {
  const cap = maxSpanMsFor(events);
  if (viewSpanMs.value > cap) viewSpanMs.value = cap;
  if (viewAnchorT0Us.value != null && events.length >= 2) {
    const tFirst = dataT0(events);
    const dataEnd = events[events.length - 1]!.tUs;
    const spanUs = Math.round(viewSpanMs.value * 1000);
    if (viewAnchorT0Us.value < tFirst) viewAnchorT0Us.value = tFirst;
    if (viewAnchorT0Us.value + spanUs > dataEnd) {
      viewAnchorT0Us.value = Math.max(tFirst, dataEnd - spanUs);
    }
  }
}

watch(maxWindowMs, () => {
  clampViewToBuffer(snapshot.value.events as CompositeEvent[]);
});

watch(
  () => {
    const ev = snapshot.value.events;
    if (ev.length === 0) return "0";
    return `${ev.length}:${ev[ev.length - 1]!.tUs}`;
  },
  () => {
    clampViewToBuffer(snapshot.value.events as CompositeEvent[]);
  },
);

function cssColor(canvas: HTMLCanvasElement, varName: string, fallback: string): string {
  const v = getComputedStyle(canvas).getPropertyValue(varName).trim();
  return v || fallback;
}

function drawWaveforms(
  ctx: CanvasRenderingContext2D,
  view: ChartView,
  canvas: HTMLCanvasElement,
) {
  CHANNELS.forEach((ch, idx) => {
    const { yHigh, yLow } = laneY(idx, view, true);
    const yMid = (yHigh + yLow) / 2;

    ctx.fillStyle = cssColor(canvas, "--color-gray", "#9ca3af");
    ctx.font = "11px system-ui, sans-serif";
    ctx.textAlign = "right";
    ctx.fillText(ch.label, LABEL_W - 6, yMid + 4);

    const toX = (tUs: number) => xAtTime(tUs, view);
    const visible = view.visible;
    let prevT = view.t0;
    let prevVal = valueAtTime(view.t0, visible, ch.key);

    for (const ev of visible) {
      const x = toX(ev.tUs);
      const val = channelValue(ev, ch.key);
      if (ev.tUs > prevT) {
        ctx.strokeStyle = ch.color;
        ctx.lineWidth = 2;
        ctx.setLineDash([]);
        ctx.beginPath();
        ctx.moveTo(toX(prevT), prevVal ? yHigh : yLow);
        ctx.lineTo(x, prevVal ? yHigh : yLow);
        ctx.stroke();
      }
      if (val !== prevVal) {
        ctx.beginPath();
        ctx.moveTo(x, prevVal ? yHigh : yLow);
        ctx.lineTo(x, val ? yHigh : yLow);
        ctx.stroke();
      }
      prevT = ev.tUs;
      prevVal = val;
    }

    const xEnd = view.plotLeft + view.plotW;
    ctx.beginPath();
    ctx.moveTo(toX(prevT), prevVal ? yHigh : yLow);
    ctx.lineTo(xEnd, prevVal ? yHigh : yLow);
    ctx.stroke();
  });
}

function drawCycleMarkers(
  ctx: CanvasRenderingContext2D,
  view: ChartView,
  canvas: HTMLCanvasElement,
) {
  const cycleColor = cssColor(canvas, "--color-warning", "#d97706");
  ctx.save();
  ctx.strokeStyle = cycleColor;
  ctx.lineWidth = 1;
  ctx.setLineDash([5, 4]);
  ctx.globalAlpha = 0.85;

  for (const tTdc of view.tdcTimes) {
    const x = xAtTime(tTdc, view);
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, view.cssH);
    ctx.stroke();

    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
    ctx.fillStyle = cycleColor;
    ctx.font = "10px system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.fillText("0°", x, 11);
    ctx.globalAlpha = 0.85;
    ctx.setLineDash([5, 4]);
  }

  ctx.restore();
}

function drawCrosshair(
  ctx: CanvasRenderingContext2D,
  view: ChartView,
  canvas: HTMLCanvasElement,
  x: number,
  rpm: number | null | undefined,
) {
  const tUs = timeAtX(x, view);
  const angle = crankAngleDeg(tUs, view, rpm);
  const lineColor = cssColor(canvas, "--color-fg", "#e5e7eb");

  ctx.save();
  ctx.strokeStyle = lineColor;
  ctx.lineWidth = 1;
  ctx.setLineDash([]);
  ctx.globalAlpha = 0.55;
  ctx.beginPath();
  ctx.moveTo(x, 0);
  ctx.lineTo(x, view.cssH);
  ctx.stroke();
  ctx.globalAlpha = 1;

  const boxPad = 5;
  const lineH = 13;
  const rows: { text: string; color: string }[] = [
    { text: `° ${angle.toFixed(1)}`, color: lineColor },
  ];

  const dots: { y: number; color: string }[] = [];

  CHANNELS.forEach((ch, idx) => {
    const on = valueAtTime(tUs, view.visible, ch.key);
    const { y } = laneY(idx, view, on);
    dots.push({ y, color: ch.color });
    rows.push({ text: `${ch.label}: ${on ? "1" : "0"}`, color: ch.color });
  });

  for (const d of dots) {
    ctx.fillStyle = d.color;
    ctx.beginPath();
    ctx.arc(x, d.y, 3, 0, Math.PI * 2);
    ctx.fill();
    ctx.strokeStyle = cssColor(canvas, "--color-bg", "#0f1115");
    ctx.lineWidth = 1.5;
    ctx.stroke();
  }

  ctx.font = "10px ui-monospace, monospace";
  const maxW = Math.max(...rows.map((r) => ctx.measureText(r.text).width));
  const tipW = maxW + boxPad * 2;
  const tipH = rows.length * lineH + boxPad * 2;
  const plotRight = view.plotLeft + view.plotW;

  let tipX = x + TIP_OFFSET_X;
  if (tipX + tipW > plotRight - 2) {
    tipX = x - TIP_OFFSET_X - tipW;
  }
  tipX = Math.max(view.plotLeft + 2, tipX);

  const midY =
    dots.length > 0
      ? dots.reduce((s, d) => s + d.y, 0) / dots.length
      : view.cssH / 2;
  let tipY = midY - tipH / 2;
  tipY = Math.max(4, Math.min(tipY, view.cssH - tipH - 4));

  ctx.fillStyle = cssColor(canvas, "--color-bg-elevated", "rgba(20,22,28,0.94)");
  ctx.strokeStyle = cssColor(canvas, "--color-border", "#444");
  ctx.fillRect(tipX, tipY, tipW, tipH);
  ctx.strokeRect(tipX, tipY, tipW, tipH);

  ctx.textAlign = "left";
  ctx.textBaseline = "top";
  rows.forEach((row, i) => {
    ctx.fillStyle = row.color;
    ctx.fillText(row.text, tipX + boxPad, tipY + boxPad + i * lineH);
  });

  ctx.restore();
}

function draw() {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  const dpr = window.devicePixelRatio || 1;
  const cssW = canvas.clientWidth;
  const cssH = chartHeight.value;
  if (cssW <= 0 || cssH <= 0) return;

  canvas.width = Math.floor(cssW * dpr);
  canvas.height = Math.floor(cssH * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  ctx.fillStyle = cssColor(canvas, "--color-bg", "#0f1115");
  ctx.fillRect(0, 0, cssW, cssH);

  const events = snapshot.value.events as CompositeEvent[];
  const timeRange = currentTimeRange(events);
  const view = buildChartView(
    events,
    viewSpanMs.value,
    cssW,
    cssH,
    LABEL_W,
    CHANNELS.length,
    timeRange,
  );

  if (!view) {
    ctx.fillStyle = cssColor(canvas, "--color-gray", "#888");
    ctx.font = "12px system-ui, sans-serif";
    ctx.textAlign = "left";
    ctx.fillText(
      events.length < 2
        ? connected.value
          ? "Ожидание событий триггера (composite logger)…"
          : "Подключите ECU"
        : "Мало точек в окне",
      LABEL_W + 8,
      cssH / 2,
    );
    return;
  }

  const gridColor = cssColor(canvas, "--color-border", "#333");
  ctx.strokeStyle = gridColor;
  ctx.lineWidth = 1;
  ctx.setLineDash([]);
  for (let i = 0; i <= 4; i++) {
    const x = view.plotLeft + (view.plotW * i) / 4;
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, cssH);
    ctx.stroke();
  }

  drawWaveforms(ctx, view, canvas);
  drawCycleMarkers(ctx, view, canvas);

  if (hoverInside.value && hoverX.value != null) {
    drawCrosshair(ctx, view, canvas, hoverX.value, snapshot.value.rpm);
  }

  ctx.fillStyle = cssColor(canvas, "--color-gray", "#6b7280");
  ctx.font = "9px system-ui, sans-serif";
  ctx.textAlign = "right";
  ctx.fillText(`цикл ${CRANK_CYCLE_DEG}°`, view.plotLeft + view.plotW, cssH - 3);
}

function scheduleDraw() {
  requestAnimationFrame(draw);
}

function onPointerMove(e: PointerEvent) {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const rect = canvas.getBoundingClientRect();
  const x = e.clientX - rect.left;
  hoverX.value = x;
  hoverInside.value = x >= LABEL_W && x <= rect.width - 4;
  scheduleDraw();
}

function onPointerLeave() {
  hoverInside.value = false;
  hoverX.value = null;
  scheduleDraw();
}

function plotFracFromClientX(clientX: number): number {
  const wrap = plotWrapRef.value;
  if (!wrap) return 0.5;
  const rect = wrap.getBoundingClientRect();
  const plotW = Math.max(1, rect.width - LABEL_W - 8);
  const x = clientX - rect.left - LABEL_W;
  return Math.min(1, Math.max(0, x / plotW));
}

/** Колёсико вверх (deltaY < 0) — уже окно. */
function zoomAtPointer(clientX: number, zoomIn: boolean) {
  const events = snapshot.value.events as CompositeEvent[];
  if (events.length < 2) return;

  const range = currentTimeRange(events);
  if (!range) return;

  const tFirst = dataT0(events);
  const spanUs = range.tEnd - range.t0;
  const minSpanUs = Math.round(MIN_VIEW_MS * 1000);
  const maxSpanUs = Math.round(maxSpanMsFor(events) * 1000);
  const frac = plotFracFromClientX(clientX);
  const tAnchor = range.t0 + frac * spanUs;

  if (zoomIn) {
    if (spanUs <= minSpanUs) return;
    const newSpanUs = Math.max(minSpanUs, Math.round(spanUs / ZOOM_STEP));
    viewSpanMs.value = newSpanUs / 1000;
    viewAnchorT0Us.value = Math.max(tFirst, tAnchor - frac * newSpanUs);
  } else {
    if (spanUs >= maxSpanUs - 1) {
      viewSpanMs.value = maxSpanMsFor(events);
      viewAnchorT0Us.value = null;
    } else {
      const newSpanUs = Math.min(maxSpanUs, Math.round(spanUs * ZOOM_STEP));
      viewSpanMs.value = newSpanUs / 1000;
      viewAnchorT0Us.value = Math.max(tFirst, tAnchor - frac * newSpanUs);
    }
  }
  clampViewToBuffer(events);
  scheduleDraw();
}

function onCanvasWheel(e: WheelEvent) {
  if ((snapshot.value.events as CompositeEvent[]).length < 2) return;
  if (e.deltaY === 0) return;
  e.preventDefault();
  zoomAtPointer(e.clientX, e.deltaY < 0);
}

function onPlotDblClick() {
  resetViewWindow();
  scheduleDraw();
}

async function applyLoggingEnabled(on: boolean) {
  loggerBusy.value = true;
  loggerError.value = null;
  try {
    await setLoggingEnabled(on);
  } catch (e) {
    loggerError.value = e instanceof Error ? e.message : String(e);
  } finally {
    loggerBusy.value = false;
  }
}

async function loadAutostartFromProject() {
  try {
    const ui = await getProjectUi<CompositeChartUiSettings>(PERSIST_KEY_COMPOSITE_CHART);
    autostart.value = Boolean(ui.autostart);
  } catch {
    autostart.value = false;
  }
}

watch(autostart, (v) => {
  void setProjectUi(PERSIST_KEY_COMPOSITE_CHART, { autostart: v });
});

watch(
  [connected, autostart],
  ([conn, auto]) => {
    if (conn && auto && !loggingEnabled.value && !loggerBusy.value) {
      void applyLoggingEnabled(true);
    }
  },
);

onMounted(async () => {
  await initCompositeLogger();
  await loadAutostartFromProject();
  if (connected.value && autostart.value && !loggingEnabled.value) {
    await applyLoggingEnabled(true);
  }
  const canvas = canvasRef.value;
  if (canvas) {
    ro = new ResizeObserver(scheduleDraw);
    ro.observe(canvas);
    canvas.addEventListener("pointermove", onPointerMove);
    canvas.addEventListener("pointerleave", onPointerLeave);
  }
  resetViewWindow(snapshot.value.events as CompositeEvent[]);
  scheduleDraw();
});

onUnmounted(() => {
  if (loggingEnabled.value) {
    void setLoggingEnabled(false);
  }
  ro?.disconnect();
  const canvas = canvasRef.value;
  canvas?.removeEventListener("pointermove", onPointerMove);
  canvas?.removeEventListener("pointerleave", onPointerLeave);
});

watch(
  () => {
    const ev = snapshot.value.events;
    if (ev.length === 0) return "0";
    return `${ev.length}:${ev[ev.length - 1]!.tUs}`;
  },
  scheduleDraw,
);
watch([maxWindowMs, viewSpanMs, chartHeight, connected], scheduleDraw);
watch([hoverX, hoverInside], scheduleDraw);

const statusLine = computed(() => {
  const s = snapshot.value;
  const ev = s.events as CompositeEvent[];
  const parts: string[] = [];
  if (s.loggingEnabled) parts.push("log on");
  if (s.polling) parts.push("poll");
  if (s.rpm != null) parts.push(`${Math.round(s.rpm)} RPM`);
  const cap = ev.length >= 2 ? maxSpanMsFor(ev) : maxWindowMs.value;
  const win =
    viewSpanMs.value < cap - 0.5
      ? `окно ${viewSpanMs.value.toFixed(0)}/${cap.toFixed(0)} ms`
      : `окно ${cap.toFixed(0)} ms`;
  parts.push(win);
  parts.push(`${s.events.length} pts`);
  if (s.lastBatch > 0) parts.push(`+${s.lastBatch}`);
  return parts.join(" · ");
});
</script>

<template>
  <div class="composite-chart">
    <header class="cc-header">
      <span class="cc-title">Trigger logger</span>
      <span class="cc-status" :class="{ warn: !connected }">{{ statusLine }}</span>
    </header>
    <div class="cc-toolbar">
      <button
        type="button"
        class="btn primary"
        :disabled="!connected || loggerBusy || loggingEnabled"
        @click="applyLoggingEnabled(true)"
      >
        Старт
      </button>
      <button
        type="button"
        class="btn secondary"
        :disabled="!connected || loggerBusy || !loggingEnabled"
        @click="applyLoggingEnabled(false)"
      >
        Стоп
      </button>
      <label class="cc-autostart">
        <input v-model="autostart" type="checkbox" :disabled="loggerBusy" />
        Автозапуск при подключении
      </label>
    </div>
    <p v-if="loggerError" class="cc-error">{{ loggerError }}</p>
    <div
      ref="plotWrapRef"
      class="cc-plot-wrap"
      title="Колёсико — масштаб, двойной щелчок — сброс"
      @wheel.prevent="onCanvasWheel"
      @dblclick="onPlotDblClick"
    >
      <canvas
        ref="canvasRef"
        class="cc-canvas"
        :style="{ height: `${chartHeight}px` }"
        aria-label="Composite trigger logger"
      />
    </div>
    <p v-if="snapshot.lastError" class="cc-error">{{ snapshot.lastError }}</p>
    <p v-else-if="connected && !loggingEnabled" class="cc-hint">
      Нажмите «Старт» или включите автозапуск.
    </p>
    <p v-else class="cc-hint">
      Колёсико над графиком — масштаб (до {{ maxWindowMs }} ms и ширины буфера). Двойной щелчок — сброс.
      TDC = 0°, цикл {{ CRANK_CYCLE_DEG }}°.
    </p>
  </div>
</template>

<style scoped>
.composite-chart {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
  padding: 0.5rem 0.65rem;
  background: var(--color-bg-muted);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  min-width: 280px;
}

.cc-header {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  justify-content: space-between;
  gap: 0.35rem 0.75rem;
}

.cc-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
}

.cc-toolbar .btn {
  font-size: 0.75rem;
  padding: 0.25rem 0.65rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border);
  cursor: pointer;
}

.cc-toolbar .btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.cc-toolbar .btn.primary {
  background: var(--color-accent, #3b82f6);
  color: #fff;
  border-color: transparent;
}

.cc-toolbar .btn.secondary {
  background: var(--color-bg);
  color: var(--color-fg);
}

.cc-autostart {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  font-size: 0.72rem;
  color: var(--color-gray);
  margin-left: auto;
  cursor: pointer;
}

.cc-autostart input {
  margin: 0;
}

.cc-title {
  font-size: 0.8rem;
  font-weight: 600;
  color: var(--color-fg);
}

.cc-status {
  font-size: 0.72rem;
  color: var(--color-gray);
  font-variant-numeric: tabular-nums;
}

.cc-status.warn {
  color: var(--color-warning, #d97706);
}

.cc-plot-wrap {
  width: 100%;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border);
  overflow: hidden;
}

.cc-canvas {
  width: 100%;
  display: block;
  cursor: crosshair;
}

.cc-error {
  margin: 0;
  font-size: 0.72rem;
  color: var(--color-danger, #dc2626);
}

.cc-hint {
  margin: 0;
  font-size: 0.72rem;
  color: var(--color-gray);
}
</style>
