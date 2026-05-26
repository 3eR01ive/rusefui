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
  initCompositeTimeline,
  useCompositeTimeline,
  compositeTimelineLoadEpoch,
} from "../../composables/useCompositeTimeline";
import { useLogViewportLink } from "../../composables/useLogViewportLink";
import { initOutputTimeline, useOutputTimeline } from "../../composables/useOutputTimeline";
import { listen } from "@tauri-apps/api/event";
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

const MIN_VIEW_MS = 5;
const ZOOM_STEP = 1.12;

/** Ширина окна просмотра (мс). */
const viewSpanMs = ref(maxWindowMs.value);
/** Левый край окна просмотра (µs). */
const viewAnchorT0Us = ref<number | null>(null);
/** Пользователь менял зум/пан — не подстраивать вид во время записи. */
const userAdjustedView = ref(false);

const { snapshot, setLoggingEnabled } = useCompositeLogger();
const {
  status: timelineStatus,
  hasFile: timelineHasFile,
  queryView: queryTimelineView,
  controlView: controlTimelineView,
  pickAndLoadFile,
  refreshStatus: refreshTimelineStatus,
} = useCompositeTimeline();
const { linked: viewportLinked, setLinked: setViewportLinked } = useLogViewportLink();
const { status: outputTimelineStatus, controlView: controlOutputView } =
  useOutputTimeline();
const { getProjectUi, setProjectUi } = useProject();

const reviewEvents = ref<CompositeEvent[]>([]);
const openingLog = ref(false);
const openLogError = ref<string | null>(null);
const dataCtx = useDataContext();
const connected = computed(() => dataCtx.connection.value.connected);
const loggingEnabled = computed(() => snapshot.value.loggingEnabled);
const reviewMode = computed(() => timelineHasFile.value && !loggingEnabled.value);
const alignTdc = ref(false);
const CAPTURE_DURATIONS_MS = [500, 1000, 3000] as const;
const captureDurationMs = ref(1000);
const durationDropdownOpen = ref(false);
const autoStopRemainingSec = ref<number | null>(null);
const loggerBusy = ref(false);
const loggerError = ref<string | null>(null);

let autoStopTimer: ReturnType<typeof setInterval> | null = null;
let autoStopDeadlineMs = 0;

function formatDuration(ms: number): string {
  return ms < 1000 ? `${ms} мс` : `${ms / 1000} сек`;
}

function selectDuration(ms: number) {
  captureDurationMs.value = ms;
  durationDropdownOpen.value = false;
  void persistUiSettings();
}

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

function chartEvents(): CompositeEvent[] {
  if (reviewMode.value) {
    return reviewEvents.value;
  }
  return snapshot.value.events as CompositeEvent[];
}

function dataT0(events: readonly CompositeEvent[]): number {
  return events[0]!.tUs;
}

function outputLogViewport() {
  const st = outputTimelineStatus.value;
  return { viewEndSec: st.viewEndSec, spanSec: st.spanSec };
}

async function refreshReviewEvents(): Promise<void> {
  if (!reviewMode.value) {
    reviewEvents.value = [];
    return;
  }
  const wrap = plotWrapRef.value;
  const w = Math.max(64, wrap?.clientWidth ?? 800);
  const view = await queryTimelineView(
    w,
    viewportLinked.value ? outputLogViewport() : undefined,
  );
  reviewEvents.value = view.events;
  scheduleDraw();
}

function currentTimeRangeFromOutput(): ChartTimeRange {
  const st = outputTimelineStatus.value;
  const spanUs = Math.max(1, Math.round(st.spanSec * 1_000_000));
  const tEnd = Math.round(st.viewEndSec * 1_000_000);
  const t0 = tEnd - spanUs;
  return { t0, tEnd: t0 + spanUs, spanUs };
}

function currentTimeRangeFromTimeline(): ChartTimeRange | null {
  const st = timelineStatus.value;
  if (st.eventCount < 2) return null;
  const spanUs = Math.max(1, Math.round(st.spanSec * 1_000_000));
  const tEnd = Math.round(st.viewEndSec * 1_000_000);
  const t0 = tEnd - spanUs;
  return { t0, tEnd: t0 + spanUs, spanUs };
}

function captureStartT0(events: readonly CompositeEvent[]): number {
  let t0 = dataT0(events);
  if (alignTdc.value) {
    t0 = snapT0ToTdc(events, t0);
  }
  return t0;
}

/** Весь склеенный захват в окне просмотра. */
async function fitFullCapture(events: readonly CompositeEvent[]) {
  if (reviewMode.value) {
    const st = timelineStatus.value;
    const span = Math.max(MIN_VIEW_MS / 1000, st.dataMaxSec - st.dataMinSec);
    await controlTimelineView({
      followLive: false,
      viewEndSec: st.dataMaxSec,
      spanSec: span,
    });
    await refreshReviewEvents();
    return;
  }
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
  if (reviewMode.value && viewportLinked.value) {
    return currentTimeRangeFromOutput();
  }
  if (reviewMode.value) {
    return currentTimeRangeFromTimeline();
  }
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
  const events = chartEvents();
  if (events.length >= 2) {
    void fitFullCapture(events);
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

  const events = chartEvents();
  const timeRange = currentTimeRange(events);
  const sharedAxis = reviewMode.value && viewportLinked.value;
  const view = buildChartView(
    events,
    viewSpanMs.value,
    cssW,
    cssH,
    LABEL_W,
    CHANNELS.length,
    timeRange,
    { allowEmptyWindow: sharedAxis },
  );

  if (!view) {
    ctx.fillStyle = cssColor(canvas, "--color-gray", "#888");
    ctx.font = "12px system-ui, sans-serif";
    ctx.textAlign = "left";
    const msg =
      sharedAxis
        ? "Нет trigger-событий в этом окне Log (общая шкала elapsed_sec)"
        : reviewMode.value && timelineStatus.value.eventCount >= 2
        ? "Двойной щелчок — показать весь trigger-лог"
        : reviewMode.value
          ? "Не удалось прочитать trigger CSV (нужен trigger_*.csv)"
          : events.length < 2
            ? connected.value
              ? "Старт — запись; Стоп — просмотр. Или «Лог триггера…»"
              : "Подключите ECU"
            : "Мало точек в окне — двойной щелчок";
    ctx.fillText(msg, LABEL_W + 8, cssH / 2);
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

async function panByClientDelta(deltaX: number) {
  const events = chartEvents();
  const range = currentTimeRange(events);
  if (!range) return;
  if (!viewportLinked.value && events.length < 2) return;

  if (reviewMode.value) {
    const panSec = (-deltaX / plotWidthPx()) * (range.spanUs / 1_000_000);
    if (viewportLinked.value) {
      await controlOutputView({ panSec });
    } else {
      await controlTimelineView({ panSec });
    }
    await refreshReviewEvents();
    return;
  }

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
  const events = chartEvents();
  if (e.button !== 0) return;
  if (!viewportLinked.value && events.length < 2) return;
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
async function zoomAtPointer(clientX: number, zoomIn: boolean) {
  const events = chartEvents();
  const range = currentTimeRange(events);
  if (!range) return;
  if (!viewportLinked.value && events.length < 2) return;

  if (reviewMode.value) {
    const factor = zoomIn ? ZOOM_STEP : 1 / ZOOM_STEP;
    if (viewportLinked.value) {
      await controlOutputView({ zoomFactor: factor });
    } else {
      await controlTimelineView({ zoomFactor: factor });
    }
    await refreshReviewEvents();
    return;
  }

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
  const events = chartEvents();
  if (!viewportLinked.value && events.length < 2) return;
  if (e.deltaY === 0) return;
  e.preventDefault();
  zoomAtPointer(e.clientX, e.deltaY < 0);
}

function onPlotDblClick() {
  if (viewportLinked.value && reviewMode.value) return;
  resetViewWindow();
}

async function onOpenCompositeLog() {
  openingLog.value = true;
  openLogError.value = null;
  try {
    const st = await pickAndLoadFile();
    if (!st) return;
    await controlTimelineView({
      followLive: false,
      viewEndSec: st.dataMaxSec,
      spanSec: Math.max(MIN_VIEW_MS / 1000, st.dataMaxSec - st.dataMinSec),
    });
    await refreshReviewEvents();
    if (reviewEvents.value.length < 2) {
      openLogError.value =
        "Файл загружен, но в окне просмотра мало точек. Двойной щелчок по графику — показать весь файл. Нужен trigger_*.csv из composite_logs, не output_*.csv.";
    }
  } catch (e) {
    openLogError.value = e instanceof Error ? e.message : String(e);
  } finally {
    openingLog.value = false;
  }
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
  const ms = captureDurationMs.value;
  if (ms <= 0) return;

  autoStopDeadlineMs = Date.now() + ms;
  autoStopRemainingSec.value = Math.ceil(ms / 1000);

  autoStopTimer = setInterval(() => {
    const left = autoStopDeadlineMs - Date.now();
    if (left <= 0) {
      clearAutoStopTimer();
      if (loggingEnabled.value && !loggerBusy.value) {
        void applyLoggingEnabled(false);
      }
      return;
    }
    autoStopRemainingSec.value = Math.ceil(left / 1000);
  }, 100);
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
      await refreshTimelineStatus();
      if (reviewMode.value) {
        const st = timelineStatus.value;
        await controlTimelineView({
          followLive: false,
          viewEndSec: st.dataMaxSec,
          spanSec: Math.max(MIN_VIEW_MS / 1000, st.dataMaxSec - st.dataMinSec),
        });
        await refreshReviewEvents();
      } else {
        const events = snapshot.value.events as CompositeEvent[];
        if (events.length >= 2) {
          await fitFullCapture(events);
        }
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
    alignTdc.value = Boolean(ui.alignTdc);
    if (ui.captureDurationMs != null && CAPTURE_DURATIONS_MS.includes(ui.captureDurationMs as (typeof CAPTURE_DURATIONS_MS)[number])) {
      captureDurationMs.value = ui.captureDurationMs;
    }
  } catch {
    alignTdc.value = false;
  }
}

function persistUiSettings() {
  void setProjectUi(PERSIST_KEY_COMPOSITE_CHART, {
    alignTdc: alignTdc.value,
    captureDurationMs: captureDurationMs.value,
  });
}

watch(alignTdc, () => {
  persistUiSettings();
  scheduleDraw();
});

watch(
  [reviewMode, () => timelineStatus.value.viewEndSec, () => timelineStatus.value.spanSec],
  () => {
    void refreshReviewEvents();
  },
);

watch(compositeTimelineLoadEpoch, () => {
  void refreshReviewEvents();
});

async function onViewportLinkChange(checked: boolean) {
  await setViewportLinked(checked);
  if (checked && reviewMode.value) {
    await refreshReviewEvents();
  }
}

watch(
  () =>
    viewportLinked.value
      ? `${outputTimelineStatus.value.viewEndSec}:${outputTimelineStatus.value.spanSec}:${outputTimelineStatus.value.followLive}:${outputTimelineStatus.value.liveSec}`
      : "",
  () => {
    if (viewportLinked.value && reviewMode.value) {
      void refreshReviewEvents();
    }
  },
);

watch(loggingEnabled, (on, wasOn) => {
  if (wasOn && !on) {
    void refreshTimelineStatus().then(() => refreshReviewEvents());
  }
});

onMounted(async () => {
  await initCompositeLogger();
  await initCompositeTimeline();
  await initOutputTimeline();
  const refreshIfLinked = () => {
    if (viewportLinked.value && reviewMode.value) {
      void refreshReviewEvents();
    }
  };
  await listen("output-timeline-status", refreshIfLinked);
  await listen("composite-timeline-status", refreshIfLinked);
  await loadUiFromProject();
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
  document.addEventListener("click", onDocClick);
});

function onDocClick() {
  durationDropdownOpen.value = false;
}

onUnmounted(() => {
  clearAutoStopTimer();
  document.removeEventListener("click", onDocClick);
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
  if (viewportLinked.value && reviewMode.value) {
    const o = outputTimelineStatus.value;
    const t0 = (o.viewEndSec - o.spanSec) * 1000;
    const t1 = o.viewEndSec * 1000;
    parts.push(`↔ Log ${t0.toFixed(0)}–${t1.toFixed(0)} ms`);
  }
  if (reviewMode.value) {
    parts.push(`файл ${timelineStatus.value.eventCount} evt`);
  } else if (s.loggingEnabled) {
    parts.push("log on");
  }
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
    <div class="cc-header">
      <span class="cc-status" :class="{ warn: !connected }">{{ statusLine }}</span>
    </div>
    <div class="cc-toolbar">
      <!-- Старт / Стоп split-кнопка -->
      <div class="btn-split">
        <button
          type="button"
          class="btn split-main"
          :class="loggingEnabled ? 'stop' : 'primary'"
          :disabled="!connected || loggerBusy"
          @click="applyLoggingEnabled(!loggingEnabled)"
        >
          <template v-if="loggingEnabled">
            Стоп
            <span v-if="autoStopRemainingSec != null" class="split-remain">{{ autoStopRemainingSec }}с</span>
          </template>
          <template v-else>
            Старт · {{ formatDuration(captureDurationMs) }}
          </template>
        </button>
        <button
          v-if="!loggingEnabled"
          type="button"
          class="btn primary split-arrow"
          :disabled="!connected || loggerBusy"
          aria-label="Выбрать длительность записи"
          @click.stop="durationDropdownOpen = !durationDropdownOpen"
        >
          ▾
        </button>
        <div v-if="durationDropdownOpen" class="split-dropdown" @click.stop>
          <button
            v-for="d in CAPTURE_DURATIONS_MS"
            :key="d"
            type="button"
            class="split-opt"
            :class="{ active: d === captureDurationMs }"
            @click="selectDuration(d)"
          >
            {{ formatDuration(d) }}
          </button>
        </div>
      </div>
      <label class="cc-autostart">
        <input
          type="checkbox"
          :checked="viewportLinked"
          :disabled="loggerBusy"
          @change="onViewportLinkChange(($event.target as HTMLInputElement).checked)"
        />
        Одна шкала с Log
      </label>
      <button
        type="button"
        class="btn secondary"
        :disabled="openingLog || loggerBusy"
        @click="onOpenCompositeLog"
      >
        Лог триггера…
      </button>
    </div>
    <p v-if="loggerError" class="cc-error">{{ loggerError }}</p>
    <p v-if="openLogError" class="cc-error">{{ openLogError }}</p>
    <div
      ref="plotWrapRef"
      class="cc-plot-wrap"
      :class="{ 'cc-plot-wrap--linked': viewportLinked && reviewMode }"
      :title="
        viewportLinked && reviewMode
          ? 'Связано с Log: ◀▶, колёсико и перетаскивание на графике Log'
          : 'Запись → стоп → весь захват. Колёсико — масштаб, перетаскивание — перемотка, двойной щелчок — весь захват'
      "
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
    <p v-else-if="connected && !loggingEnabled && !reviewMode" class="cc-hint">
      Запись: «Старт» → «Стоп» в этой панели (Trigger logger). Файл trigger_*.csv появится здесь
      автоматически. Кнопка «Лог output» в шапке — только RPM/CLT, не триггер.
    </p>
    <p v-else-if="reviewMode && viewportLinked" class="cc-hint">
      Общая шкала с Log:
      {{ ((outputTimelineStatus.viewEndSec - outputTimelineStatus.spanSec) * 1000).toFixed(0) }}–{{
        (outputTimelineStatus.viewEndSec * 1000).toFixed(0)
      }}
      ms (elapsed_sec). Нет trigger-данных в окне — пустой график. Управление — на Log (◀▶, зум, drag).
    </p>
    <p v-else-if="reviewMode && reviewEvents.length >= 2" class="cc-hint">
      Trigger-лог: {{ timelineStatus.eventCount }} событий. «Одна шкала с Log» — то же окно времени, что на
      output (мс от начала сессии).
    </p>
    <p v-else-if="reviewMode" class="cc-hint">
      Файл trigger загружен ({{ timelineStatus.eventCount }} событий), но в окне мало точек.
      Двойной щелчок по графику — показать всё.
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

/* ---- split-кнопка Старт ---- */
.btn-split {
  position: relative;
  display: inline-flex;
}

.btn-split .split-main {
  border-radius: var(--radius-sm) 0 0 var(--radius-sm);
  border-right: 1px solid rgba(255 255 255 / 0.25) !important;
}

/* когда стрелка скрыта (в режиме Стоп) — правый радиус возвращаем */
.btn-split .split-main:last-child {
  border-radius: var(--radius-sm);
  border-right: none !important;
}

.cc-toolbar .btn.stop {
  background: var(--color-error, #ef4444);
  color: #fff;
  border-color: transparent;
}

.split-remain {
  margin-left: 0.35rem;
  opacity: 0.75;
  font-size: 0.7em;
  font-variant-numeric: tabular-nums;
}

.btn-split .split-arrow {
  border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
  padding: 0.25rem 0.4rem;
  font-size: 0.65rem;
  line-height: 1;
}

.split-dropdown {
  position: absolute;
  top: calc(100% + 3px);
  left: 0;
  z-index: 200;
  background: var(--color-bg-elevated, #1e1e2e);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  box-shadow: 0 4px 12px rgba(0 0 0 / 0.35);
  min-width: 7rem;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.split-opt {
  padding: 0.35rem 0.75rem;
  font-size: 0.75rem;
  text-align: left;
  background: none;
  border: none;
  color: var(--color-fg, #e2e8f0);
  cursor: pointer;
  transition: background 0.1s;
}

.split-opt:hover {
  background: var(--color-bg-muted, #2a2a3e);
}

.split-opt.active {
  color: var(--color-accent, #3b82f6);
  font-weight: 600;
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

.cc-plot-wrap--linked {
  border-color: var(--color-accent, #2563eb);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-accent, #2563eb) 35%, transparent);
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
