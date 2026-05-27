<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import type { CrankEdgeMode } from "../../composables/useProject";

export interface WheelTooth {
  angleDeg: number;
  kind: "rise" | "fall" | "tooth";
  fallAngleDeg?: number | null;
}

export interface TriggerWheelDisk {
  label: string;
  teeth: WheelTooth[];
  arcSpanDeg: number;
  logicalTdcDeg: number;
  offsetTdcDeg: number | null;
  eventsPerCycle: number;
}

export interface TriggerWheelsView {
  crank: TriggerWheelDisk;
  cam: TriggerWheelDisk;
  cyclesUsed: number;
  cyclesSeen: number;
  message?: string | null;
}

const props = defineProps<{
  view: TriggerWheelsView | null;
  edgeMode: CrankEdgeMode;
  height?: number;
}>();

const wrapRef = ref<HTMLDivElement | null>(null);
const canvasRef = ref<HTMLCanvasElement | null>(null);
let ro: ResizeObserver | null = null;

function cssColor(canvas: HTMLCanvasElement, varName: string, fallback: string): string {
  const v = getComputedStyle(canvas).getPropertyValue(varName).trim();
  return v || fallback;
}

function toRad(deg: number): number {
  return ((deg - 90) * Math.PI) / 180;
}

/** Восходящий — стрелка от внутреннего радиуса наружу. */
function drawRiseArrow(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  innerR: number,
  outerR: number,
  angleDeg: number,
  color: string,
) {
  const a = toRad(angleDeg);
  const cos = Math.cos(a);
  const sin = Math.sin(a);
  const x0 = cx + (innerR + 6) * cos;
  const y0 = cy + (innerR + 6) * sin;
  const x1 = cx + (outerR - 4) * cos;
  const y1 = cy + (outerR - 4) * sin;

  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = 1.5;
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(x0, y0);
  ctx.lineTo(x1, y1);
  ctx.stroke();

  const ux = cos;
  const uy = sin;
  const head = 5;
  ctx.beginPath();
  ctx.moveTo(x1, y1);
  ctx.lineTo(x1 - ux * 4 + uy * head, y1 - uy * 4 - ux * head);
  ctx.lineTo(x1 - ux * 4 - uy * head, y1 - uy * 4 + ux * head);
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

/** Нисходящий — стрелка от внешнего радиуса внутрь. */
function drawFallArrow(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  innerR: number,
  outerR: number,
  angleDeg: number,
  color: string,
) {
  const a = toRad(angleDeg);
  const cos = Math.cos(a);
  const sin = Math.sin(a);
  const x0 = cx + (outerR - 4) * cos;
  const y0 = cy + (outerR - 4) * sin;
  const x1 = cx + (innerR + 6) * cos;
  const y1 = cy + (innerR + 6) * sin;

  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = 1.5;
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(x0, y0);
  ctx.lineTo(x1, y1);
  ctx.stroke();

  const ux = -cos;
  const uy = -sin;
  const head = 5;
  ctx.beginPath();
  ctx.moveTo(x1, y1);
  ctx.lineTo(x1 - ux * 4 + uy * head, y1 - uy * 4 - ux * head);
  ctx.lineTo(x1 - ux * 4 - uy * head, y1 - uy * 4 + ux * head);
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

/** Режим ↕: дуга по внешнему радиусу между вершинами rise и fall. */
function drawToothChord(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  outerR: number,
  riseDeg: number,
  fallDeg: number,
  color: string,
) {
  const a0 = toRad(riseDeg);
  const a1 = toRad(fallDeg);
  const x0 = cx + outerR * Math.cos(a0);
  const y0 = cy + outerR * Math.sin(a0);
  const x1 = cx + outerR * Math.cos(a1);
  const y1 = cy + outerR * Math.sin(a1);

  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = 2;
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(x0, y0);
  ctx.lineTo(x1, y1);
  ctx.stroke();
  ctx.restore();
}

function drawDisk(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  r: number,
  disk: TriggerWheelDisk,
  edgeMode: CrankEdgeMode,
  colors: {
    rim: string;
    hub: string;
    rise: string;
    fall: string;
    chord: string;
    logical: string;
    offset: string;
    label: string;
  },
) {
  const innerR = r * 0.62;
  const outerR = r * 0.98;

  ctx.save();
  ctx.fillStyle = colors.hub;
  ctx.beginPath();
  ctx.arc(cx, cy, innerR * 0.92, 0, Math.PI * 2);
  ctx.fill();

  ctx.strokeStyle = colors.rim;
  ctx.lineWidth = 2;
  ctx.beginPath();
  ctx.arc(cx, cy, outerR, 0, Math.PI * 2);
  ctx.stroke();
  ctx.beginPath();
  ctx.arc(cx, cy, innerR, 0, Math.PI * 2);
  ctx.stroke();

  for (const tooth of disk.teeth) {
    if (edgeMode === "both" && tooth.kind === "tooth" && tooth.fallAngleDeg != null) {
      drawRiseArrow(ctx, cx, cy, innerR, outerR, tooth.angleDeg, colors.rise);
      drawFallArrow(ctx, cx, cy, innerR, outerR, tooth.fallAngleDeg, colors.fall);
      drawToothChord(ctx, cx, cy, outerR, tooth.angleDeg, tooth.fallAngleDeg, colors.chord);
      continue;
    }
    if (edgeMode === "rise" && tooth.kind === "rise") {
      drawRiseArrow(ctx, cx, cy, innerR, outerR, tooth.angleDeg, colors.rise);
    }
    if (edgeMode === "fall" && tooth.kind === "fall") {
      drawFallArrow(ctx, cx, cy, innerR, outerR, tooth.angleDeg, colors.fall);
    }
  }

  const drawTdcLine = (deg: number, color: string, dash: number[]) => {
    const a = toRad(deg);
    ctx.save();
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    ctx.setLineDash(dash);
    ctx.beginPath();
    ctx.moveTo(cx + innerR * 0.4 * Math.cos(a), cy + innerR * 0.4 * Math.sin(a));
    ctx.lineTo(cx + (outerR + 6) * Math.cos(a), cy + (outerR + 6) * Math.sin(a));
    ctx.stroke();
    ctx.restore();
  };

  drawTdcLine(disk.logicalTdcDeg, colors.logical, []);
  if (disk.offsetTdcDeg != null) {
    drawTdcLine(disk.offsetTdcDeg, colors.offset, [5, 4]);
  }

  ctx.fillStyle = colors.label;
  ctx.font = "600 14px system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  const sub =
    disk.eventsPerCycle > 0
      ? ` · ${disk.eventsPerCycle} событ./цикл · ${disk.arcSpanDeg.toFixed(0)}°`
      : "";
  ctx.fillText(disk.label + sub, cx, cy + outerR + 12);
  ctx.restore();
}

function layoutHeight(cssW: number): number {
  if (props.height != null && props.height > 0) return props.height;
  return Math.round(Math.min(580, Math.max(400, cssW * 0.52)));
}

function diskRadius(cssW: number, diskAreaH: number): number {
  const halfW = cssW * 0.5;
  return Math.max(48, Math.min(halfW * 0.88 - 20, diskAreaH * 0.48 - 8));
}

function draw() {
  const canvas = canvasRef.value;
  const wrap = wrapRef.value;
  if (!canvas || !wrap) return;
  const view = props.view;
  const dpr = window.devicePixelRatio || 1;
  const cssW = wrap.clientWidth;
  const cssH = layoutHeight(cssW);
  if (cssW <= 0 || cssH <= 0) return;

  canvas.width = Math.floor(cssW * dpr);
  canvas.height = Math.floor(cssH * dpr);
  canvas.style.width = `${cssW}px`;
  canvas.style.height = `${cssH}px`;

  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = cssColor(canvas, "--color-bg", "#0f1115");
  ctx.fillRect(0, 0, cssW, cssH);

  const footerH = 28;
  const diskAreaH = cssH - footerH;

  if (!view) {
    ctx.fillStyle = cssColor(canvas, "--color-gray", "#888");
    ctx.font = "13px system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.fillText("Нет данных для дисков", cssW / 2, diskAreaH / 2);
    return;
  }

  const colors = {
    rim: cssColor(canvas, "--color-border", "#555"),
    hub: cssColor(canvas, "--color-bg-muted", "#1a1d24"),
    rise: cssColor(canvas, "--color-accent", "#3b82f6"),
    fall: cssColor(canvas, "--color-warning", "#d97706"),
    chord: cssColor(canvas, "--color-fg-muted", "#9ca3af"),
    logical: cssColor(canvas, "--color-success", "#22c55e"),
    offset: cssColor(canvas, "--color-error", "#ef4444"),
    label: cssColor(canvas, "--color-fg", "#e5e7eb"),
  };

  const r = diskRadius(cssW, diskAreaH);
  const cy = diskAreaH * 0.46;
  drawDisk(ctx, cssW * 0.25, cy, r, view.crank, props.edgeMode, colors);
  drawDisk(ctx, cssW * 0.75, cy, r, view.cam, props.edgeMode, colors);

  ctx.fillStyle = cssColor(canvas, "--color-gray", "#888");
  ctx.font = "11px system-ui, sans-serif";
  ctx.textAlign = "left";
  ctx.fillText(view.message ?? "", 12, cssH - 10);
}

function scheduleDraw() {
  requestAnimationFrame(draw);
}

onMounted(() => {
  if (wrapRef.value) {
    ro = new ResizeObserver(scheduleDraw);
    ro.observe(wrapRef.value);
  }
  scheduleDraw();
});

onUnmounted(() => {
  ro?.disconnect();
});

watch(() => [props.view, props.edgeMode, props.height], scheduleDraw, { deep: true });
</script>

<template>
  <div ref="wrapRef" class="cc-wheels">
    <canvas ref="canvasRef" class="cc-wheels-canvas" aria-label="Диски триггеров" />
  </div>
</template>

<style scoped>
.cc-wheels {
  width: 100%;
  min-height: 400px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border);
  overflow: hidden;
  background: var(--color-bg);
}

.cc-wheels-canvas {
  display: block;
  width: 100%;
}
</style>
