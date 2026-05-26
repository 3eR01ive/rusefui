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
  buildChartView,
  channelValue,
  crankAngleDeg,
  CRANK_CYCLE_DEG,
  laneY,
  timeAtX,
  valueAtTime,
  xAtTime,
  type ChannelKey,
  type ChartView,
} from "./compositeChartGeometry";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const windowMs = computed(() => Number(props.props.windowMs ?? 500));
const chartHeight = computed(() => Math.max(120, Number(props.props.height ?? 220)));

const { snapshot } = useCompositeLogger();
const dataCtx = useDataContext();
const connected = computed(() => dataCtx.connection.value.connected);

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

  const angleLabel = `${angle.toFixed(1)}°`;
  ctx.font = "bold 11px system-ui, sans-serif";
  const angleW = ctx.measureText(angleLabel).width;
  const boxPad = 4;
  const angleBoxW = angleW + boxPad * 2;
  let boxX = x - angleBoxW / 2;
  boxX = Math.max(view.plotLeft, Math.min(boxX, view.plotLeft + view.plotW - angleBoxW));

  ctx.fillStyle = cssColor(canvas, "--color-bg-elevated", "rgba(20,22,28,0.92)");
  ctx.strokeStyle = lineColor;
  ctx.lineWidth = 1;
  ctx.fillRect(boxX, 2, angleBoxW, 16);
  ctx.strokeRect(boxX, 2, angleBoxW, 16);
  ctx.fillStyle = lineColor;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(angleLabel, boxX + angleBoxW / 2, 10);

  const labels: string[] = [];
  const dots: { y: number; color: string }[] = [];

  CHANNELS.forEach((ch, idx) => {
    const on = valueAtTime(tUs, view.visible, ch.key);
    const { y } = laneY(idx, view, on);
    dots.push({ y, color: ch.color });
    labels.push(`${ch.label}: ${on ? "1" : "0"}`);
  });

  for (const d of dots) {
    ctx.fillStyle = d.color;
    ctx.beginPath();
    ctx.arc(x, d.y, 4, 0, Math.PI * 2);
    ctx.fill();
    ctx.strokeStyle = cssColor(canvas, "--color-bg", "#0f1115");
    ctx.lineWidth = 1.5;
    ctx.stroke();
  }

  ctx.font = "10px ui-monospace, monospace";
  const lineH = 13;
  const maxW = Math.max(...labels.map((l) => ctx.measureText(l).width));
  const tipW = maxW + boxPad * 2;
  const tipH = labels.length * lineH + boxPad * 2;
  let tipX = x + 10;
  if (tipX + tipW > view.plotLeft + view.plotW) {
    tipX = x - 10 - tipW;
  }
  let tipY = 22;
  if (tipY + tipH > view.cssH) tipY = view.cssH - tipH - 4;

  ctx.fillStyle = cssColor(canvas, "--color-bg-elevated", "rgba(20,22,28,0.94)");
  ctx.strokeStyle = cssColor(canvas, "--color-border", "#444");
  ctx.fillRect(tipX, tipY, tipW, tipH);
  ctx.strokeRect(tipX, tipY, tipW, tipH);

  ctx.textAlign = "left";
  ctx.textBaseline = "top";
  labels.forEach((text, i) => {
    const ch = CHANNELS[i]!;
    ctx.fillStyle = ch.color;
    ctx.fillText(text, tipX + boxPad, tipY + boxPad + i * lineH);
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
  const view = buildChartView(
    events,
    windowMs.value,
    cssW,
    cssH,
    LABEL_W,
    CHANNELS.length,
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

onMounted(async () => {
  await initCompositeLogger();
  const canvas = canvasRef.value;
  if (canvas) {
    ro = new ResizeObserver(scheduleDraw);
    ro.observe(canvas);
    canvas.addEventListener("pointermove", onPointerMove);
    canvas.addEventListener("pointerleave", onPointerLeave);
  }
  scheduleDraw();
});

onUnmounted(() => {
  ro?.disconnect();
  const canvas = canvasRef.value;
  canvas?.removeEventListener("pointermove", onPointerMove);
  canvas?.removeEventListener("pointerleave", onPointerLeave);
});

watch([snapshot, windowMs, chartHeight, connected, hoverX, hoverInside], scheduleDraw, {
  deep: true,
});

const statusLine = computed(() => {
  const s = snapshot.value;
  const parts: string[] = [];
  if (s.polling) parts.push("poll");
  if (s.rpm != null) parts.push(`${Math.round(s.rpm)} RPM`);
  parts.push(`${s.events.length} в окне`);
  parts.push(`${s.totalEvents} всего`);
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
    <canvas
      ref="canvasRef"
      class="cc-canvas"
      :style="{ height: `${chartHeight}px` }"
      aria-label="Composite trigger logger"
    />
    <p v-if="snapshot.lastError" class="cc-error">{{ snapshot.lastError }}</p>
    <p v-else-if="connected && !snapshot.polling" class="cc-hint">
      Опрос composite не запущен (загрузка config или отключение).
    </p>
    <p v-else class="cc-hint">
      Вертикальные штрихи — TDC (0°), полный цикл {{ CRANK_CYCLE_DEG }}°. Наведите курсор для угла и
      значений.
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

.cc-canvas {
  width: 100%;
  display: block;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border);
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
