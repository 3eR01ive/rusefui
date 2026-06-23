<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useInstanceBind } from "../../composables/useInstanceBind";
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
import { useTabActivity } from "../../composables/useTabActivity";
import { initOutputTimeline, useOutputTimeline } from "../../composables/useOutputTimeline";
import { useOutputChannels } from "../../composables/useOutputChannels";
import { initConfig, useConfig } from "../../composables/useConfig";
import { listen } from "@tauri-apps/api/event";
import {
  PERSIST_KEY_COMPOSITE_CHART,
  registerProjectUiFlushHook,
  useProject,
  type CompositeChartUiSettings,
  type CrankEdgeMode,
} from "../../composables/useProject";
import { invoke } from "@tauri-apps/api/core";
import CompositeTriggerWheels, {
  type TriggerWheelsView,
} from "./CompositeTriggerWheels.vue";
import CompositeTriggerAnalysis, {
  type TriggerAnalysis,
} from "./CompositeTriggerAnalysis.vue";
import {
  bufferSpanMs,
  buildChartView,
  crankAngleDeg,
  crankDegFromFirmwareTdc,
  computeNextGlobalTriggerAngleOffset,
  CRANK_CYCLE_DEG,
  GLOBAL_TRIGGER_ANGLE_OFFSET_FIELD,
  laneY,
  signedDegFromFirmwareTdc,
  maxViewSpanMs,
  snapT0ToTdc,
  timeAtX,
  valueAtTime,
  xAtTime,
  type ChannelKey,
  type ChartTimeRange,
  type ChartView,
} from "./compositeChartGeometry";
import {
  CompositeChartRenderer,
  type CompositeChannelDef,
  type CompositeRenderRequest,
} from "./compositeChartRenderer";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const instanceRef = computed(() => props.instance);
const { source: bindSource } = useInstanceBind(instanceRef);

if (bindSource.value && bindSource.value !== "compositeLogger") {
  console.warn(
    `[composite-chart] ожидался bind.source=compositeLogger, получен ${bindSource.value}`,
  );
}

const maxWindowMs = computed(() => Math.max(5, Number(props.props.windowMs ?? 300)));
const chartHeight = computed(() => Math.max(120, Number(props.props.height ?? 220)));

const MIN_VIEW_MS = 5;
const ZOOM_STEP = 1.12;

const viewSpanMs = ref(maxWindowMs.value);
const viewAnchorT0Us = ref<number | null>(null);
const userAdjustedView = ref(false);

const { snapshot, setLoggingEnabled } = useCompositeLogger();
const {
  status: timelineStatus,
  hasFile: timelineHasFile,
  queryView: queryTimelineView,
  controlView: controlTimelineView,
  pickAndLoadFile,
  refreshStatus: refreshTimelineStatus,
  sessionEvents: fetchCompositeSessionEvents,
} = useCompositeTimeline();
const { linked: viewportLinked, setLinked: setViewportLinked } = useLogViewportLink();
const { status: outputTimelineStatus, controlView: controlOutputView } =
  useOutputTimeline();
const { snapshot: outputChannelsSnapshot } = useOutputChannels();
const { isActive: tabActive } = useTabActivity();
const { getProjectUi, setProjectUi } = useProject();
const {
  snapshot: configSnapshot,
  configCanEdit,
  getField: getConfigField,
  setField: setConfigField,
  burn: burnConfig,
} = useConfig();

const realTdcTUs = ref<number | null>(null);
const tdcPlaceMode = ref(false);
const burnOffsetAfterWrite = ref(false);
const offsetWriteBusy = ref(false);
const offsetWriteError = ref<string | null>(null);

const PLACE_CLICK_MAX_PX = 6;
let placePointerPending = false;
let placeDownClientX = 0;
let placeDownClientY = 0;
let placeDownInsidePlot = false;

const reviewEvents = ref<CompositeEvent[]>([]);
const dataCtx = useDataContext();
const connected = computed(() => dataCtx.connection.value.connected);
const loggingEnabled = computed(() => snapshot.value.loggingEnabled);
const reviewMode = computed(() => timelineHasFile.value && !loggingEnabled.value);
const alignTdc = ref(false);
const CAPTURE_DURATIONS_MS = [500, 1000, 3000] as const;
const captureDurationMs = ref(1000);
const durationDropdownOpen = ref(false);
const crankEdgeMode = ref<CrankEdgeMode>("both");
// По умолчанию скрыто: trigger wheels делают компонент высоким (нельзя ужать
// канвас по высоте). Включается галкой в тулбаре.
const showTriggerWheels = ref(false);
const showTriggerAnalysis = ref(false);
const triggerAnalysis = ref<TriggerAnalysis | null>(null);
const analysisBusy = ref(false);
const triggerWheelsView = ref<TriggerWheelsView | null>(null);
let wheelComputeTimer: ReturnType<typeof setTimeout> | null = null;
const autoStopRemainingSec = ref<number | null>(null);
const loggerBusy = ref(false);
const loggerError = ref<string | null>(null);
const openingLog = ref(false);
const openLogError = ref<string | null>(null);

let autoStopTimer: ReturnType<typeof setInterval> | null = null;
let autoStopDeadlineMs = 0;
let autoStopArmed = false;

function formatDuration(ms: number): string {
  return ms < 1000 ? `${ms} мс` : `${ms / 1000} сек`;
}

function selectDuration(ms: number) {
  captureDurationMs.value = ms;
  durationDropdownOpen.value = false;
  void persistUiSettings();
}

const plotWrapRef = ref<HTMLDivElement | null>(null);
const glCanvasRef = ref<HTMLCanvasElement | null>(null);
const hoverX = ref<number | null>(null);
const hoverInside = ref(false);
const panPointerIdRef = ref<number | null>(null);
let panStartClientX = 0;
let panStartClientY = 0;
let panPrevClientX = 0;
let panStartT0Us = 0;
let panHasMoved = false;
const CLICK_PAN_THRESHOLD_PX = 5;
let ro: ResizeObserver | null = null;

let liveDrawRaf = 0;
let pendingPanPx = 0;
let panWheelRafId = 0;

// ---- WebGL renderer ---------------------------------------------------------
const renderer = new CompositeChartRenderer();
const currentView = ref<ChartView | null>(null);

// Pre-computed channel RGBA colors (matching CHANNELS hex below)
type Rgba = [number, number, number, number];
const CHANNEL_RGBAS: Rgba[] = [
  [0.231, 0.510, 0.965, 1], // #3b82f6 Pri
  [0.545, 0.361, 0.965, 1], // #8b5cf6 Sec
  [0.961, 0.620, 0.043, 1], // #f59e0b TDC
  [0.063, 0.722, 0.506, 1], // #10b981 Sync
  [0.937, 0.267, 0.267, 1], // #ef4444 Coil
  [0.024, 0.714, 0.831, 1], // #06b6d4 Inj
];

const CHANNELS: { key: ChannelKey; label: string; color: string }[] = [
  { key: "pri",  label: "Pri",  color: "#3b82f6" },
  { key: "sec",  label: "Sec",  color: "#8b5cf6" },
  { key: "trg",  label: "TDC",  color: "#f59e0b" },
  { key: "sync", label: "Sync", color: "#10b981" },
  { key: "coil", label: "Coil", color: "#ef4444" },
  { key: "inj",  label: "Inj",  color: "#06b6d4" },
];

const CHANNEL_DEFS: CompositeChannelDef[] = CHANNELS.map((ch, i) => ({
  key: ch.key,
  color: CHANNEL_RGBAS[i]!,
}));

const LABEL_W = 44;
const TIP_OFFSET_X = 40;

// CSS var → RGBA for WebGL
let colorProbe: CanvasRenderingContext2D | null = null;
function cssColorToRgba(css: string, alpha = 1): Rgba {
  if (!colorProbe) colorProbe = document.createElement("canvas").getContext("2d");
  const probe = colorProbe;
  if (!probe) return [0.5, 0.5, 0.5, alpha];
  probe.fillStyle = css;
  const p = probe.fillStyle as string;
  if (p.startsWith("#")) {
    const hex = p.slice(1);
    const full = hex.length === 3 ? hex.split("").map((c) => c + c).join("") : hex;
    const n = Number.parseInt(full, 16);
    return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255, alpha];
  }
  const m = p.match(/rgba?\(([^)]+)\)/);
  if (!m) return [0.5, 0.5, 0.5, alpha];
  const parts = m[1]!.split(",").map((s) => Number.parseFloat(s.trim()));
  return [(parts[0] ?? 0) / 255, (parts[1] ?? 0) / 255, (parts[2] ?? 0) / 255, parts[3] ?? alpha];
}

function readGlColors(canvas: HTMLCanvasElement) {
  const s = getComputedStyle(canvas);
  const v = (n: string, fb: string) => s.getPropertyValue(n).trim() || fb;
  return {
    bg:        cssColorToRgba(v("--color-bg",      "#0f1115")),
    grid:      cssColorToRgba(v("--color-border",  "#333"),    1),
    tdc:       cssColorToRgba(v("--color-warning", "#d97706"), 0.85),
    accent:    cssColorToRgba(v("--color-accent",  "#3b82f6"), 0.95),
    crosshair: cssColorToRgba(v("--color-fg",      "#e5e7eb"), 0.55),
  };
}

// ---- Overlay computed -------------------------------------------------------

// Channel labels (left side, vertically centered in each lane)
const overlayLabels = computed(() => {
  const view = currentView.value;
  if (!view) return [];
  return CHANNELS.map((ch, i) => {
    const { yHigh, yLow } = laneY(i, view, true);
    return { label: ch.label, color: ch.color, top: (yHigh + yLow) / 2 };
  });
});

// TDC cycle labels ("#N" + "0°") above each TDC marker vertical line
const overlayTdcLabels = computed(() => {
  const view = currentView.value;
  if (!view) return [];
  return view.tdcMarkers
    .map((m) => ({ cycle: m.cycle, left: xAtTime(m.tUs, view) }))
    .filter((l) => l.left >= view.plotLeft - 2 && l.left <= view.plotLeft + view.plotW + 2);
});

// Real TDC marker label
const overlayRealTdcLeft = computed(() => {
  const view = currentView.value;
  if (!view || realTdcTUs.value == null) return null;
  return xAtTime(realTdcTUs.value, view);
});

// Crosshair tooltip + dots
const crosshairOverlay = computed(() => {
  const view = currentView.value;
  if (!view || !hoverInside.value || hoverX.value == null) return null;
  if (!tdcPlaceMode.value && panPointerIdRef.value != null) return null;
  const x = hoverX.value;
  const tUs = timeAtX(x, view);
  const angle = crankAngleDeg(tUs, view, snapshot.value.rpm);
  const channels = CHANNELS.map((ch, i) => {
    const val = valueAtTime(tUs, view.visible, ch.key);
    const dotY = laneY(i, view, val).y;
    return { label: ch.label, color: ch.color, val, dotY };
  });

  const TIP_W = 90;
  const LINE_H = 13;
  const BOX_PAD = 5;
  const tipH = (channels.length + 1) * LINE_H + BOX_PAD * 2;
  const plotRight = view.plotLeft + view.plotW;
  let tipLeft = x + TIP_OFFSET_X;
  if (tipLeft + TIP_W > plotRight - 2) tipLeft = x - TIP_OFFSET_X - TIP_W;
  tipLeft = Math.max(view.plotLeft + 2, tipLeft);
  const avgDotY = channels.reduce((s, c) => s + c.dotY, 0) / channels.length;
  let tipTop = avgDotY - tipH / 2;
  tipTop = Math.max(4, Math.min(tipTop, view.cssH - tipH - 4));

  return { x, angle, channels, tipLeft, tipTop, tipW: TIP_W };
});

// Empty state message
const emptyMsg = computed(() => {
  const sharedAxis = reviewMode.value && viewportLinked.value;
  if (sharedAxis) return "Нет trigger-событий в этом окне Log (общая шкала elapsed_sec)";
  if (reviewMode.value && timelineStatus.value.eventCount >= 2)
    return "Двойной щелчок — показать весь trigger-лог";
  if (reviewMode.value) return "Не удалось прочитать trigger CSV (нужен trigger_*.csv)";
  const events = chartEvents();
  if (events.length < 2) {
    return connected.value ? "Старт — запись; Стоп — просмотр" : "Подключите ECU";
  }
  return "Мало точек в окне — двойной щелчок";
});

// ---- Draw -------------------------------------------------------------------

function drawFrame() {
  const canvas = glCanvasRef.value;
  if (!canvas) return;
  const cssW = canvas.clientWidth;
  const cssH = chartHeight.value;
  if (cssW <= 0 || cssH <= 0) return;

  const events = chartEvents();
  const timeRange = currentTimeRange(events);
  const sharedAxis = reviewMode.value && viewportLinked.value;
  const view = buildChartView(
    events, viewSpanMs.value, cssW, cssH, LABEL_W, CHANNELS.length, timeRange,
    { allowEmptyWindow: sharedAxis },
  );

  currentView.value = view;

  const showCrosshair =
    hoverInside.value &&
    hoverX.value != null &&
    (tdcPlaceMode.value || panPointerIdRef.value == null);

  const colors = readGlColors(canvas);

  const req: CompositeRenderRequest = {
    width: cssW,
    height: cssH,
    view,
    channels: CHANNEL_DEFS,
    edgeMode: crankEdgeMode.value,
    bgRgba: colors.bg,
    gridRgba: colors.grid,
    tdcRgba: colors.tdc,
    accentRgba: colors.accent,
    crosshairRgba: colors.crosshair,
    realTdcTUs: realTdcTUs.value,
    crosshairX: showCrosshair ? hoverX.value : null,
  };
  renderer.paint(req);
}

function scheduleDraw() {
  if (!tabActive.value) return;
  requestAnimationFrame(drawFrame);
}

function startLiveDraw() {
  const tick = () => {
    if (!loggingEnabled.value || !tabActive.value) { liveDrawRaf = 0; return; }
    scheduleDraw();
    liveDrawRaf = requestAnimationFrame(tick);
  };
  if (liveDrawRaf === 0) liveDrawRaf = requestAnimationFrame(tick);
}

function stopLiveDraw() {
  if (liveDrawRaf !== 0) { cancelAnimationFrame(liveDrawRaf); liveDrawRaf = 0; }
}

// ---- View management (unchanged) -------------------------------------------

function maxSpanMsFor(events: readonly CompositeEvent[]): number {
  return maxViewSpanMs(events, maxWindowMs.value, MIN_VIEW_MS);
}

function chartEvents(): CompositeEvent[] {
  return reviewMode.value ? reviewEvents.value : (snapshot.value.events as CompositeEvent[]);
}

function dataT0(events: readonly CompositeEvent[]): number {
  return events[0]!.tUs;
}

function outputLogViewport() {
  const st = outputTimelineStatus.value;
  return { viewEndSec: st.viewEndSec, spanSec: st.spanSec };
}

async function refreshReviewEvents(): Promise<void> {
  if (!reviewMode.value) { reviewEvents.value = []; return; }
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
  const liveSec =
    outputChannelsSnapshot.value.timelineLiveSec ?? st.liveSec ?? st.viewEndSec;
  const viewEndSec = st.followLive ? liveSec : st.viewEndSec;
  const tEnd = Math.round(viewEndSec * 1_000_000);
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
  if (alignTdc.value) t0 = snapT0ToTdc(events, t0);
  return t0;
}

async function fitFullCapture(events: readonly CompositeEvent[]) {
  if (reviewMode.value) {
    const st = timelineStatus.value;
    const span = Math.max(MIN_VIEW_MS / 1000, st.dataMaxSec - st.dataMinSec);
    await controlTimelineView({ followLive: false, viewEndSec: st.dataMaxSec, spanSec: span });
    await refreshReviewEvents();
    return;
  }
  if (events.length < 2) return;
  viewAnchorT0Us.value = captureStartT0(events);
  viewSpanMs.value = Math.max(MIN_VIEW_MS, bufferSpanMs(events));
  userAdjustedView.value = false;
  clampViewToBuffer(events);
}

function fitGrowingCapture(events: readonly CompositeEvent[]) {
  if (userAdjustedView.value || viewportLinked.value || events.length < 2) return;
  fitFullCapture(events);
}

function currentTimeRange(events: readonly CompositeEvent[]): ChartTimeRange | null {
  if (viewportLinked.value && reviewMode.value) return currentTimeRangeFromOutput();
  if (reviewMode.value) return currentTimeRangeFromTimeline();
  if (events.length < 2) return null;
  const dataStart = dataT0(events);
  const dataEnd = events[events.length - 1]!.tUs;
  const spanUs = Math.round(viewSpanMs.value * 1000);
  let t0 = viewAnchorT0Us.value ?? captureStartT0(events);
  if (alignTdc.value) t0 = snapT0ToTdc(events, t0);
  const maxT0 = Math.max(dataStart, dataEnd - spanUs);
  if (t0 > maxT0) t0 = maxT0;
  if (t0 < dataStart) t0 = dataStart;
  return { t0, tEnd: t0 + spanUs, spanUs };
}

function resetViewWindow() {
  const events = chartEvents();
  if (events.length >= 2) { void fitFullCapture(events); }
  else { viewSpanMs.value = maxWindowMs.value; viewAnchorT0Us.value = null; userAdjustedView.value = false; }
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

watch(maxWindowMs, () => { clampViewToBuffer(snapshot.value.events as CompositeEvent[]); });
watch(
  () => {
    const ev = snapshot.value.events;
    if (ev.length === 0) return "0";
    return `${ev.length}:${ev[0]!.tUs}:${ev[ev.length - 1]!.tUs}`;
  },
  () => {
    const events = snapshot.value.events as CompositeEvent[];
    clampViewToBuffer(events);
    if (loggingEnabled.value) fitGrowingCapture(events);
  },
);

// ---- Pointer events (unchanged logic) --------------------------------------

function getCurrentChartView(): ChartView | null {
  const canvas = glCanvasRef.value;
  if (!canvas) return null;
  const cssW = canvas.clientWidth;
  const cssH = chartHeight.value;
  if (cssW <= 0 || cssH <= 0) return null;
  const events = chartEvents();
  const timeRange = currentTimeRange(events);
  const sharedAxis = reviewMode.value && viewportLinked.value;
  return buildChartView(events, viewSpanMs.value, cssW, cssH, LABEL_W, CHANNELS.length, timeRange, { allowEmptyWindow: sharedAxis });
}

function plotWidthPx(): number {
  const wrap = plotWrapRef.value;
  if (!wrap) return 1;
  return Math.max(1, wrap.getBoundingClientRect().width - LABEL_W - 8);
}

async function panByClientDelta(currentClientX: number) {
  const events = chartEvents();
  const range = currentTimeRange(events);
  if (!range) return;
  if (!viewportLinked.value && events.length < 2) return;

  if (reviewMode.value) {
    const stepX = currentClientX - panPrevClientX;
    panPrevClientX = currentClientX;
    const panSec = (-stepX / plotWidthPx()) * (range.spanUs / 1_000_000);
    if (viewportLinked.value) { await controlOutputView({ panSec }); }
    else { await controlTimelineView({ panSec }); }
    await refreshReviewEvents();
    return;
  }

  const deltaX = currentClientX - panStartClientX;
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

function canvasPlotX(clientX: number): { x: number; inside: boolean } | null {
  const canvas = glCanvasRef.value;
  if (!canvas) return null;
  const rect = canvas.getBoundingClientRect();
  const x = clientX - rect.left;
  return { x, inside: x >= LABEL_W && x <= rect.width - 4 };
}

function beginPan(e: PointerEvent) {
  const events = chartEvents();
  if (!viewportLinked.value && events.length < 2) return;
  const range = currentTimeRange(events);
  if (!range) return;
  const canvas = glCanvasRef.value;
  if (!canvas) return;
  panPointerIdRef.value = e.pointerId;
  panStartClientX = e.clientX;
  panStartClientY = e.clientY;
  panPrevClientX = e.clientX;
  panStartT0Us = range.t0;
  panHasMoved = false;
  viewAnchorT0Us.value = range.t0;
  canvas.setPointerCapture(e.pointerId);
}

function onPointerDown(e: PointerEvent) {
  if (e.button !== 0) return;
  const canvas = glCanvasRef.value;
  if (!canvas) return;
  if (tdcPlaceMode.value) {
    const plot = canvasPlotX(e.clientX);
    placePointerPending = true;
    placeDownClientX = e.clientX;
    placeDownClientY = e.clientY;
    placeDownInsidePlot = plot?.inside ?? false;
    return;
  }
  beginPan(e);
}

function onPointerMove(e: PointerEvent) {
  const canvas = glCanvasRef.value;
  if (!canvas) return;
  if (placePointerPending && tdcPlaceMode.value) {
    const dx = e.clientX - placeDownClientX;
    const dy = e.clientY - placeDownClientY;
    if (Math.hypot(dx, dy) > PLACE_CLICK_MAX_PX) {
      placePointerPending = false;
      beginPan(e);
    }
  }
  if (panPointerIdRef.value === e.pointerId) {
    if (!panHasMoved) {
      const dx = e.clientX - panStartClientX;
      const dy = e.clientY - panStartClientY;
      if (dx * dx + dy * dy > CLICK_PAN_THRESHOLD_PX * CLICK_PAN_THRESHOLD_PX) {
        panHasMoved = true;
        panPrevClientX = e.clientX; // reset incremental pan baseline
        userAdjustedView.value = true;
      }
    }
    if (panHasMoved) void panByClientDelta(e.clientX);
    return;
  }
  const rect = canvas.getBoundingClientRect();
  const x = e.clientX - rect.left;
  hoverX.value = x;
  hoverInside.value = x >= LABEL_W && x <= rect.width - 4;
  scheduleDraw();
}

function endPan(e: PointerEvent) {
  if (panPointerIdRef.value !== e.pointerId) return;
  panPointerIdRef.value = null;
  glCanvasRef.value?.releasePointerCapture(e.pointerId);
}

function placeRealTdcAtClientX(clientX: number): boolean {
  const plot = canvasPlotX(clientX);
  if (!plot?.inside) return false;
  const view = getCurrentChartView();
  if (!view) return false;
  const tUs = timeAtX(plot.x, view);
  const deg = crankDegFromFirmwareTdc(tUs, view, snapshot.value.rpm);
  if (deg == null) {
    offsetWriteError.value = "Нет TDC ECU в буфере — нужна запись со стимом или синхронизацией.";
    return false;
  }
  realTdcTUs.value = tUs;
  offsetWriteError.value = null;
  tdcPlaceMode.value = false;
  scheduleDraw();
  return true;
}

function onPointerUp(e: PointerEvent) {
  if (placePointerPending && tdcPlaceMode.value && placeDownInsidePlot) {
    placePointerPending = false;
    placeRealTdcAtClientX(placeDownClientX);
    return;
  }
  placePointerPending = false;

  // Click (not drag, not TDC mode) → zoom in/out like Log chart
  if (!panHasMoved && !tdcPlaceMode.value && panPointerIdRef.value === e.pointerId) {
    const plot = canvasPlotX(e.clientX);
    if (plot?.inside) {
      const factor = e.ctrlKey ? 1 / ZOOM_STEP : ZOOM_STEP;
      void zoomAtPointerFactor(e.clientX, factor);
    }
  }
  endPan(e);
}

function onPointerCancel(e: PointerEvent) { endPan(e); }
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

async function zoomAtPointerFactor(clientX: number, factor: number) {
  const events = chartEvents();
  const range = currentTimeRange(events);
  if (!range) return;
  if (!viewportLinked.value && events.length < 2) return;

  const frac = plotFracFromClientX(clientX);
  const spanUs = range.spanUs;
  const minSpanUs = Math.round(MIN_VIEW_MS * 1000);

  if (reviewMode.value) {
    const maxSpanSec = 3600;
    const minSpanSec = MIN_VIEW_MS / 1000;
    const oldSpanSec = spanUs / 1_000_000;
    const newSpanSec = Math.min(maxSpanSec, Math.max(minSpanSec, oldSpanSec / factor));
    const tAnchorSec = range.t0 / 1_000_000 + frac * oldSpanSec;
    const newViewEndSec = tAnchorSec + (1 - frac) * newSpanSec;
    if (viewportLinked.value) { await controlOutputView({ viewEndSec: newViewEndSec, spanSec: newSpanSec, followLive: false }); }
    else { await controlTimelineView({ viewEndSec: newViewEndSec, spanSec: newSpanSec, followLive: false }); }
    await refreshReviewEvents();
    return;
  }

  const maxSpanUs = Math.round(maxSpanMsFor(events) * 1000);
  const tAnchor = range.t0 + frac * spanUs;
  userAdjustedView.value = true;

  if (factor > 1) {
    if (spanUs <= minSpanUs) return;
    const newSpanUs = Math.max(minSpanUs, Math.round(spanUs / factor));
    viewSpanMs.value = newSpanUs / 1000;
    viewAnchorT0Us.value = tAnchor - frac * newSpanUs;
  } else {
    if (spanUs >= maxSpanUs - 1) { void fitFullCapture(events); return; }
    const newSpanUs = Math.min(maxSpanUs, Math.round(spanUs * factor));
    viewSpanMs.value = newSpanUs / 1000;
    viewAnchorT0Us.value = tAnchor - frac * newSpanUs;
  }
  clampViewToBuffer(events);
  scheduleDraw();
}


function onCanvasWheel(e: WheelEvent) {
  const events = chartEvents();
  if (!viewportLinked.value && events.length < 2) return;
  const dy = e.deltaY;
  const dx = e.deltaX;
  if (dy === 0 && dx === 0) return;
  e.preventDefault();

  if (e.ctrlKey) {
    void zoomAtPointerFactor(e.clientX, dy < 0 ? ZOOM_STEP : 1 / ZOOM_STEP);
    return;
  }

  const PIXELS_PER_LINE = 40;
  const rawDelta = e.deltaMode === 0 ? dy || dx : (dy || dx) * PIXELS_PER_LINE;
  scheduleWheelPan(rawDelta);
}

function scheduleWheelPan(rawPx: number) {
  pendingPanPx += rawPx;
  if (panWheelRafId !== 0) return;
  panWheelRafId = requestAnimationFrame(() => {
    panWheelRafId = 0;
    const px = pendingPanPx;
    pendingPanPx = 0;
    void panByWheelDelta(px);
  });
}

async function panByWheelDelta(rawPx: number) {
  const events = chartEvents();
  const range = currentTimeRange(events);
  if (!range) return;

  const frac = rawPx / Math.max(1, plotWidthPx());

  if (reviewMode.value) {
    const panSec = frac * (range.spanUs / 1_000_000);
    if (viewportLinked.value) { await controlOutputView({ panSec }); }
    else { await controlTimelineView({ panSec }); }
    await refreshReviewEvents();
    return;
  }

  const dataStart = dataT0(events);
  const dataEnd = events[events.length - 1]!.tUs;
  const dtUs = frac * range.spanUs;
  let t0 = range.t0 + dtUs;
  const maxT0 = Math.max(dataStart, dataEnd - range.spanUs);
  t0 = Math.min(maxT0, Math.max(dataStart, t0));
  if (alignTdc.value) { t0 = snapT0ToTdc(events, t0); if (t0 > maxT0) t0 = maxT0; }
  viewAnchorT0Us.value = t0;
  userAdjustedView.value = true;
  scheduleDraw();
}

function onPlotDblClick() { /* double-click intentionally disabled */ }

// ---- Log loading (unchanged) -----------------------------------------------

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
        "Файл загружен, но мало точек. Нужен trigger_*.csv из composite_logs, не output_*.csv.";
    }
  } catch (e) {
    openLogError.value = e instanceof Error ? e.message : String(e);
  } finally {
    openingLog.value = false;
  }
}

// ---- Auto-stop (unchanged) -------------------------------------------------

function clearAutoStopTimer() {
  autoStopArmed = false;
  if (autoStopTimer != null) { clearInterval(autoStopTimer); autoStopTimer = null; }
  autoStopRemainingSec.value = null;
  autoStopDeadlineMs = 0;
}

function startAutoStopTimer() {
  clearAutoStopTimer();
  const ms = captureDurationMs.value;
  if (ms <= 0) return;
  autoStopArmed = true;
  autoStopRemainingSec.value = null;
}

async function applyLoggingEnabled(on: boolean) {
  loggerBusy.value = true;
  loggerError.value = null;
  try {
    await setLoggingEnabled(on);
    if (on) {
      if (!viewportLinked.value) {
        userAdjustedView.value = false;
        viewAnchorT0Us.value = null;
        viewSpanMs.value = maxWindowMs.value;
      }
      startAutoStopTimer();
    } else {
      clearAutoStopTimer();
      const outputWasLive = outputTimelineStatus.value.followLive;
      await refreshTimelineStatus();
      if (reviewMode.value) {
        const st = timelineStatus.value;
        await controlTimelineView({
          followLive: false,
          viewEndSec: st.dataMaxSec,
          spanSec: Math.max(MIN_VIEW_MS / 1000, st.dataMaxSec - st.dataMinSec),
        });
        if (viewportLinked.value && outputWasLive) await controlOutputView({ followLive: true });
        await refreshReviewEvents();
      } else {
        const events = snapshot.value.events as CompositeEvent[];
        if (events.length >= 2) await fitFullCapture(events);
      }
    }
  } catch (e) {
    loggerError.value = e instanceof Error ? e.message : String(e);
    clearAutoStopTimer();
  } finally {
    loggerBusy.value = false;
  }
}

// ---- UI persistence (unchanged) --------------------------------------------

async function loadUiFromProject() {
  try {
    const ui = await getProjectUi<CompositeChartUiSettings>(PERSIST_KEY_COMPOSITE_CHART);
    alignTdc.value = Boolean(ui.alignTdc);
    if (ui.captureDurationMs != null && CAPTURE_DURATIONS_MS.includes(ui.captureDurationMs as (typeof CAPTURE_DURATIONS_MS)[number])) {
      captureDurationMs.value = ui.captureDurationMs;
    }
    if (ui.crankEdgeMode === "rise" || ui.crankEdgeMode === "fall") {
      crankEdgeMode.value = ui.crankEdgeMode;
    } else {
      crankEdgeMode.value = "both";
    }
  } catch {
    alignTdc.value = false;
    crankEdgeMode.value = "both";
  }
}

function persistUiSettings() {
  void setProjectUi(PERSIST_KEY_COMPOSITE_CHART, {
    alignTdc: alignTdc.value,
    captureDurationMs: captureDurationMs.value,
    crankEdgeMode: crankEdgeMode.value,
  });
}

watch(alignTdc, () => { persistUiSettings(); scheduleDraw(); });
watch(crankEdgeMode, () => { persistUiSettings(); scheduleDraw(); });

watch(
  [reviewMode, () => timelineStatus.value.viewEndSec, () => timelineStatus.value.spanSec],
  () => { void refreshReviewEvents(); },
);
watch(compositeTimelineLoadEpoch, () => { void refreshReviewEvents(); });

async function onViewportLinkChange(checked: boolean) {
  // Snapshot current output position BEFORE any async calls
  let savedViewport: { viewEndSec: number; spanSec: number } | undefined;
  if (!checked && viewportLinked.value && reviewMode.value) {
    const range = currentTimeRangeFromOutput();
    savedViewport = { viewEndSec: range.tEnd / 1_000_000, spanSec: range.spanUs / 1_000_000 };
  }
  // Unlink FIRST so that side-effects (watch on timelineStatus, Tauri event listener)
  // already see viewportLinked=false and don't re-fetch with the output viewport
  await setViewportLinked(checked);
  if (savedViewport) {
    // Now push the saved position into the composite timeline's own view
    await controlTimelineView({ followLive: false, ...savedViewport });
  }
  if (reviewMode.value) await refreshReviewEvents();
}

watch(
  () =>
    viewportLinked.value
      ? `${outputTimelineStatus.value.viewEndSec}:${outputTimelineStatus.value.spanSec}:${outputTimelineStatus.value.followLive}`
      : "",
  () => {
    if (!viewportLinked.value) return;
    if (reviewMode.value) { void refreshReviewEvents(); }
    else if (loggingEnabled.value) { scheduleDraw(); }
  },
);

watch(
  () =>
    viewportLinked.value && outputTimelineStatus.value.followLive
      ? outputChannelsSnapshot.value.timelineLiveSec
      : null,
  (liveSec) => { if (liveSec != null) scheduleDraw(); },
);

watch(loggingEnabled, (on, wasOn) => {
  if (on) { startLiveDraw(); }
  else { stopLiveDraw(); }
  if (wasOn && !on) { void refreshTimelineStatus().then(() => refreshReviewEvents()); }
});

watch(tabActive, (active, wasActive) => {
  if (active && !wasActive) { scheduleDraw(); if (loggingEnabled.value) startLiveDraw(); }
  else if (!active) { stopLiveDraw(); }
});

watch(
  () => snapshot.value.chunksReceived,
  (chunks) => {
    if (!loggingEnabled.value || !autoStopArmed || autoStopTimer != null) return;
    if (chunks > 0) {
      const ms = captureDurationMs.value;
      autoStopDeadlineMs = Date.now() + ms;
      autoStopRemainingSec.value = Math.ceil(ms / 1000);
      autoStopTimer = setInterval(() => {
        const left = autoStopDeadlineMs - Date.now();
        if (left <= 0) {
          clearAutoStopTimer();
          if (loggingEnabled.value && !loggerBusy.value) void applyLoggingEnabled(false);
          return;
        }
        autoStopRemainingSec.value = Math.ceil(left / 1000);
      }, 100);
      autoStopArmed = false;
    }
  },
);

// ---- TDC calibration (unchanged logic) -------------------------------------

const currentTriggerOffset = computed(() => getConfigField(GLOBAL_TRIGGER_ANGLE_OFFSET_FIELD));

function chartViewDeps(): void {
  void viewSpanMs.value; void viewAnchorT0Us.value; void alignTdc.value;
  void viewportLinked.value; void reviewMode.value;
  const ev = chartEvents();
  if (ev.length > 0) void ev[ev.length - 1]!.tUs;
}

const calibrationPreview = computed(() => {
  chartViewDeps();
  if (!tdcPlaceMode.value || !hoverInside.value || hoverX.value == null) return null;
  const view = getCurrentChartView();
  if (!view) return null;
  const tUs = timeAtX(hoverX.value, view);
  const deg = crankDegFromFirmwareTdc(tUs, view, snapshot.value.rpm);
  if (deg == null) return null;
  const delta = signedDegFromFirmwareTdc(deg);
  const cur = currentTriggerOffset.value;
  const next = cur != null ? computeNextGlobalTriggerAngleOffset(cur, deg) : null;
  return { deg, delta, next };
});

const calibrationAtMarker = computed(() => {
  chartViewDeps();
  if (realTdcTUs.value == null) return null;
  const view = getCurrentChartView();
  if (!view) return null;
  const deg = crankDegFromFirmwareTdc(realTdcTUs.value, view, snapshot.value.rpm);
  if (deg == null) return null;
  const delta = signedDegFromFirmwareTdc(deg);
  const cur = currentTriggerOffset.value;
  const next = cur != null ? computeNextGlobalTriggerAngleOffset(cur, deg) : null;
  return { deg, delta, next };
});

const canCalibrateTdc = computed(() => {
  chartViewDeps();
  const events = chartEvents();
  if (events.length < 2) return false;
  const view = getCurrentChartView();
  return view != null && view.tdcMarkersAll.length > 0;
});

const canWriteTriggerOffset = computed(
  () =>
    configCanEdit(configSnapshot.value) &&
    calibrationAtMarker.value?.next != null &&
    !offsetWriteBusy.value,
);

function toggleTdcPlaceMode() {
  if (tdcPlaceMode.value) {
    tdcPlaceMode.value = false;
    placePointerPending = false;
    offsetWriteError.value = null;
    scheduleDraw();
    return;
  }
  if (!canCalibrateTdc.value) {
    offsetWriteError.value = "Нет TDC ECU в буфере — сначала запись со стимом или синхронизацией.";
    return;
  }
  offsetWriteError.value = null;
  tdcPlaceMode.value = true;
  scheduleDraw();
}

async function applyTriggerAngleAdvance() {
  const cal = calibrationAtMarker.value;
  if (cal?.next == null) return;
  if (!configCanEdit(configSnapshot.value)) { offsetWriteError.value = "Конфиг недоступен для записи."; return; }
  offsetWriteBusy.value = true;
  offsetWriteError.value = null;
  try {
    await setConfigField(GLOBAL_TRIGGER_ANGLE_OFFSET_FIELD, cal.next);
    if (burnOffsetAfterWrite.value) await burnConfig();
    realTdcTUs.value = null;
  } catch (e) {
    offsetWriteError.value = e instanceof Error ? e.message : "Не удалось записать offset";
  } finally {
    offsetWriteBusy.value = false;
  }
}

function clearRealTdcMarker() {
  realTdcTUs.value = null;
  tdcPlaceMode.value = false;
  placePointerPending = false;
  offsetWriteError.value = null;
  scheduleDraw();
  scheduleWheelCompute();
}

function physicalTdcDegForWheels(): number | null {
  if (realTdcTUs.value == null) return null;
  const view = getCurrentChartView();
  if (!view) return null;
  const deg = crankDegFromFirmwareTdc(realTdcTUs.value, view, snapshot.value.rpm);
  if (deg == null) return null;
  return signedDegFromFirmwareTdc(deg);
}

// ---- Trigger wheels (unchanged) --------------------------------------------

function scheduleWheelCompute() {
  if (!showTriggerWheels.value) { triggerWheelsView.value = null; return; }
  if (wheelComputeTimer != null) { clearTimeout(wheelComputeTimer); wheelComputeTimer = null; }
  wheelComputeTimer = setTimeout(() => { wheelComputeTimer = null; void computeTriggerWheels(); }, 280);
}

async function allSessionEventsForWheels(): Promise<CompositeEvent[]> {
  if (reviewMode.value && timelineStatus.value.eventCount >= 4) {
    try { return await fetchCompositeSessionEvents(); } catch { /* fallback */ }
  }
  return snapshot.value.events as CompositeEvent[];
}

async function computeTriggerWheels() {
  const events = await allSessionEventsForWheels();
  if (events.length < 4) { triggerWheelsView.value = null; return; }
  try {
    triggerWheelsView.value = await invoke<TriggerWheelsView>("composite_compute_trigger_wheels", {
      params: {
        events,
        edgeMode: crankEdgeMode.value,
        triggerAngleAdvanceDeg: getConfigField(GLOBAL_TRIGGER_ANGLE_OFFSET_FIELD),
        physicalTdcDeg: physicalTdcDegForWheels(),
      },
    });
  } catch { triggerWheelsView.value = null; }
}

function onToggleAnalysis() {
  if (showTriggerAnalysis.value && !triggerAnalysis.value) {
    void computeTriggerAnalysis();
  }
}

async function computeTriggerAnalysis() {
  if (analysisBusy.value) return;
  analysisBusy.value = true;
  try {
    const events = await allSessionEventsForWheels();
    if (events.length < 8) {
      triggerAnalysis.value = null;
      return;
    }
    triggerAnalysis.value = await invoke<TriggerAnalysis>("composite_analyze_trigger", {
      params: { events, channel: "pri", edgeMode: "rise" },
    });
  } catch {
    triggerAnalysis.value = null;
  } finally {
    analysisBusy.value = false;
  }
}

/** Центрировать окно графика на сбое. Работает и в live, и в review (по `pos`). */
function jumpToFault(f: { tUs: number; pos: number }) {
  if (reviewMode.value) {
    const st = timelineStatus.value;
    const dataSpan = Math.max(0, st.dataMaxSec - st.dataMinSec);
    if (dataSpan <= 0) return;
    const span = st.spanSec;
    const centerSec = st.dataMinSec + f.pos * dataSpan;
    void controlTimelineView({
      followLive: false,
      viewEndSec: centerSec + span / 2,
      spanSec: span,
    });
    return;
  }
  const events = chartEvents();
  if (events.length < 2) return;
  const spanUs = viewSpanMs.value * 1000;
  viewAnchorT0Us.value = f.tUs - spanUs / 2;
  userAdjustedView.value = true;
  clampViewToBuffer(events);
  scheduleDraw();
}

function onTdcPlaceKeyDown(e: KeyboardEvent) {
  if (e.key === "Escape" && tdcPlaceMode.value) {
    tdcPlaceMode.value = false;
    placePointerPending = false;
    scheduleDraw();
  }
}

// ---- Lifecycle -------------------------------------------------------------

let unregUiFlush: (() => void) | null = null;

onMounted(async () => {
  unregUiFlush = registerProjectUiFlushHook(persistUiSettings);
  await initCompositeLogger();
  await initCompositeTimeline();
  await initOutputTimeline();
  await initConfig();

  const refreshIfLinked = () => {
    if (viewportLinked.value && reviewMode.value) void refreshReviewEvents();
  };
  await listen("output-timeline-status", refreshIfLinked);
  await listen("composite-timeline-status", refreshIfLinked);
  await loadUiFromProject();

  const canvas = glCanvasRef.value;
  const wrap = plotWrapRef.value;

  if (canvas) {
    renderer.attach(canvas);
    canvas.addEventListener("pointerdown", onPointerDown);
    canvas.addEventListener("pointermove", onPointerMove);
    canvas.addEventListener("pointerup", onPointerUp);
    canvas.addEventListener("pointercancel", onPointerCancel);
    canvas.addEventListener("pointerleave", onPointerLeave);
  }
  if (wrap) {
    ro = new ResizeObserver(scheduleDraw);
    ro.observe(wrap);
  } else if (canvas) {
    ro = new ResizeObserver(scheduleDraw);
    ro.observe(canvas);
  }

  resetViewWindow();
  scheduleDraw();
  scheduleWheelCompute();
  if (loggingEnabled.value) startLiveDraw();
  document.addEventListener("click", onDocClick);
  document.addEventListener("keydown", onTdcPlaceKeyDown);
});

function onDocClick() { durationDropdownOpen.value = false; }

onUnmounted(() => {
  unregUiFlush?.();
  if (wheelComputeTimer != null) { clearTimeout(wheelComputeTimer); wheelComputeTimer = null; }
  if (panWheelRafId !== 0) { cancelAnimationFrame(panWheelRafId); panWheelRafId = 0; }
  clearAutoStopTimer();
  stopLiveDraw();
  document.removeEventListener("click", onDocClick);
  document.removeEventListener("keydown", onTdcPlaceKeyDown);
  if (loggingEnabled.value) void setLoggingEnabled(false);
  ro?.disconnect();
  const canvas = glCanvasRef.value;
  canvas?.removeEventListener("pointerdown", onPointerDown);
  canvas?.removeEventListener("pointermove", onPointerMove);
  canvas?.removeEventListener("pointerup", onPointerUp);
  canvas?.removeEventListener("pointercancel", onPointerCancel);
  canvas?.removeEventListener("pointerleave", onPointerLeave);
  renderer.detach();
});

// ---- Watches for redraw (unchanged) ----------------------------------------

watch(
  () => { const ev = snapshot.value.events; if (ev.length === 0) return "0"; return `${ev.length}:${ev[ev.length - 1]!.tUs}`; },
  scheduleDraw,
);
watch([maxWindowMs, viewSpanMs, chartHeight, connected, alignTdc], scheduleDraw);
watch([hoverX, hoverInside], scheduleDraw);
watch([realTdcTUs, tdcPlaceMode], scheduleDraw);
watch(
  () => {
    const snap = snapshot.value.events;
    const n = reviewMode.value ? timelineStatus.value.eventCount : snap.length;
    const tail = snap.length > 0 ? snap[snap.length - 1]!.tUs : 0;
    return `${reviewMode.value}:${n}:${tail}:${compositeTimelineLoadEpoch.value}`;
  },
  () => scheduleWheelCompute(),
);
watch([crankEdgeMode, showTriggerWheels], () => scheduleWheelCompute());
// Анализ сбоев инвалидируем при смене источника (live↔review, новый файл):
// он считается по требованию, поэтому пере-считываем только если панель открыта.
watch([reviewMode, compositeTimelineLoadEpoch], () => {
  triggerAnalysis.value = null;
  if (showTriggerAnalysis.value) void computeTriggerAnalysis();
});
watch(
  () => getConfigField(GLOBAL_TRIGGER_ANGLE_OFFSET_FIELD),
  () => scheduleWheelCompute(),
);

// ---- Status line (unchanged) -----------------------------------------------

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
  if (reviewMode.value) { parts.push(`файл ${timelineStatus.value.eventCount} evt`); }
  else if (s.loggingEnabled) { parts.push("log on"); }
  if (autoStopRemainingSec.value != null && autoStopRemainingSec.value > 0) {
    parts.push(`стоп ${autoStopRemainingSec.value} с`);
  }
  if (s.polling) parts.push("poll");
  if (s.rpm != null) parts.push(`${Math.round(s.rpm)} RPM`);
  const cap = ev.length >= 2 ? maxSpanMsFor(ev) : maxWindowMs.value;
  const rec = s.recordedSpanMs > 0 ? s.recordedSpanMs : ev.length >= 2 ? bufferSpanMs(ev) : 0;
  const win = viewSpanMs.value < cap - 0.5
    ? `вид ${viewSpanMs.value.toFixed(0)}/${cap.toFixed(0)} ms`
    : `вид ${cap.toFixed(0)} ms`;
  parts.push(win);
  if (rec > 0 && !s.loggingEnabled) parts.push(`захват ${rec.toFixed(0)} ms`);
  if (s.recordedSpanMs > 0) parts.push(`запись ${s.recordedSpanMs.toFixed(0)} ms`);
  parts.push(`${s.events.length} pts`);
  if (tdcPlaceMode.value) parts.push("установка TDC");
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
          <template v-else>Старт · {{ formatDuration(captureDurationMs) }}</template>
        </button>
        <button
          v-if="!loggingEnabled"
          type="button"
          class="btn primary split-arrow"
          :disabled="!connected || loggerBusy"
          aria-label="Выбрать длительность записи"
          @click.stop="durationDropdownOpen = !durationDropdownOpen"
        >▾</button>
        <div v-if="durationDropdownOpen" class="split-dropdown" @click.stop>
          <button
            v-for="d in CAPTURE_DURATIONS_MS"
            :key="d"
            type="button"
            class="split-opt"
            :class="{ active: d === captureDurationMs }"
            @click="selectDuration(d)"
          >{{ formatDuration(d) }}</button>
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
      >Лог триггера…</button>
      <div class="cc-edge-seg" title="Фронты на графике и на дисках (↑ / ↓ / ↕)">
        <button
          v-for="m in (['both', 'rise', 'fall'] as CrankEdgeMode[])"
          :key="m"
          type="button"
          class="cc-edge-btn"
          :class="{ active: crankEdgeMode === m }"
          @click="crankEdgeMode = m; scheduleWheelCompute()"
        >{{ m === 'both' ? '↕' : m === 'rise' ? '↑' : '↓' }}</button>
      </div>
      <label class="cc-wheels-toggle" title="Диски коленвала и распредвала (усреднение по циклам TDC)">
        <input v-model="showTriggerWheels" type="checkbox" @change="scheduleWheelCompute()" />
        Диски
      </label>
      <label
        class="cc-wheels-toggle"
        title="Поиск сбоев декодирования: потерянные/лишние фронты и рассинхрон счёта зубьев"
      >
        <input v-model="showTriggerAnalysis" type="checkbox" @change="onToggleAnalysis()" />
        Анализ
      </label>
      <div
        class="cc-tdc-cal"
        title="Режим установки: клик по графику — реальный TDC; перетаскивание — сдвиг; Esc — отмена"
      >
        <button
          type="button"
          class="btn"
          :class="tdcPlaceMode ? 'stop' : 'secondary'"
          :disabled="(!tdcPlaceMode && !canCalibrateTdc) || offsetWriteBusy"
          @click="toggleTdcPlaceMode"
        >{{ tdcPlaceMode ? "Отмена установки" : "Установить TDC" }}</button>
        <button
          v-if="realTdcTUs != null && !tdcPlaceMode"
          type="button"
          class="btn secondary"
          :disabled="offsetWriteBusy"
          @click="clearRealTdcMarker"
        >Сброс</button>
        <button
          type="button"
          class="btn primary"
          :disabled="!canWriteTriggerOffset"
          @click="applyTriggerAngleAdvance"
        >{{ offsetWriteBusy ? "Запись…" : "В конфиг ECU" }}</button>
        <label v-if="canWriteTriggerOffset" class="cc-tdc-burn">
          <input v-model="burnOffsetAfterWrite" type="checkbox" />
          Burn
        </label>
      </div>
    </div>

    <p v-if="tdcPlaceMode" class="cc-tdc-hint">
      Клик по графику — реальный TDC (стробоскоп). Перетаскивание — сдвиг. Esc или «Отмена» — выход.
      <template v-if="calibrationPreview">
        <span class="cc-tdc-hint-preview">
          · превью Δ {{ calibrationPreview.delta >= 0 ? "+" : "" }}{{ calibrationPreview.delta.toFixed(1) }}°
          <template v-if="calibrationPreview.next != null"> → {{ calibrationPreview.next.toFixed(1) }}°</template>
        </span>
      </template>
    </p>
    <p v-else-if="calibrationAtMarker" class="cc-tdc-preview">
      <template v-if="currentTriggerOffset != null">Сейчас {{ currentTriggerOffset.toFixed(1) }}°</template>
      · реал. TDC: Δ {{ calibrationAtMarker.delta >= 0 ? "+" : "" }}{{ calibrationAtMarker.delta.toFixed(1) }}°
      <template v-if="calibrationAtMarker.next != null"> → запись {{ calibrationAtMarker.next.toFixed(1) }}°</template>
    </p>
    <p v-if="offsetWriteError" class="cc-error">{{ offsetWriteError }}</p>
    <p v-if="loggerError" class="cc-error">{{ loggerError }}</p>
    <p v-if="openLogError" class="cc-error">{{ openLogError }}</p>

    <div
      ref="plotWrapRef"
      class="cc-plot-wrap"
      :class="{ 'cc-plot-wrap--linked': viewportLinked && reviewMode }"
      :title="
        tdcPlaceMode
          ? 'Клик — реальный TDC; перетаскивание — сдвиг; Esc — отмена'
          : viewportLinked && reviewMode
            ? 'Связано с Log: управление на графике Log'
            : 'Клик — зум, Ctrl+клик — зум-аут, drag — пан'
      "
      @wheel="onCanvasWheel"
      @dblclick="onPlotDblClick"
    >
      <!-- WebGL waveform canvas -->
      <canvas
        ref="glCanvasRef"
        class="cc-canvas"
        :class="{ 'cc-canvas--place-tdc': tdcPlaceMode }"
        :style="{ height: `${chartHeight}px` }"
        aria-label="Composite trigger logger"
      />

      <!-- HTML overlay: labels, TDC markers, crosshair, empty state -->
      <div class="cc-overlay" :style="{ height: `${chartHeight}px` }" aria-hidden="true">

        <!-- Channel name labels — left column -->
        <span
          v-for="lbl in overlayLabels"
          :key="lbl.label"
          class="cc-ch-label"
          :style="{ top: `${lbl.top}px`, color: lbl.color }"
        >{{ lbl.label }}</span>

        <!-- TDC cycle vertical marker labels -->
        <div
          v-for="tdc in overlayTdcLabels"
          :key="tdc.left"
          class="cc-tdc-text"
          :style="{ left: `${tdc.left}px` }"
        >#{{ tdc.cycle }}<br/>0°</div>

        <!-- Real TDC marker label (bottom) -->
        <div
          v-if="overlayRealTdcLeft != null"
          class="cc-real-tdc-text"
          :style="{ left: `${overlayRealTdcLeft}px` }"
        >реал. TDC</div>

        <!-- Crosshair dots on each channel's current value level -->
        <template v-if="crosshairOverlay">
          <span
            v-for="ch in crosshairOverlay.channels"
            :key="ch.label"
            class="cc-ch-dot"
            :style="{ left: `${crosshairOverlay.x}px`, top: `${ch.dotY}px`, background: ch.color }"
          />
          <!-- Crosshair tooltip -->
          <div
            class="cc-ch-tooltip"
            :style="{
              left: `${crosshairOverlay.tipLeft}px`,
              top: `${crosshairOverlay.tipTop}px`,
              width: `${crosshairOverlay.tipW}px`,
            }"
          >
            <div class="cc-tip-angle">° {{ crosshairOverlay.angle.toFixed(1) }}</div>
            <div
              v-for="ch in crosshairOverlay.channels"
              :key="ch.label"
              class="cc-tip-ch"
              :style="{ color: ch.color }"
            >{{ ch.label }}: {{ ch.val ? "1" : "0" }}</div>
          </div>
        </template>

        <!-- Crank cycle label bottom-right -->
        <div v-if="currentView" class="cc-cycle-label">цикл {{ CRANK_CYCLE_DEG }}°</div>

        <!-- Empty state message -->
        <div v-if="!currentView" class="cc-empty-msg">{{ emptyMsg }}</div>
      </div>
    </div>

    <CompositeTriggerWheels
      v-if="showTriggerWheels"
      :view="triggerWheelsView"
      :edge-mode="crankEdgeMode"
    />
    <CompositeTriggerAnalysis
      v-if="showTriggerAnalysis"
      :analysis="triggerAnalysis"
      :busy="analysisBusy"
      @refresh="computeTriggerAnalysis()"
      @jump="jumpToFault"
    />
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
      всё накопленное. «Установить TDC» → клик по графику → «В конфиг ECU» (Trigger Angle Advance).
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
  width: 100%;
  max-width: 100%;
  min-width: 0;
  box-sizing: border-box;
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
.cc-toolbar .btn:disabled { opacity: 0.5; cursor: not-allowed; }
.cc-toolbar .btn.primary { background: var(--color-accent, #3b82f6); color: #fff; border-color: transparent; }
.cc-toolbar .btn.secondary { background: var(--color-bg); color: var(--color-fg); }

.cc-autostart {
  display: flex; align-items: center; gap: 0.35rem;
  font-size: 0.72rem; color: var(--color-gray); margin-left: auto; cursor: pointer;
}
.cc-autostart input { margin: 0; }

/* Split-кнопка */
.btn-split { position: relative; display: inline-flex; }
.btn-split .split-main {
  border-radius: var(--radius-sm) 0 0 var(--radius-sm);
  border-right: 1px solid rgba(255 255 255 / 0.25) !important;
}
.btn-split .split-main:last-child { border-radius: var(--radius-sm); border-right: none !important; }
.cc-toolbar .btn.stop { background: var(--color-error, #ef4444); color: #fff; border-color: transparent; }
.split-remain { margin-left: 0.35rem; opacity: 0.75; font-size: 0.7em; font-variant-numeric: tabular-nums; }
.btn-split .split-arrow { border-radius: 0 var(--radius-sm) var(--radius-sm) 0; padding: 0.25rem 0.4rem; font-size: 0.65rem; line-height: 1; }
.split-dropdown {
  position: absolute; top: calc(100% + 3px); left: 0; z-index: 200;
  background: var(--color-bg-elevated, #1e1e2e); border: 1px solid var(--color-border);
  border-radius: var(--radius-sm); box-shadow: 0 4px 12px rgba(0 0 0 / 0.35);
  min-width: 7rem; display: flex; flex-direction: column; overflow: hidden;
}
.split-opt { padding: 0.35rem 0.75rem; font-size: 0.75rem; text-align: left; background: none; border: none; color: var(--color-fg, #e2e8f0); cursor: pointer; transition: background 0.1s; }
.split-opt:hover { background: var(--color-bg-muted, #2a2a3e); }
.split-opt.active { color: var(--color-accent, #3b82f6); font-weight: 600; }

/* Edge mode */
.cc-edge-seg { display: flex; border: 1px solid var(--color-border, #374151); border-radius: 5px; overflow: hidden; }
.cc-edge-btn { padding: 2px 8px; font-size: 0.85rem; background: none; border: none; color: var(--color-fg-muted, #9ca3af); cursor: pointer; line-height: 1; transition: background 0.12s, color 0.12s; }
.cc-edge-btn:not(:last-child) { border-right: 1px solid var(--color-border, #374151); }
.cc-edge-btn:hover { background: var(--color-bg-muted, #2a2a3e); color: var(--color-fg); }
.cc-edge-btn.active { background: var(--color-accent, #3b82f6); color: #fff; }

/* TDC calibration */
.cc-tdc-cal { display: flex; flex-wrap: wrap; align-items: center; gap: 0.35rem; padding-left: 0.35rem; border-left: 1px solid var(--color-border, #374151); }
.cc-tdc-burn { display: flex; align-items: center; gap: 0.25rem; font-size: 0.72rem; color: var(--color-gray); cursor: pointer; }
.cc-tdc-burn input { margin: 0; }
.cc-tdc-preview { margin: 0; font-size: 0.72rem; color: var(--color-fg-muted, #9ca3af); font-variant-numeric: tabular-nums; }
.cc-tdc-hint { margin: 0; font-size: 0.72rem; color: var(--color-warning, #d97706); }
.cc-tdc-hint-preview { color: var(--color-fg-muted, #9ca3af); font-variant-numeric: tabular-nums; }

/* Wheels toggle */
.cc-wheels-toggle { display: flex; align-items: center; gap: 0.3rem; font-size: 0.72rem; color: var(--color-gray); cursor: pointer; }
.cc-wheels-toggle input { margin: 0; }

/* Canvas + overlay wrapper */
.cc-canvas--place-tdc {
  cursor: cell;
  outline: 2px solid color-mix(in srgb, var(--color-accent, #3b82f6) 55%, transparent);
  outline-offset: -2px;
}

.cc-plot-wrap {
  position: relative;
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

/* Overlay */
.cc-overlay {
  position: absolute;
  inset: 0;
  pointer-events: none;
  overflow: hidden;
}

/* Channel labels — left column */
.cc-ch-label {
  position: absolute;
  left: 0;
  width: 38px;
  text-align: right;
  font-size: 11px;
  font-family: system-ui, sans-serif;
  line-height: 1;
  transform: translateY(-50%);
  white-space: nowrap;
}

/* TDC cycle labels at top of each TDC vertical line */
.cc-tdc-text {
  position: absolute;
  top: 4px;
  font-size: 10px;
  font-weight: 700;
  font-family: system-ui, sans-serif;
  color: var(--color-warning, #d97706);
  transform: translateX(-50%);
  line-height: 1.2;
  white-space: nowrap;
}

/* Real TDC label at bottom of its dashed line */
.cc-real-tdc-text {
  position: absolute;
  bottom: 4px;
  font-size: 10px;
  font-weight: 700;
  font-family: system-ui, sans-serif;
  color: var(--color-accent, #3b82f6);
  transform: translateX(-50%);
  white-space: nowrap;
}

/* Crosshair dot on each channel waveform */
.cc-ch-dot {
  position: absolute;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  transform: translate(-50%, -50%);
  box-shadow: 0 0 0 1.5px var(--color-bg, #0f1115);
}

/* Crosshair tooltip */
.cc-ch-tooltip {
  position: absolute;
  background: var(--color-bg-elevated, rgba(20,22,28,0.94));
  border: 1px solid var(--color-border, #444);
  border-radius: 3px;
  padding: 5px;
  font-family: ui-monospace, monospace;
  font-size: 10px;
  line-height: 1.3;
}
.cc-tip-angle { color: var(--color-text, var(--color-fg, #e2e8f0)); font-weight: 600; }
.cc-tip-ch { margin-top: 1px; }

/* Bottom-right cycle label */
.cc-cycle-label {
  position: absolute;
  bottom: 3px;
  right: 12px;
  font-size: 9px;
  font-family: system-ui, sans-serif;
  color: var(--color-gray, #6b7280);
}

/* Empty state */
.cc-empty-msg {
  position: absolute;
  top: 50%;
  left: 44px;
  transform: translateY(-50%);
  font-size: 12px;
  font-family: system-ui, sans-serif;
  color: var(--color-gray, #888);
}

/* Common text styles */
.cc-title { font-size: 0.8rem; font-weight: 600; color: var(--color-fg); }
.cc-status { font-size: 0.72rem; color: var(--color-gray); font-variant-numeric: tabular-nums; }
.cc-status.warn { color: var(--color-warning, #d97706); }
.cc-error { margin: 0; font-size: 0.72rem; color: var(--color-danger, #dc2626); }
.cc-hint { margin: 0; font-size: 0.72rem; color: var(--color-gray); }
</style>
