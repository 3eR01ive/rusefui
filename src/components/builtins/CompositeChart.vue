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
  bufferSpanMs,
  buildChartView,
  channelValue,
  crankAngleDeg,
  CRANK_CYCLE_DEG,
  laneY,
  maxViewSpanMs,
  snapT0ToTdc,
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
const defaultAutoStopSec = computed(() =>
  Math.max(0, Math.round(Number(props.props.autoStopSec ?? 0))),
);

const MIN_VIEW_MS = 5;
const ZOOM_STEP = 1.12;

/** Ширина окна просмотра (мс). */
const viewSpanMs = ref(maxWindowMs.value);
/** Левый край окна просмотра (µs). */
const viewAnchorT0Us = ref<number | null>(null);
/** Пользователь менял зум/пан — не подстраивать вид во время записи. */
const userAdjustedView = ref(false);

const { snapshot, setLoggingEnabled } = useCompositeLogger();
const { getProjectUi, setProjectUi } = useProject();
const dataCtx = useDataContext();
const connected = computed(() => dataCtx.connection.value.connected);
const loggingEnabled = computed(() => snapshot.value.loggingEnabled);
const autostart = ref(false);
const alignTdc = ref(false);
const autoStopSec = ref(0);
const autoStopRemainingSec = ref<number | null>(null);
const loggerBusy = ref(false);
const loggerError = ref<string | null>(null);

let autoStopTimer: ReturnType<typeof setInterval> | null = null;
let autoStopDeadlineMs = 0;

const plotWrapRef = ref<HTMLDivElement | null>(null);
const canvasRef = ref<HTMLCanvasElement | null>(null);
const hoverX = ref<number | null>(null);
const hoverInside = ref(false);
let ro: ResizeObserver | null = null;
let panPointerId: number | null = null;
let panStartClientX = 0;
let panStartT0Us = 0;

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

function captureStartT0(events: readonly CompositeEvent[]): number {
  let t0 = dataT0(events);
  if (alignTdc.value) {
    t0 = snapT0ToTdc(events, t0);
  }
  return t0;
}

/** Весь склеенный захват в окне просмотра. */
function fitFullCapture(events: readonly CompositeEvent[]) {
  if (events.length < 2) return;
  viewAnchorT0Us.value = captureStartT0(events);
  viewSpanMs.value = Math.max(MIN_VIEW_MS, bufferSpanMs(events));
  userAdjustedView.value = false;
  clampViewToBuffer(events);
}

/** Во время записи — показывать всё накопленное с начала сессии. */
function fitGrowingCapture(events: readonly CompositeEvent[]) {
  if (userAdjustedView.value || events.length < 2) return;
  fitFullCapture(events);
}

function currentTimeRange(events: readonly CompositeEvent[]): ChartTimeRange | null {
  if (events.length < 2) return null;
  const dataStart = dataT0(events);
  const dataEnd = events[events.length - 1]!.tUs;
  const spanUs = Math.round(viewSpanMs.value * 1000);

  let t0 = viewAnchorT0Us.value ?? captureStartT0(events);
  if (alignTdc.value) {
    t0 = snapT0ToTdc(events, t0);
  }

  const maxT0 = Math.max(dataStart, dataEnd - spanUs);
  if (t0 > maxT0) t0 = maxT0;
  if (t0 < dataStart) t0 = dataStart;

  return { t0, tEnd: t0 + spanUs, spanUs };
}

function resetViewWindow() {
  const events = snapshot.value.events as CompositeEvent[];
  if (events.length >= 2) {
    fitFullCapture(events);
  } else {
    viewSpanMs.value = maxWindowMs.value;
    viewAnchorT0Us.value = null;
    userAdjustedView.value = false;
  }
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
    return `${ev.length}:${ev[0]!.tUs}:${ev[ev.length - 1]!.tUs}`;
  },
  () => {
    const events = snapshot.value.events as CompositeEvent[];
    clampViewToBuffer(events);
    if (loggingEnabled.value) {
      fitGrowingCapture(events);
    }
  },
);

watch(loggingEnabled, (on, wasOn) => {
  if (wasOn && !on) {
    const events = snapshot.value.events as CompositeEvent[];
    if (events.length >= 2) {
      fitFullCapture(events);
      scheduleDraw();
    }
  }
});

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

  for (const { tUs, cycle } of view.tdcMarkers) {
    const x = xAtTime(tUs, view);
    if (x < view.plotLeft - 2 || x > view.plotLeft + view.plotW + 2) continue;

    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, view.cssH);
    ctx.stroke();

    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
    ctx.fillStyle = cycleColor;
    ctx.font = "bold 10px system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    ctx.fillText(`#${cycle}`, x, 4);
    ctx.font = "9px system-ui, sans-serif";
    ctx.fillText("0°", x, 16);
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

function plotWidthPx(): number {
  const wrap = plotWrapRef.value;
  if (!wrap) return 1;
  return Math.max(1, wrap.getBoundingClientRect().width - LABEL_W - 8);
}

function panByClientDelta(deltaX: number) {
  const events = snapshot.value.events as CompositeEvent[];
  if (events.length < 2) return;
  const range = currentTimeRange(events);
  if (!range) return;

  const dtUs = (-deltaX / plotWidthPx()) * range.spanUs;
  const dataStart = dataT0(events);
  const dataEnd = events[events.length - 1]!.tUs;
  const spanUs = range.spanUs;
  let t0 = panStartT0Us + dtUs;
  const maxT0 = Math.max(dataStart, dataEnd - spanUs);
  t0 = Math.min(maxT0, Math.max(dataStart, t0));
  if (alignTdc.value) {
    t0 = snapT0ToTdc(events, t0);
    if (t0 > maxT0) t0 = maxT0;
  }
  viewAnchorT0Us.value = t0;
  scheduleDraw();
}

function onPointerDown(e: PointerEvent) {
  const events = snapshot.value.events as CompositeEvent[];
  if (e.button !== 0 || events.length < 2) return;
  const range = currentTimeRange(events);
  if (!range) return;

  const canvas = canvasRef.value;
  if (!canvas) return;

  panPointerId = e.pointerId;
  panStartClientX = e.clientX;
  panStartT0Us = range.t0;
  viewAnchorT0Us.value = range.t0;
  userAdjustedView.value = true;
  canvas.setPointerCapture(e.pointerId);
}

function onPointerMove(e: PointerEvent) {
  const canvas = canvasRef.value;
  if (!canvas) return;

  if (panPointerId === e.pointerId) {
    panByClientDelta(e.clientX - panStartClientX);
    return;
  }

  const rect = canvas.getBoundingClientRect();
  const x = e.clientX - rect.left;
  hoverX.value = x;
  hoverInside.value = x >= LABEL_W && x <= rect.width - 4;
  scheduleDraw();
}

function endPan(e: PointerEvent) {
  if (panPointerId !== e.pointerId) return;
  panPointerId = null;
  canvasRef.value?.releasePointerCapture(e.pointerId);
}

function onPointerUp(e: PointerEvent) {
  endPan(e);
}

function onPointerCancel(e: PointerEvent) {
  endPan(e);
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

  const spanUs = range.spanUs;
  const minSpanUs = Math.round(MIN_VIEW_MS * 1000);
  const maxSpanUs = Math.round(maxSpanMsFor(events) * 1000);
  const frac = plotFracFromClientX(clientX);
  const tAnchor = range.t0 + frac * spanUs;

  userAdjustedView.value = true;

  if (zoomIn) {
    if (spanUs <= minSpanUs) return;
    const newSpanUs = Math.max(minSpanUs, Math.round(spanUs / ZOOM_STEP));
    viewSpanMs.value = newSpanUs / 1000;
    viewAnchorT0Us.value = tAnchor - frac * newSpanUs;
  } else {
    if (spanUs >= maxSpanUs - 1) {
      fitFullCapture(events);
      return;
    }
    const newSpanUs = Math.min(maxSpanUs, Math.round(spanUs * ZOOM_STEP));
    viewSpanMs.value = newSpanUs / 1000;
    viewAnchorT0Us.value = tAnchor - frac * newSpanUs;
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

function clearAutoStopTimer() {
  if (autoStopTimer != null) {
    clearInterval(autoStopTimer);
    autoStopTimer = null;
  }
  autoStopRemainingSec.value = null;
  autoStopDeadlineMs = 0;
}

function startAutoStopTimer() {
  clearAutoStopTimer();
  const sec = Math.round(autoStopSec.value);
  if (sec <= 0) return;

  autoStopDeadlineMs = Date.now() + sec * 1000;
  autoStopRemainingSec.value = sec;

  autoStopTimer = setInterval(() => {
    const left = Math.ceil((autoStopDeadlineMs - Date.now()) / 1000);
    if (left <= 0) {
      clearAutoStopTimer();
      if (loggingEnabled.value && !loggerBusy.value) {
        void applyLoggingEnabled(false);
      }
      return;
    }
    autoStopRemainingSec.value = left;
  }, 250);
}

async function applyLoggingEnabled(on: boolean) {
  loggerBusy.value = true;
  loggerError.value = null;
  try {
    await setLoggingEnabled(on);
    if (on) {
      userAdjustedView.value = false;
      viewAnchorT0Us.value = null;
      viewSpanMs.value = maxWindowMs.value;
      startAutoStopTimer();
    } else {
      clearAutoStopTimer();
      const events = snapshot.value.events as CompositeEvent[];
      if (events.length >= 2) {
        fitFullCapture(events);
      }
    }
  } catch (e) {
    loggerError.value = e instanceof Error ? e.message : String(e);
    clearAutoStopTimer();
  } finally {
    loggerBusy.value = false;
  }
}

async function loadUiFromProject() {
  try {
    const ui = await getProjectUi<CompositeChartUiSettings>(PERSIST_KEY_COMPOSITE_CHART);
    autostart.value = Boolean(ui.autostart);
    alignTdc.value = Boolean(ui.alignTdc);
    if (ui.autoStopSec != null && ui.autoStopSec >= 0) {
      autoStopSec.value = Math.round(ui.autoStopSec);
    } else {
      autoStopSec.value = defaultAutoStopSec.value;
    }
  } catch {
    autostart.value = false;
    alignTdc.value = false;
    autoStopSec.value = defaultAutoStopSec.value;
  }
}

function persistUiSettings() {
  void setProjectUi(PERSIST_KEY_COMPOSITE_CHART, {
    autostart: autostart.value,
    alignTdc: alignTdc.value,
    autoStopSec: Math.max(0, Math.round(autoStopSec.value)),
  });
}

watch(autostart, persistUiSettings);
watch(alignTdc, () => {
  persistUiSettings();
  scheduleDraw();
});
watch(autoStopSec, () => {
  persistUiSettings();
  if (loggingEnabled.value) startAutoStopTimer();
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
  await loadUiFromProject();
  if (connected.value && autostart.value && !loggingEnabled.value) {
    await applyLoggingEnabled(true);
  }
  const canvas = canvasRef.value;
  if (canvas) {
    ro = new ResizeObserver(scheduleDraw);
    ro.observe(canvas);
    canvas.addEventListener("pointerdown", onPointerDown);
    canvas.addEventListener("pointermove", onPointerMove);
    canvas.addEventListener("pointerup", onPointerUp);
    canvas.addEventListener("pointercancel", onPointerCancel);
    canvas.addEventListener("pointerleave", onPointerLeave);
  }
  resetViewWindow();
  scheduleDraw();
});

onUnmounted(() => {
  clearAutoStopTimer();
  if (loggingEnabled.value) {
    void setLoggingEnabled(false);
  }
  ro?.disconnect();
  const canvas = canvasRef.value;
  canvas?.removeEventListener("pointerdown", onPointerDown);
  canvas?.removeEventListener("pointermove", onPointerMove);
  canvas?.removeEventListener("pointerup", onPointerUp);
  canvas?.removeEventListener("pointercancel", onPointerCancel);
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
watch([maxWindowMs, viewSpanMs, chartHeight, connected, alignTdc], scheduleDraw);
watch([hoverX, hoverInside], scheduleDraw);

const statusLine = computed(() => {
  const s = snapshot.value;
  const ev = s.events as CompositeEvent[];
  const parts: string[] = [];
  if (s.loggingEnabled) parts.push("log on");
  if (autoStopRemainingSec.value != null && autoStopRemainingSec.value > 0) {
    parts.push(`стоп ${autoStopRemainingSec.value} с`);
  }
  if (s.polling) parts.push("poll");
  if (s.rpm != null) parts.push(`${Math.round(s.rpm)} RPM`);
  const cap = ev.length >= 2 ? maxSpanMsFor(ev) : maxWindowMs.value;
  const rec = s.recordedSpanMs > 0 ? s.recordedSpanMs : ev.length >= 2 ? bufferSpanMs(ev) : 0;
  const win =
    viewSpanMs.value < cap - 0.5
      ? `вид ${viewSpanMs.value.toFixed(0)}/${cap.toFixed(0)} ms`
      : `вид ${cap.toFixed(0)} ms`;
  parts.push(win);
  if (rec > 0 && !s.loggingEnabled) {
    parts.push(`захват ${rec.toFixed(0)} ms`);
  }
  if (s.recordedSpanMs > 0) {
    parts.push(`запись ${s.recordedSpanMs.toFixed(0)} ms`);
  }
  parts.push(`${s.events.length} pts`);
  if (s.tdcCyclesTotal > 0) parts.push(`TDC #${s.tdcCyclesTotal}`);
  if (s.chunksReceived > 0) parts.push(`${s.chunksReceived} chunk`);
  if (s.lastBatch > 0) parts.push(`+${s.lastBatch}`);
  if (s.lastChunkGapMs > 1) parts.push(`разрыв ${s.lastChunkGapMs.toFixed(1)} ms`);
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
      <label class="cc-autostart">
        <input v-model="alignTdc" type="checkbox" :disabled="loggerBusy" />
        Выравнивать по TDC
      </label>
      <label class="cc-timer">
        <span>Стоп через</span>
        <input
          v-model.number="autoStopSec"
          type="number"
          min="0"
          max="86400"
          step="1"
          :disabled="loggerBusy"
          title="0 — без автоматического стопа"
        />
        <span>с</span>
      </label>
    </div>
    <p v-if="loggerError" class="cc-error">{{ loggerError }}</p>
    <div
      ref="plotWrapRef"
      class="cc-plot-wrap"
      title="Запись → стоп → весь захват. Колёсико — масштаб, перетаскивание — перемотка, двойной щелчок — весь захват"
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
    <p v-else-if="connected && !loggingEnabled && snapshot.events.length < 2" class="cc-hint">
      «Старт» — запись с ECU, «Стоп» — склеить сессию и просмотреть весь захват (зум, перемотка).
    </p>
    <p v-else-if="!loggingEnabled && snapshot.events.length >= 2" class="cc-hint">
      Захват готов: {{ snapshot.events.length }} точек, {{ snapshot.recordedSpanMs.toFixed(0) }} ms.
      Колёсико — масштаб, перетаскивание — перемотка, двойной щелчок — показать весь захват.
    </p>
    <p v-else class="cc-hint">
      Идёт запись: куски ECU склеиваются в одну сессию. «Стоп» — остановить приём и просмотреть
      всё накопленное. TDC = 0°, цикл {{ CRANK_CYCLE_DEG }}°.
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

.cc-timer {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  font-size: 0.75rem;
  color: var(--color-fg-muted);
  cursor: default;
}

.cc-timer input {
  width: 4.25rem;
  padding: 0.2rem 0.35rem;
  font-size: 0.75rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: var(--color-bg);
  color: var(--color-fg);
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
  touch-action: none;
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
