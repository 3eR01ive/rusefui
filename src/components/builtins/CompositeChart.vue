<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useDataContext } from "../../core/data-context";
import {
  initCompositeLogger,
  useCompositeLogger,
  type CompositeEvent,
} from "../../composables/useCompositeLogger";

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
let ro: ResizeObserver | null = null;

const CHANNELS = [
  { key: "pri" as const, label: "Pri", color: "#3b82f6" },
  { key: "sec" as const, label: "Sec", color: "#8b5cf6" },
  { key: "trg" as const, label: "TDC", color: "#f59e0b" },
  { key: "sync" as const, label: "Sync", color: "#10b981" },
  { key: "coil" as const, label: "Coil", color: "#ef4444" },
  { key: "inj" as const, label: "Inj", color: "#06b6d4" },
];

const LABEL_W = 44;

function channelValue(ev: CompositeEvent, key: (typeof CHANNELS)[number]["key"]): boolean {
  return ev[key];
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

  ctx.fillStyle = getComputedStyle(canvas).getPropertyValue("--color-bg").trim() || "#0f1115";
  ctx.fillRect(0, 0, cssW, cssH);

  const events = snapshot.value.events;
  if (events.length < 2) {
    ctx.fillStyle = getComputedStyle(canvas).getPropertyValue("--color-gray").trim() || "#888";
    ctx.font = "12px system-ui, sans-serif";
    ctx.fillText(
      connected.value
        ? "Ожидание событий триггера (composite logger)…"
        : "Подключите ECU",
      LABEL_W + 8,
      cssH / 2,
    );
    return;
  }

  const tEnd = events[events.length - 1]!.tUs;
  const windowUs = Math.round(windowMs.value * 1000);
  const tStart = Math.max(0, tEnd - windowUs);

  const plotW = cssW - LABEL_W - 8;
  const laneH = (cssH - 8) / CHANNELS.length;

  const t0 = Math.max(0, tStart);
  const span = Math.max(1, tEnd - t0);

  const visible = events.filter((e) => e.tUs >= t0 && e.tUs <= tEnd);
  if (visible.length < 2) {
    ctx.fillStyle = "#888";
    ctx.font = "12px system-ui";
    ctx.fillText("Мало точек в окне", LABEL_W + 8, cssH / 2);
    return;
  }

  const gridColor =
    getComputedStyle(canvas).getPropertyValue("--color-border").trim() || "#333";
  ctx.strokeStyle = gridColor;
  ctx.lineWidth = 1;
  for (let i = 0; i <= 4; i++) {
    const x = LABEL_W + (plotW * i) / 4;
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, cssH);
    ctx.stroke();
  }

  CHANNELS.forEach((ch, idx) => {
    const y0 = idx * laneH + 4;
    const yMid = y0 + laneH * 0.5;
    const yHigh = y0 + laneH * 0.22;
    const yLow = y0 + laneH * 0.78;

    ctx.fillStyle =
      getComputedStyle(canvas).getPropertyValue("--color-gray").trim() || "#9ca3af";
    ctx.font = "11px system-ui, sans-serif";
    ctx.textAlign = "right";
    ctx.fillText(ch.label, LABEL_W - 6, yMid + 4);

    const toX = (tUs: number) => LABEL_W + ((tUs - t0) / span) * plotW;

    let prevT = t0;
    let prevVal = channelValue(visible[0]!, ch.key);
    for (const ev of visible) {
      const x = toX(ev.tUs);
      const val = channelValue(ev, ch.key);
      if (ev.tUs > prevT) {
        ctx.strokeStyle = ch.color;
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(toX(prevT), prevVal ? yHigh : yLow);
        ctx.lineTo(x, prevVal ? yHigh : yLow);
        ctx.stroke();
      }
      if (val !== prevVal) {
        ctx.strokeStyle = ch.color;
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(x, prevVal ? yHigh : yLow);
        ctx.lineTo(x, val ? yHigh : yLow);
        ctx.stroke();
      }
      prevT = ev.tUs;
      prevVal = val;
    }

    const xEnd = LABEL_W + plotW;
    ctx.strokeStyle = ch.color;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(toX(prevT), prevVal ? yHigh : yLow);
    ctx.lineTo(xEnd, prevVal ? yHigh : yLow);
    ctx.stroke();
  });
}

function scheduleDraw() {
  requestAnimationFrame(draw);
}

onMounted(async () => {
  await initCompositeLogger();
  const canvas = canvasRef.value;
  if (canvas) {
    ro = new ResizeObserver(scheduleDraw);
    ro.observe(canvas);
  }
  scheduleDraw();
});

onUnmounted(() => {
  ro?.disconnect();
});

watch([snapshot, windowMs, chartHeight, connected], scheduleDraw, { deep: true });

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
