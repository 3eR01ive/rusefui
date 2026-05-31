<script setup lang="ts">
import {
  computed,
  onMounted,
  onUnmounted,
  ref,
  shallowRef,
  watch,
} from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useInstanceBind } from "../../composables/useInstanceBind";
import { initOutputChannels, useOutputChannels } from "../../composables/useOutputChannels";
import { useEcuConnection } from "../../composables/useEcuConnection";
import { useDataContext } from "../../core/data-context";
import { useOutputFields } from "../../composables/useOutputFields";
import {
  initProject,
  PERSIST_KEY_OUTPUT_CHART,
  projectUiEpoch,
  workspaceResetEpoch,
  useProject,
  type LogUiSettings,
} from "../../composables/useProject";
import {
  initOutputTimeline,
  useOutputTimeline,
  type OutputTimelineView,
} from "../../composables/useOutputTimeline";
import { useLogViewportLink } from "../../composables/useLogViewportLink";
import { useTabActivity } from "../../composables/useTabActivity";
import {
  logPanelMargins,
  plotXToTime,
  type LogGraphPanelSpec,
  type LogTraceSpec,
} from "../../composables/drawTimeSeriesChart";
import { logChartRenderer } from "../../composables/logChartRenderer";
import {
  buildLogChartOverlay,
  labelStyle,
  markerLabelStyle,
  type LogChartOverlay,
} from "../../composables/logChartOverlay";
import type { TimeSeries } from "../../composables/useTimeSeriesBuffer";

interface LogGraphGroup {
  id: string;
  fieldNames: string[];
}

const MAX_CHANNELS = 12;
const MAX_GRAPHS = 6;
let graphIdSeq = 1;

function nextGraphId(): string {
  graphIdSeq += 1;
  return `g${graphIdSeq}`;
}

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const chartSizeOverride = {
  window: null as number | null,
  height: null as number | null,
};

const windowSeconds = computed(() => {
  if (chartSizeOverride.window !== null && chartSizeOverride.window > 0) {
    return chartSizeOverride.window;
  }
  const w = Number(props.props.windowSeconds ?? 30);
  return w > 0 ? w : 30;
});

const chartHeight = computed(() => {
  if (chartSizeOverride.height !== null && chartSizeOverride.height > 120) {
    return chartSizeOverride.height;
  }
  const h = Number(props.props.height ?? 220);
  return h > 120 ? h : 220;
});

const ZOOM_STEP_MIN = 1;
const ZOOM_STEP_MAX = 40;

const settingsExpanded = ref(false);

function clampZoomStepPct(n: number): number {
  if (!Number.isFinite(n)) return 1;
  return Math.min(ZOOM_STEP_MAX, Math.max(ZOOM_STEP_MIN, Math.round(n)));
}

function zoomStepFromProps(): number {
  const fromProps = Number(props.props.zoomStepPercent);
  if (Number.isFinite(fromProps)) return clampZoomStepPct(fromProps);
  return 1;
}

const zoomStepPct = ref(zoomStepFromProps());
const ZOOM_SPEED = 6;
const zoomStepFactor = computed(() => 1 + (zoomStepPct.value * ZOOM_SPEED) / 100);
/** Колёсико — мельче кнопок ±, но с тем же коэффициентом скорости. */
const wheelZoomFactor = computed(() => 1 + (zoomStepPct.value * ZOOM_SPEED) / 1000);

function onZoomStepChange(): void {
  zoomStepPct.value = clampZoomStepPct(zoomStepPct.value);
  scheduleSaveLogUiToProject();
}

function toggleSettingsExpanded(): void {
  settingsExpanded.value = !settingsExpanded.value;
  if (!settingsExpanded.value) {
    showSuggest.value = false;
  }
  scheduleSaveLogUiToProject();
}

const instanceRef = computed(() => props.instance);
const { fields: boundFields, source: bindSource } = useInstanceBind(instanceRef);

const defaultFields = computed(() => {
  if (bindSource.value && bindSource.value !== "outputChannels") {
    console.warn(
      `[output-chart] ожидался bind.source=outputChannels, получен ${bindSource.value}`,
    );
  }
  return boundFields.value;
});

const canvasRef = ref<HTMLCanvasElement | null>(null);
const canvasWrapRef = ref<HTMLDivElement | null>(null);
const chartOverlay = ref<LogChartOverlay>({ crosshairX: null, labels: [], markers: [] });
const searchInputRef = ref<HTMLInputElement | null>(null);
const canvasWidth = ref(0);

function measureCanvasWidth(): number {
  const wrap = canvasWrapRef.value;
  if (wrap && wrap.clientWidth > 0) {
    return Math.max(200, Math.floor(wrap.clientWidth));
  }
  return Math.max(200, canvasWidth.value);
}

const { snapshot } = useOutputChannels();
const { isActive: tabActive } = useTabActivity();
const { fields: allFields, reload: reloadOutputFields } = useOutputFields();
const { offlineMode } = useEcuConnection(useDataContext());
const { getProjectUi, setProjectUi } = useProject();

let applyingProjectUi = false;
let saveLogUiTimer = 0;
const {
  status: timelineStatus,
  hasHistory: timelineHasHistory,
  fieldColor,
  pullSeriesChunk,
  seriesRevision,
  controlView,
  refreshStatus: refreshTimelineStatus,
  loadEpoch,
  valueRangeForPoints,
  pickAndLoadFile,
} = useOutputTimeline();
const { linked: viewportLinked, setLinked: setViewportLinked } = useLogViewportLink();
const openingOutputLog = ref(false);
const openOutputLogError = ref<string | null>(null);

async function onOpenOutputLog() {
  openingOutputLog.value = true;
  openOutputLogError.value = null;
  try {
    await pickAndLoadFile();
  } catch (e) {
    openOutputLogError.value = e instanceof Error ? e.message : String(e);
  } finally {
    openingOutputLog.value = false;
  }
}

async function runFileLoadStream(): Promise<void> {
  invalidateSeriesCache();
  lastView.value = null;
  localViewport.value = null;
  seriesStreamGen += 1;
  const st = await refreshTimelineStatus();
  if (st.dataMaxSec > st.dataMinSec) {
    await clampTimelineViewToData(st.dataMinSec, st.dataMaxSec);
  }
  await streamSeriesIntoCache(true);
  scheduleRedraw(true);
}

const graphGroups = ref<LogGraphGroup[]>([
  { id: "g1", fieldNames: [...defaultFields.value] },
]);
const activeGraphId = ref("g1");

/** Все назначения каналов (один параметр может быть на нескольких графах). */
function allSelectedFields(): string[] {
  return graphGroups.value.flatMap((g) => g.fieldNames);
}

/** Уникальные имена для опроса ECU и буфера рядов. */
function uniquePollFields(): string[] {
  return [...new Set(allSelectedFields())];
}

function activeGraph(): LogGraphGroup | undefined {
  return (
    graphGroups.value.find((g) => g.id === activeGraphId.value) ?? graphGroups.value[0]
  );
}

function isFieldOnActiveGraph(name: string): boolean {
  const g = activeGraph();
  return g?.fieldNames.includes(name) ?? false;
}

function isFieldOnAnyGraph(name: string): boolean {
  return allSelectedFields().includes(name);
}

function syncGraphFields(): void {
  syncRangeInputs();
}

async function refreshFieldCatalog(): Promise<void> {
  await reloadOutputFields();
  const defaults = defaultFields.value.filter((f) =>
    allFields.value.length === 0 ? true : allFields.value.some((x) => x.name === f),
  );
  const names =
    defaults.length > 0
      ? defaults
      : allFields.value.length > 0
        ? [allFields.value[0]!.name]
        : [];
  graphGroups.value = [{ id: "g1", fieldNames: names }];
  activeGraphId.value = "g1";
  graphIdSeq = 1;
  syncGraphFields();
}
const fieldFilter = ref("");
const showSuggest = ref(false);
/** Канал под курсором/фокусом списка (превью на графе без добавления до клика). */
const suggestPreviewField = ref<string | null>(null);
const suggestListRef = ref<HTMLUListElement | null>(null);
const suggestStyle = ref({ top: "0px", left: "0px", width: "0px" });
const lastView = shallowRef<OutputTimelineView | null>(null);
/** Полные ряды из Rust RAM — pan/zoom только сдвигают окно локально. */
const seriesCache = new Map<string, { t: number; v: number }[]>();
let seriesStreamGen = 0;
let seriesFetchInflight: Promise<void> | null = null;

function updateSuggestPosition(): void {
  const el = searchInputRef.value;
  if (!el) return;
  const r = el.getBoundingClientRect();
  suggestStyle.value = {
    top: `${r.bottom + 2}px`,
    left: `${r.left}px`,
    width: `${r.width}px`,
  };
}

function openSuggest(): void {
  showSuggest.value = true;
  updateSuggestPosition();
}

function closeSuggestSoon(): void {
  window.setTimeout(() => {
    showSuggest.value = false;
  }, 160);
}

function effectiveSuggestPreviewField(): string | null {
  if (!showSuggest.value) return null;
  const p = suggestPreviewField.value;
  if (!p || isFieldOnActiveGraph(p)) return null;
  return p;
}

function fieldsForTimelineQuery(): string[] {
  const base = uniquePollFields();
  const extra = effectiveSuggestPreviewField();
  if (extra && !base.includes(extra)) {
    return [...base, extra];
  }
  return base;
}

function setSuggestPreview(name: string): void {
  if (!showSuggest.value) return;
  if (isFieldOnActiveGraph(name)) {
    if (suggestPreviewField.value !== null) {
      suggestPreviewField.value = null;
      void pullLiveSeriesDelta().then(() => scheduleRedraw(true));
    }
    return;
  }
  if (suggestPreviewField.value === name) return;
  suggestPreviewField.value = name;
  void pullLiveSeriesDelta().then(() => scheduleRedraw(true));
}

function clearSuggestPreview(): void {
  if (suggestPreviewField.value === null) return;
  suggestPreviewField.value = null;
  scheduleRedraw(true);
}

function onSuggestItemBlur(): void {
  window.requestAnimationFrame(() => {
    const root = suggestListRef.value;
    const ae = document.activeElement as Node | null;
    if (root && ae && root.contains(ae)) return;
    clearSuggestPreview();
  });
}

function onSuggestPick(name: string): void {
  suggestPreviewField.value = null;
  toggleField(name);
}

/** min/max для шкалы Y; пустая строка = авто по данным окна. */
const rangeInputs = ref<Record<string, { min: string; max: string }>>({});

function syncRangeInputs(): void {
  const next: Record<string, { min: string; max: string }> = {};
  for (const name of uniquePollFields()) {
    next[name] = rangeInputs.value[name] ?? { min: "", max: "" };
  }
  rangeInputs.value = next;
}

function parseRangeInput(raw: string): number | null {
  const t = raw.trim();
  if (!t) return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

function setRangeMin(name: string, value: string): void {
  const prev = rangeInputs.value[name] ?? { min: "", max: "" };
  rangeInputs.value = { ...rangeInputs.value, [name]: { ...prev, min: value } };
  scheduleRedraw();
}

function setRangeMax(name: string, value: string): void {
  const prev = rangeInputs.value[name] ?? { min: "", max: "" };
  rangeInputs.value = { ...rangeInputs.value, [name]: { ...prev, max: value } };
  scheduleRedraw();
}

watch(
  () => snapshot.value.iniFieldCount ?? 0,
  (count, prev) => {
    if (count > 0 && count !== prev) {
      void reloadOutputFields();
    }
  },
);

watch(offlineMode, () => {
  void reloadOutputFields();
});

watch(windowSeconds, (sec) => {
  void controlView({ spanSec: sec, followLive: timelineStatus.value.followLive });
});

watch(graphGroups, () => syncGraphFields(), { deep: true });

function buildLogUiSettings(): LogUiSettings {
  const st = timelineStatus.value;
  const lv = localViewport.value;
  return {
    windowSeconds: windowSeconds.value,
    chartHeight: chartHeight.value,
    zoomStepPct: zoomStepPct.value,
    settingsExpanded: settingsExpanded.value,
    graphGroups: graphGroups.value.map((g) => ({
      id: g.id,
      fieldNames: [...g.fieldNames],
    })),
    activeGraphId: activeGraphId.value,
    rangeInputs: Object.fromEntries(
      Object.entries(rangeInputs.value).map(([k, v]) => [k, { min: v.min, max: v.max }]),
    ),
    followLive: lv ? false : st.followLive,
    spanSec: lv?.spanSec ?? st.spanSec,
  };
}

function resolvedSpanSec(ui: LogUiSettings): number {
  if (ui.spanSec !== undefined && ui.spanSec > 0) return ui.spanSec;
  if (ui.windowSeconds > 0) return ui.windowSeconds;
  return windowSeconds.value;
}

async function applyLogUiFromProject(): Promise<void> {
  applyingProjectUi = true;
  try {
    const ui = await getProjectUi<LogUiSettings>(PERSIST_KEY_OUTPUT_CHART);
    const span = resolvedSpanSec(ui);
    chartSizeOverride.window = ui.windowSeconds > 0 ? ui.windowSeconds : null;
    chartSizeOverride.height = ui.chartHeight > 120 ? ui.chartHeight : null;
    zoomStepPct.value = clampZoomStepPct(ui.zoomStepPct);
    settingsExpanded.value = ui.settingsExpanded;
    graphGroups.value =
      ui.graphGroups.length > 0
        ? ui.graphGroups.map((g) => ({ id: g.id, fieldNames: [...g.fieldNames] }))
        : [{ id: "g1", fieldNames: [...defaultFields.value] }];
    activeGraphId.value =
      graphGroups.value.some((g) => g.id === ui.activeGraphId)
        ? ui.activeGraphId
        : graphGroups.value[0]!.id;
    rangeInputs.value = Object.fromEntries(
      Object.entries(ui.rangeInputs).map(([k, v]) => [k, { min: v.min, max: v.max }]),
    );
    syncGraphFields();
    localViewport.value = null;
    const followLive = ui.followLive ?? true;
    const st = await refreshTimelineStatus();
    const ctrl: Parameters<typeof controlView>[0] = { spanSec: span, followLive };
    if (!followLive && st.dataMaxSec > st.dataMinSec) {
      ctrl.viewEndSec = st.dataMaxSec;
    }
    await controlView(ctrl);
    scheduleRedraw();
  } catch {
    await controlView({ spanSec: windowSeconds.value, followLive: true });
  } finally {
    applyingProjectUi = false;
  }
}

function scheduleSaveLogUiToProject(): void {
  if (applyingProjectUi) return;
  if (saveLogUiTimer !== 0) window.clearTimeout(saveLogUiTimer);
  saveLogUiTimer = window.setTimeout(() => {
    saveLogUiTimer = 0;
    void setProjectUi(PERSIST_KEY_OUTPUT_CHART, buildLogUiSettings());
  }, 400);
}

watch(projectUiEpoch, () => {
  void applyLogUiFromProject();
});

watch(workspaceResetEpoch, () => {
  invalidateSeriesCache();
  lastView.value = null;
  seriesStreamGen += 1;
  void refreshTimelineStatus().then(() => {
    void applyLogUiFromProject().then(() => {
      void streamSeriesIntoCache(true).then(() => scheduleRedraw(true));
    });
  });
});

watch(
  [
    graphGroups,
    rangeInputs,
    zoomStepPct,
    settingsExpanded,
    windowSeconds,
    chartHeight,
    () => timelineStatus.value.followLive,
    () => timelineStatus.value.spanSec,
  ],
  () => scheduleSaveLogUiToProject(),
  { deep: true },
);

function fieldValue(name: string): number | null {
  const live = snapshot.value.values[name];
  if (live !== undefined) return live;
  const series = lastView.value?.series.find((s) => s.field === name);
  const pts = series?.points;
  if (pts && pts.length > 0) return pts[pts.length - 1]!.v;
  return null;
}

const channelRows = computed(() => {
  const rows: {
    slotKey: string;
    name: string;
    graphId: string;
    graphLabel: string;
    color: string;
    units: string;
    value: number | null;
    min: string;
    max: string;
  }[] = [];
  graphGroups.value.forEach((g, gi) => {
    g.fieldNames.forEach((name, idx) => {
      const meta = allFields.value.find((f) => f.name === name);
      const last = fieldValue(name);
      const ranges = rangeInputs.value[name] ?? { min: "", max: "" };
      rows.push({
        slotKey: `${g.id}:${name}:${idx}`,
        name,
        graphId: g.id,
        graphLabel: `Граф ${gi + 1}`,
        color: fieldColor(name),
        units: meta?.units ?? "",
        value: last,
        min: ranges.min,
        max: ranges.max,
      });
    });
  });
  return rows;
});

const PANEL_GAP_UI = 2;

const canvasHeight = computed(() => {
  const n = Math.max(1, graphGroups.value.length);
  const perPanel = chartHeight.value;
  return perPanel * n + PANEL_GAP_UI * Math.max(0, n - 1) + 4;
});

const hasAnyChannel = computed(() => allSelectedFields().length > 0);

const setupSummary = computed(() => {
  const ch = allSelectedFields().length;
  const gr = graphGroups.value.filter((g) => g.fieldNames.length > 0).length;
  const parts: string[] = [];
  if (ch > 0) parts.push(`${ch} кан.`);
  parts.push(`${gr || graphGroups.value.length} граф.`);
  parts.push(`окно ${(localViewport.value?.spanSec ?? timelineStatus.value.spanSec).toFixed(0)} с`);
  if (!timelineStatus.value.followLive) parts.push("пауза");
  if (viewportLinked.value) {
    const o = timelineStatus.value;
    const t0 = (o.viewEndSec - o.spanSec) * 1000;
    const t1 = o.viewEndSec * 1000;
    parts.push(`↔ Trigger ${t0.toFixed(0)}–${t1.toFixed(0)} ms`);
  }
  return parts.join(" · ");
});

async function onViewportLinkChange(checked: boolean) {
  await setViewportLinked(checked);
}

const filteredFields = computed(() => {
  const q = fieldFilter.value.trim().toLowerCase();
  const list = allFields.value;
  if (!list.length) return [];
  if (!q) return list.slice(0, 80);
  return list.filter((f) => f.name.toLowerCase().includes(q)).slice(0, 80);
});

const suggestEmptyHint = computed(() => {
  if (allFields.value.length === 0) {
    if (snapshot.value.connected) {
      return "INI без output channels — переподключите ECU";
    }
    if (offlineMode.value) {
      return "Нет INI offline — задайте RUSEFI_INI_PATH или положите rusefi_*.ini в test_data / generated";
    }
    return "Подключите ECU или включите Offline — список из INI";
  }
  if (fieldFilter.value.trim() && filteredFields.value.length === 0) {
    return "Нет совпадений";
  }
  return null;
});

function fallbackSeries(name: string, tMin: number, tMax: number): TimeSeries | null {
  const v = snapshot.value.values[name];
  if (v === undefined || !snapshot.value.connected) return null;
  const t0 = Math.max(tMin, tMax - 0.001);
  return {
    field: name,
    color: fieldColor(name),
    points: [
      { t: t0, v },
      { t: tMax, v },
    ],
  };
}

function timelineLiveSec(): number {
  const fromSnap = snapshot.value.timelineLiveSec;
  if (fromSnap !== undefined && Number.isFinite(fromSnap)) return fromSnap;
  return timelineStatus.value.liveSec;
}

function buildPanels(
  _tMin: number,
  _tMax: number,
  resolveSeries: (name: string) => TimeSeries | null,
  previewFieldName: string | null,
): LogGraphPanelSpec[] {
  const panels: LogGraphPanelSpec[] = [];
  graphGroups.value.forEach((group, gi) => {
    const traces: LogTraceSpec[] = [];
    for (const name of group.fieldNames) {
      const s = resolveSeries(name);
      if (!s) continue;
      const inp = rangeInputs.value[name] ?? { min: "", max: "" };
      const { vMin, vMax } = valueRangeForPoints(
        s.points.filter((p) => p.t >= _tMin - 1e-9 && p.t <= _tMax + 1e-9),
        parseRangeInput(inp.min),
        parseRangeInput(inp.max),
      );
      const meta = allFields.value.find((f) => f.name === name);
      traces.push({
        series: s,
        vMin,
        vMax,
        name,
        units: meta?.units ?? "",
        color: s.color,
      });
    }
    if (
      previewFieldName &&
      group.id === activeGraphId.value &&
      !group.fieldNames.includes(previewFieldName)
    ) {
      const s = resolveSeries(previewFieldName);
      if (s) {
        const inp = rangeInputs.value[previewFieldName] ?? { min: "", max: "" };
        const { vMin, vMax } = valueRangeForPoints(
          s.points.filter((p) => p.t >= _tMin - 1e-9 && p.t <= _tMax + 1e-9),
          parseRangeInput(inp.min),
          parseRangeInput(inp.max),
        );
        const meta = allFields.value.find((f) => f.name === previewFieldName);
        traces.push({
          series: s,
          vMin,
          vMax,
          name: previewFieldName,
          units: meta?.units ?? "",
          color: s.color,
          preview: true,
        });
      }
    }
    if (traces.length > 0 || graphGroups.value.length > 1) {
      panels.push({ traces, title: `Граф ${gi + 1}` });
    }
  });
  return panels;
}

/** Кривая до краёв области графика (удержание крайних значений), ось = [tMin, tMax]. */
function padSeriesToAxisEdges(
  points: { t: number; v: number }[],
  tMin: number,
  tMax: number,
): { t: number; v: number }[] {
  const pts = points
    .map((p) => ({ t: Number(p.t), v: Number(p.v) }))
    .filter((p) => Number.isFinite(p.t) && Number.isFinite(p.v))
    .sort((a, b) => a.t - b.t);
  if (pts.length === 0) return pts;
  const out: { t: number; v: number }[] = [];
  const first = pts[0]!;
  const last = pts[pts.length - 1]!;
  if (first.t > tMin + 1e-9) {
    out.push({ t: tMin, v: first.v });
  }
  out.push(...pts);
  if (last.t < tMax - 1e-9) {
    const gap = tMax - last.t;
    const maxPad = Math.max((tMax - tMin) * 0.02, 1.0);
    if (gap <= maxPad) {
      out.push({ t: tMax, v: last.v });
    }
  }
  return out;
}

function seriesForField(name: string, view: OutputTimelineView): TimeSeries | null {
  const fv = view.series.find((s) => s.field === name);
  let base: TimeSeries | null = null;
  if (fv && fv.points.length > 0) {
    base = {
      field: name,
      color: fieldColor(name),
      points: fv.points.map((p) => ({ t: Number(p.t), v: Number(p.v) })),
    };
  } else {
    base = fallbackSeries(name, view.tMin, view.tMax);
  }
  if (!base) return null;
  const withTail = withLiveTail(base, view);
  return {
    ...withTail,
    points: padSeriesToAxisEdges(withTail.points, view.tMin, view.tMax),
  };
}

/** Хвост кривой из live snapshot (elapsed_sec timeline), между query_view. */
function withLiveTail(series: TimeSeries, view: OutputTimelineView): TimeSeries {
  const live = snapshot.value.values[series.field];
  if (live === undefined || !snapshot.value.connected) return series;
  if (!timelineStatus.value.followLive) return series;
  const tMax = Math.max(view.tMax, timelineLiveSec());
  const pts = series.points.map((p) => ({ t: p.t, v: p.v }));
  if (pts.length === 0) {
    const t0 = Math.max(view.tMin, tMax - Math.max(view.tMax - view.tMin, 0.05) * 0.02);
    return {
      ...series,
      points: [
        { t: t0, v: live },
        { t: tMax, v: live },
      ],
    };
  }
  const last = pts[pts.length - 1]!;
  const gap = tMax - last.t;
  const span = Math.max(view.tMax - view.tMin, 0.05);
  const maxBridge = Math.max(span * 0.02, 1.0);
  if (gap > maxBridge) {
    // Кэш отстаёт — не рисуем диагональ через весь график; точки подтянутся на след. pull.
    return series;
  }
  if (gap < 1e-9) {
    pts[pts.length - 1] = { t: tMax, v: live };
  } else {
    pts.push({ t: tMax, v: live });
  }
  return { ...series, points: pts };
}

const legendItems = computed(() =>
  channelRows.value.map((row) => ({
    slotKey: row.slotKey,
    graphId: row.graphId,
    name: row.name,
    graphLabel: row.graphLabel,
    color: row.color,
    units: row.units,
    value: row.value,
  })),
);

function addGraph(): void {
  if (graphGroups.value.length >= MAX_GRAPHS) return;
  const id = nextGraphId();
  graphGroups.value = [...graphGroups.value, { id, fieldNames: [] }];
  activeGraphId.value = id;
}

function removeGraph(id: string): void {
  if (graphGroups.value.length <= 1) return;
  graphGroups.value = graphGroups.value.filter((x) => x.id !== id);
  if (activeGraphId.value === id) {
    activeGraphId.value = graphGroups.value[0]!.id;
  }
  syncGraphFields();
}

function moveFieldToGraph(name: string, fromGraphId: string, toGraphId: string): void {
  if (fromGraphId === toGraphId) return;
  const from = graphGroups.value.find((g) => g.id === fromGraphId);
  const to = graphGroups.value.find((g) => g.id === toGraphId);
  if (!from || !to) return;
  const idx = from.fieldNames.indexOf(name);
  if (idx < 0) return;
  from.fieldNames = from.fieldNames.filter((_, i) => i !== idx);
  if (!to.fieldNames.includes(name)) {
    to.fieldNames = [...to.fieldNames, name];
  }
  graphGroups.value = [...graphGroups.value];
  syncGraphFields();
  scheduleRedraw();
}

function toggleField(name: string): void {
  const g = activeGraph();
  if (!g) return;
  if (g.fieldNames.includes(name)) {
    g.fieldNames = g.fieldNames.filter((f) => f !== name);
    graphGroups.value = [...graphGroups.value];
    syncGraphFields();
    return;
  }
  if (allSelectedFields().length >= MAX_CHANNELS) return;
  g.fieldNames = [...g.fieldNames, name];
  if (!rangeInputs.value[name]) {
    rangeInputs.value[name] = { min: "", max: "" };
  }
  graphGroups.value = [...graphGroups.value];
  syncGraphFields();
}

function removeField(name: string, graphId: string): void {
  const g = graphGroups.value.find((x) => x.id === graphId);
  if (!g) return;
  const idx = g.fieldNames.indexOf(name);
  if (idx < 0) return;
  g.fieldNames = g.fieldNames.filter((_, i) => i !== idx);
  graphGroups.value = [...graphGroups.value];
  syncGraphFields();
}

async function panTimeline(deltaSec: number): Promise<void> {
  const { tMin, tMax } = effectiveTimeWindow();
  setLocalViewport(tMin + deltaSec, tMax + deltaSec);
  await finishViewportGesture();
}

async function zoomTimeline(factor: number): Promise<void> {
  const { tMin, tMax } = effectiveTimeWindow();
  const span = Math.max(tMax - tMin, 1e-9);
  const center = (tMin + tMax) / 2;
  const newSpan = span / factor;
  setLocalViewport(center - newSpan / 2, center + newSpan / 2);
  await finishViewportGesture();
}

async function followLive(): Promise<void> {
  localViewport.value = null;
  await controlView({ followLive: true, spanSec: windowSeconds.value });
  await refreshTimelineStatus();
  await redrawNow();
}

function clearHistory(): void {
  void followLive();
}

const chartHover = ref(false);
const chartDragging = ref(false);
/** X курсора в координатах canvas (CSS px) для кроссхайра. */
const crosshairX = ref<number | null>(null);
const CLICK_PAN_THRESHOLD_PX = 5;

interface LocalViewport {
  tMin: number;
  tMax: number;
  spanSec: number;
  viewEndSec: number;
}

/** Локальное окно на время жеста — мгновенный repaint без IPC. */
const localViewport = ref<LocalViewport | null>(null);
let localRepaintRaf = 0;
let viewportSyncTimer = 0;
const VIEWPORT_SYNC_MS = 72;

let chartPointerDown = false;
let dragStartX = 0;
let dragStartY = 0;
let dragStartViewEnd = 0;
let dragStartTMin = 0;
let wheelRaf = 0;
let pendingWheelFactor: number | null = null;
let pendingWheelX = 0;

async function toggleFollowLive(): Promise<void> {
  localViewport.value = null;
  if (timelineStatus.value.followLive) {
    await controlView({ followLive: false, viewEndSec: timelineLiveSec() });
  } else {
    await controlView({ followLive: true, spanSec: timelineStatus.value.spanSec });
  }
  await refreshTimelineStatus();
  scheduleSaveLogUiToProject();
  await redrawNow();
}

function chartMargins() {
  let maxTraces = 1;
  const pv = effectiveSuggestPreviewField();
  for (const g of graphGroups.value) {
    let n = g.fieldNames.length;
    if (pv && g.id === activeGraphId.value && !g.fieldNames.includes(pv)) {
      n += 1;
    }
    if (n > maxTraces) maxTraces = n;
  }
  return logPanelMargins(maxTraces);
}

/** Ось времени: локальный viewport при жесте, иначе lastView / status. */
function effectiveTimeWindow(): LocalViewport {
  const lv = localViewport.value;
  if (lv) return lv;
  const st = timelineStatus.value;
  if (!st.followLive) {
    const view = lastView.value;
    if (view) {
      return {
        tMin: view.tMin,
        tMax: view.tMax,
        spanSec: Math.max(view.tMax - view.tMin, 0.001),
        viewEndSec: view.tMax,
      };
    }
  }
  let tMax = st.followLive ? timelineLiveSec() : st.viewEndSec;
  let tMin = tMax - st.spanSec;
  if (st.followLive && tMax < st.spanSec) {
    tMin = 0;
    tMax = st.spanSec;
  }
  return {
    tMin,
    tMax,
    spanSec: Math.max(tMax - tMin, 0.001),
    viewEndSec: tMax,
  };
}

function displayView(view: OutputTimelineView): OutputTimelineView {
  const lv = localViewport.value;
  if (!lv) return view;
  return { ...view, tMin: lv.tMin, tMax: lv.tMax, followLive: false };
}

function dataTimeBounds(): { dataMin: number; dataMax: number } {
  const st = timelineStatus.value;
  let dataMin = st.dataMinSec;
  let dataMax = Math.max(st.dataMaxSec, timelineLiveSec());
  for (const pts of seriesCache.values()) {
    if (pts.length === 0) continue;
    const lo = pts[0]!.t;
    const hi = pts[pts.length - 1]!.t;
    if (lo < dataMin) dataMin = lo;
    if (hi > dataMax) dataMax = hi;
  }
  return { dataMin, dataMax };
}

/** Как `clamp_view_end` + `query_view` t_min в output_timeline.rs. */
function clampViewportToData(tMin: number, tMax: number): { tMin: number; tMax: number } {
  const { dataMin, dataMax } = dataTimeBounds();
  const dataSpan = Math.max(dataMax - dataMin, 0.001);

  let span = Math.max(tMax - tMin, 0.001);
  if (span > dataSpan) span = dataSpan;
  span = Math.max(span, 0.5);

  const endLo = Math.min(dataMin + span, dataMax);
  const endHi = dataMax;

  let viewEnd = tMax;
  if (viewEnd < endLo) viewEnd = endLo;
  if (viewEnd > endHi) viewEnd = endHi;

  let viewMin = viewEnd - span;
  if (viewMin < dataMin) {
    viewMin = dataMin;
    viewEnd = Math.min(dataMin + span, dataMax);
  }

  return { tMin: viewMin, tMax: viewEnd };
}

function setLocalViewport(tMin: number, tMax: number): void {
  const clamped = clampViewportToData(tMin, tMax);
  const span = Math.max(clamped.tMax - clamped.tMin, 0.001);
  localViewport.value = {
    tMin: clamped.tMin,
    tMax: clamped.tMax,
    spanSec: span,
    viewEndSec: clamped.tMax,
  };
  scheduleLocalRepaint();
}

function scheduleLocalRepaint(): void {
  if (!tabActive.value) return;
  if (localRepaintRaf !== 0) return;
  localRepaintRaf = requestAnimationFrame(() => {
    localRepaintRaf = 0;
    void redraw(true);
  });
}

function scheduleViewSyncDebounced(): void {
  if (viewportSyncTimer !== 0) window.clearTimeout(viewportSyncTimer);
  viewportSyncTimer = window.setTimeout(() => {
    viewportSyncTimer = 0;
    void syncViewToRust(false);
  }, VIEWPORT_SYNC_MS);
}

async function syncViewToRust(finalize: boolean): Promise<void> {
  const vp = localViewport.value;
  if (!vp) return;
  await controlView({
    followLive: false,
    viewEndSec: vp.viewEndSec,
    spanSec: vp.spanSec,
  });
  if (finalize) {
    localViewport.value = null;
    scheduleLocalRepaint();
  }
}

async function finishViewportGesture(): Promise<void> {
  if (viewportSyncTimer !== 0) {
    window.clearTimeout(viewportSyncTimer);
    viewportSyncTimer = 0;
  }
  await syncViewToRust(true);
  scheduleSaveLogUiToProject();
}

function clientXToPlotTime(clientX: number): number | null {
  const wrap = canvasWrapRef.value;
  if (!wrap) return null;
  const x = clientX - wrap.getBoundingClientRect().left;
  const w = measureCanvasWidth();
  const { tMin, tMax } = effectiveTimeWindow();
  return plotXToTime(x, w, chartMargins(), tMin, tMax);
}

function applyLocalZoomAtPointer(clientX: number, zoomFactor: number): void {
  if (!canPlotTimeline()) return;

  const { tMin, tMax } = effectiveTimeWindow();
  const span = Math.max(tMax - tMin, 1e-9);
  const tCursor = clientXToPlotTime(clientX) ?? tMin + span * 0.5;
  const frac = Math.min(1, Math.max(0, (tCursor - tMin) / span));
  const newSpan = span / zoomFactor;
  const newEnd = tCursor + (1 - frac) * newSpan;
  setLocalViewport(newEnd - newSpan, newEnd);
  scheduleViewSyncDebounced();
}

function scheduleWheelZoom(clientX: number, factor: number): void {
  pendingWheelX = clientX;
  pendingWheelFactor = (pendingWheelFactor ?? 1) * factor;
  if (wheelRaf !== 0) return;
  wheelRaf = requestAnimationFrame(() => {
    wheelRaf = 0;
    const f = pendingWheelFactor ?? 1;
    const x = pendingWheelX;
    pendingWheelFactor = null;
    if (Math.abs(f - 1) > 1e-6) {
      applyLocalZoomAtPointer(x, f);
    }
  });
}

function onCanvasWheel(e: WheelEvent): void {
  if (!canPlotTimeline()) return;
  e.preventDefault();
  const step = wheelZoomFactor.value;
  const factor = e.deltaY < 0 ? step : 1 / step;
  scheduleWheelZoom(e.clientX, factor);
}

async function onChartPointerDown(e: PointerEvent): Promise<void> {
  if (!canPlotTimeline() || e.button !== 0) return;
  const wrap = canvasWrapRef.value;
  if (!wrap) return;
  wrap.setPointerCapture(e.pointerId);

  chartPointerDown = true;
  chartDragging.value = false;
  dragStartX = e.clientX;
  dragStartY = e.clientY;
  const win = effectiveTimeWindow();
  dragStartTMin = win.tMin;
  dragStartViewEnd = win.tMax;
  crosshairX.value = null;
  e.preventDefault();
}

async function startChartPanDrag(): Promise<void> {
  if (chartDragging.value) return;
  chartDragging.value = true;
  if (timelineStatus.value.followLive) {
    const end = timelineLiveSec();
    const span = timelineStatus.value.spanSec;
    setLocalViewport(end - span, end);
    void controlView({ followLive: false, viewEndSec: end });
    dragStartTMin = end - span;
    dragStartViewEnd = end;
  }
}

function onChartPointerMove(e: PointerEvent): void {
  if (chartPointerDown) {
    const dx = e.clientX - dragStartX;
    const dy = e.clientY - dragStartY;
    if (
      !chartDragging.value &&
      dx * dx + dy * dy > CLICK_PAN_THRESHOLD_PX * CLICK_PAN_THRESHOLD_PX
    ) {
      void startChartPanDrag();
    }
    if (chartDragging.value) {
      const w = measureCanvasWidth();
      const margins = chartMargins();
      const plotW = Math.max(1, w - margins.left - margins.right);
      const span = Math.max(dragStartViewEnd - dragStartTMin, 1e-9);
      const end = dragStartViewEnd - ((e.clientX - dragStartX) / plotW) * span;
      setLocalViewport(end - span, end);
      return;
    }
  }
  if (!chartHover.value || !canPlotTimeline()) return;
  updateCrosshairFromEvent(e);
  scheduleRedraw(true);
}

function onChartPointerLeave(_e: PointerEvent): void {
  chartHover.value = false;
  if (!chartDragging.value && !chartPointerDown) {
    crosshairX.value = null;
    scheduleRedraw(true);
  }
}

async function onChartPointerUp(e: PointerEvent): Promise<void> {
  if (!chartPointerDown) return;
  chartPointerDown = false;
  canvasWrapRef.value?.releasePointerCapture(e.pointerId);

  const dx = e.clientX - dragStartX;
  const dy = e.clientY - dragStartY;
  const isClick = dx * dx + dy * dy <= CLICK_PAN_THRESHOLD_PX * CLICK_PAN_THRESHOLD_PX;

  if (chartDragging.value) {
    chartDragging.value = false;
    await finishViewportGesture();
    return;
  }

  if (isClick && e.button === 0 && clientXToPlotTime(e.clientX) !== null) {
    applyLocalZoomAtPointer(e.clientX, zoomStepFactor.value);
    void finishViewportGesture();
  }
}

function onChartKeyDown(e: KeyboardEvent): void {
  if (e.code !== "Space" && e.key !== " ") return;
  const wrap = canvasWrapRef.value;
  if (!wrap) return;
  if (!chartHover.value && document.activeElement !== wrap) return;
  const tag = (e.target as HTMLElement | null)?.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
  e.preventDefault();
  if (e.repeat) return;
  void toggleFollowLive();
}

let resizeObserver: ResizeObserver | null = null;
let unlistenEcu: UnlistenFn | null = null;
let redrawGeneration = 0;
let redrawRaf = 0;
let redrawInflight: Promise<void> | null = null;

function currentViewState(): {
  viewEndSec: number;
  spanSec: number;
  followLive: boolean;
} {
  const lv = localViewport.value;
  const st = timelineStatus.value;
  if (lv) {
    return { viewEndSec: lv.viewEndSec, spanSec: lv.spanSec, followLive: false };
  }
  return {
    viewEndSec: st.viewEndSec,
    spanSec: st.spanSec,
    followLive: st.followLive,
  };
}

/** Только clamp в границы данных — span и позиция не сбрасываются. */
async function clampTimelineViewToData(dataMin: number, dataMax: number): Promise<void> {
  const { viewEndSec, spanSec, followLive } = currentViewState();
  if (!(dataMax > dataMin)) return;

  const span = Math.min(spanSec, Math.max(dataMax - dataMin, 0.5));
  const endLo = Math.min(dataMin + span, dataMax);
  const endHi = dataMax;

  let viewEnd = followLive ? endHi : viewEndSec;
  if (viewEnd < endLo) viewEnd = endLo;
  if (viewEnd > endHi) viewEnd = endHi;

  localViewport.value = null;
  await controlView({
    followLive,
    viewEndSec: viewEnd,
    spanSec: span,
  });
}

function invalidateSeriesCache(): void {
  seriesCache.clear();
}

function mergeSeriesChunk(chunk: {
  series: { field: string; points: { t: number; v: number }[] }[];
}): void {
  for (const s of chunk.series) {
    const prev = seriesCache.get(s.field) ?? [];
    const add = s.points.map((p) => ({ t: Number(p.t), v: Number(p.v) }));
    if (add.length === 0) continue;
    if (prev.length === 0 || add[0]!.t >= prev[prev.length - 1]!.t) {
      seriesCache.set(s.field, prev.concat(add));
    } else {
      seriesCache.set(
        s.field,
        prev.concat(add).sort((a, b) => a.t - b.t),
      );
    }
  }
}

function slicePointsForWindow(
  pts: { t: number; v: number }[],
  tMin: number,
  tMax: number,
): { t: number; v: number }[] {
  const out: { t: number; v: number }[] = [];
  for (const p of pts) {
    if (p.t >= tMin && p.t <= tMax) out.push(p);
  }
  return out;
}

async function streamSeriesIntoCache(reset: boolean): Promise<void> {
  const fields = fieldsForTimelineQuery();
  if (fields.length === 0) return;

  const gen = ++seriesStreamGen;
  if (reset) invalidateSeriesCache();

  if (seriesFetchInflight) {
    await seriesFetchInflight;
    if (gen !== seriesStreamGen) return;
  }

  seriesFetchInflight = (async () => {
    try {
      let hasMore = true;
      let first = reset;
      while (hasMore && gen === seriesStreamGen) {
        const chunk = await pullSeriesChunk(fields, first);
        first = false;
        if (gen !== seriesStreamGen) return;
        mergeSeriesChunk(chunk);
        scheduleRedraw(true);
        hasMore = chunk.hasMore;
        if (hasMore) {
          await new Promise<void>((r) => requestAnimationFrame(() => r()));
        }
      }
    } catch (err) {
      console.warn("[Log] series stream failed:", err);
    } finally {
      seriesFetchInflight = null;
    }
  })();
  await seriesFetchInflight;
}

async function pullLiveSeriesDelta(): Promise<void> {
  const fields = fieldsForTimelineQuery();
  if (fields.length === 0) return;
  if (timelineStatus.value.fileLoading) return;

  if (seriesFetchInflight) {
    await seriesFetchInflight;
  }

  seriesFetchInflight = (async () => {
    try {
      let hasMore = true;
      while (hasMore) {
        const chunk = await pullSeriesChunk(fields, false);
        if (chunk.series.length === 0 && !chunk.hasMore) break;
        mergeSeriesChunk(chunk);
        hasMore = chunk.hasMore;
      }
    } catch (err) {
      console.warn("[Log] live series pull failed:", err);
    } finally {
      seriesFetchInflight = null;
    }
  })();
  await seriesFetchInflight;
}

function buildViewFromCache(): OutputTimelineView | null {
  const win = effectiveTimeWindow();
  const fields = fieldsForTimelineQuery();
  const followLive = timelineStatus.value.followLive;
  const series: OutputTimelineView["series"] = [];
  for (const field of fields) {
    const pts = seriesCache.get(field);
    if (pts && pts.length > 0) {
      let windowPts = slicePointsForWindow(pts, win.tMin, win.tMax);
      if (windowPts.length === 0 && followLive) {
        windowPts = pts.filter((p) => p.t <= win.tMax + 1e-6);
      }
      if (windowPts.length > 0) {
        series.push({ field, points: windowPts });
      }
    }
  }
  return {
    tMin: win.tMin,
    tMax: win.tMax,
    liveSec: timelineLiveSec(),
    followLive,
    series,
  };
}

async function refreshSeriesCache(force = false): Promise<void> {
  if (force || seriesCache.size === 0) {
    await streamSeriesIntoCache(force || seriesCache.size === 0);
    return;
  }
  await pullLiveSeriesDelta();
}

function canPlotTimeline(): boolean {
  return (
    snapshot.value.connected ||
    timelineHasHistory.value ||
    Boolean(snapshot.value.sessionLogPath ?? timelineStatus.value.sessionLogPath)
  );
}

function crosshairSpec(): { x: number } | null {
  if (!chartHover.value || chartDragging.value || crosshairX.value === null) {
    return null;
  }
  return { x: crosshairX.value };
}

function paintFromView(
  w: number,
  h: number,
  view: OutputTimelineView,
): void {
  const dv = displayView(view);
  const { tMin, tMax } = dv;
  const crosshair = crosshairSpec();
  const fields = fieldsForTimelineQuery();
  const previewName = effectiveSuggestPreviewField();
  let panels = buildPanels(
    tMin,
    tMax,
    (name) => seriesForField(name, dv),
    previewName,
  );
  if (panels.length === 0 && fields.length > 0) {
    panels = buildPanels(
      tMin,
      tMax,
      (name) => fallbackSeries(name, tMin, tMax),
      previewName,
    );
  }
  if (panels.length === 0) {
    logChartRenderer.clear(w, h);
    chartOverlay.value = { crosshairX: null, labels: [], markers: [] };
    return;
  }
  logChartRenderer.paint({ width: w, height: h, panels, tMin, tMax, crosshair });
  chartOverlay.value = buildLogChartOverlay(w, h, panels, tMin, tMax, crosshair);
}

let redrawSkipFetch = false;

function scheduleRedraw(skipFetch = false): void {
  if (!tabActive.value) return;
  if (skipFetch) {
    redrawSkipFetch = true;
  } else {
    redrawSkipFetch = false;
  }
  if (redrawRaf !== 0) return;
  redrawRaf = requestAnimationFrame(() => {
    redrawRaf = 0;
    const skip = redrawSkipFetch;
    redrawSkipFetch = false;
    redrawInflight = redraw(skip).finally(() => {
      redrawInflight = null;
    });
  });
}

function updateCrosshairFromEvent(e: PointerEvent): void {
  const wrap = canvasWrapRef.value;
  if (!wrap) return;
  const rect = wrap.getBoundingClientRect();
  crosshairX.value = e.clientX - rect.left;
}

async function redraw(skipSeriesFetch = false): Promise<void> {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const w = measureCanvasWidth();
  canvasWidth.value = w;
  const h = canvasHeight.value;

  const fields = fieldsForTimelineQuery();
  if (fields.length === 0) {
    logChartRenderer.clear(w, h);
    chartOverlay.value = { crosshairX: null, labels: [], markers: [] };
    return;
  }

  if (!canPlotTimeline()) {
    logChartRenderer.clear(w, h);
    chartOverlay.value = { crosshairX: null, labels: [], markers: [] };
    return;
  }

  const gen = ++redrawGeneration;
  if (!skipSeriesFetch) {
    await refreshSeriesCache();
  }
  if (gen !== redrawGeneration) return;

  const view = buildViewFromCache();
  if (!view) return;

  lastView.value = view;
  paintFromView(w, h, view);
}

async function redrawNow(): Promise<void> {
  invalidateSeriesCache();
  redrawGeneration += 1;
  if (redrawInflight) {
    await redrawInflight;
  }
  redrawInflight = redraw().finally(() => {
    redrawInflight = null;
  });
  await redrawInflight;
}

onMounted(async () => {
  await initProject();
  await initOutputChannels();
  await initOutputTimeline();
  await refreshFieldCatalog();
  await applyLogUiFromProject();
  if (snapshot.value.connected || timelineHasHistory.value) {
    await streamSeriesIntoCache(true);
  }

  if (canvasRef.value) {
    logChartRenderer.attach(canvasRef.value);
  }

  unlistenEcu = await listen("ecu-connection", () => {
    invalidateSeriesCache();
    lastView.value = null;
    seriesStreamGen += 1;
    void refreshFieldCatalog();
    void refreshTimelineStatus().then(() => {
      void streamSeriesIntoCache(true).then(() => scheduleRedraw());
    });
  });

  if (canvasWrapRef.value) {
    resizeObserver = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) {
        const next = Math.max(200, Math.floor(entry.contentRect.width));
        if (next !== canvasWidth.value) {
          canvasWidth.value = next;
          scheduleRedraw();
        }
      }
    });
    resizeObserver.observe(canvasWrapRef.value);
    canvasWidth.value = measureCanvasWidth();
  }
  scheduleRedraw();
  window.addEventListener("keydown", onChartKeyDown);
});

onUnmounted(() => {
  if (saveLogUiTimer !== 0) window.clearTimeout(saveLogUiTimer);
  if (viewportSyncTimer !== 0) window.clearTimeout(viewportSyncTimer);
  if (localRepaintRaf !== 0) cancelAnimationFrame(localRepaintRaf);
  logChartRenderer.detach();
  resizeObserver?.disconnect();
  unlistenEcu?.();
  window.removeEventListener("keydown", onChartKeyDown);
  window.removeEventListener("scroll", updateSuggestPosition, true);
  window.removeEventListener("resize", updateSuggestPosition);
});

watch(showSuggest, (open) => {
  if (open) {
    updateSuggestPosition();
    window.addEventListener("scroll", updateSuggestPosition, true);
    window.addEventListener("resize", updateSuggestPosition);
  } else {
    suggestPreviewField.value = null;
    scheduleRedraw(true);
    window.removeEventListener("scroll", updateSuggestPosition, true);
    window.removeEventListener("resize", updateSuggestPosition);
  }
});

watch(
  () => snapshot.value.values,
  () => {
    if (uniquePollFields().length === 0) return;
    if (!canPlotTimeline()) return;
    void pullLiveSeriesDelta().then(() => scheduleRedraw(true));
  },
);

watch(
  () => timelineStatus.value.followLive,
  () => {
    lastView.value = null;
    scheduleRedraw(true);
  },
);

watch(seriesRevision, () => {
  void pullLiveSeriesDelta().then(() => scheduleRedraw(true));
});

watch(loadEpoch, () => {
  void runFileLoadStream();
});

watch(canvasHeight, () => scheduleRedraw());
watch(rangeInputs, () => scheduleRedraw(), { deep: true });
watch(graphGroups, () => scheduleRedraw(), { deep: true });
watch(chartHeight, () => scheduleRedraw());

watch(tabActive, (active, wasActive) => {
  if (active && !wasActive) scheduleRedraw(true);
});
</script>

<template>
  <div
    class="output-chart log-chart"
    :class="{ 'log-chart--compact': !settingsExpanded }"
  >
    <div class="log-chrome">
      <button
        type="button"
        class="log-setup-toggle"
        :aria-expanded="settingsExpanded"
        :title="settingsExpanded ? 'Свернуть настройки' : 'Развернуть настройки'"
        @click="toggleSettingsExpanded"
      >
        <span class="log-setup-chevron" :class="{ open: settingsExpanded }">▸</span>
        <span class="log-setup-toggle-label">
          {{ settingsExpanded ? "Свернуть" : "Настройки log" }}
        </span>
      </button>
      <button
        type="button"
        class="btn-load-log"
        :disabled="openingOutputLog"
        title="Открыть CSV-лог (output_*.csv)"
        @click="onOpenOutputLog"
      >{{ openingOutputLog ? '…' : 'Лог…' }}</button>
      <p v-if="openOutputLogError" class="load-log-error">{{ openOutputLogError }}</p>

      <div v-if="!settingsExpanded" class="log-compact-meta">
        <span class="log-compact-summary">{{ setupSummary }}</span>
        <span class="log-interact-hint">колёсико · drag · пробел</span>
        <div class="graph-tabs graph-tabs--inline" role="tablist">
          <button
            v-for="(g, i) in graphGroups"
            :key="g.id"
            type="button"
            role="tab"
            class="graph-tab graph-tab--mini"
            :class="{ active: activeGraphId === g.id }"
            :title="`Граф ${i + 1}`"
            @click="activeGraphId = g.id"
          >
            {{ i + 1 }}
            <span v-if="g.fieldNames.length" class="graph-tab-count">{{ g.fieldNames.length }}</span>
          </button>
        </div>
        <div class="timeline-nav" aria-label="Навигация по времени">
          <button type="button" class="btn-clear btn-clear--mini" title="Назад" @click="panTimeline(-timelineStatus.spanSec * 0.25)">◀</button>
          <button
            type="button"
            class="btn-clear btn-clear--mini"
            :class="{ active: timelineStatus.followLive }"
            title="Live вкл/выкл (пробел)"
            @click="toggleFollowLive"
          >
            ●
          </button>
          <button type="button" class="btn-clear btn-clear--mini" title="Вперёд" @click="panTimeline(timelineStatus.spanSec * 0.25)">▶</button>
          <label class="zoom-step" title="Шаг зума, % (колёсико и ±)">
            <input
              v-model.number="zoomStepPct"
              type="number"
              class="zoom-step-input"
              min="1"
              max="40"
              step="1"
              @change="onZoomStepChange"
            />
            <span class="zoom-step-suffix">%</span>
          </label>
          <button
            type="button"
            class="btn-clear btn-clear--mini"
            :title="`Уменьшить окно (${zoomStepPct}%)`"
            @click="zoomTimeline(zoomStepFactor)"
          >
            −
          </button>
          <button
            type="button"
            class="btn-clear btn-clear--mini"
            :title="`Увеличить окно (${zoomStepPct}%)`"
            @click="zoomTimeline(1 / zoomStepFactor)"
          >
            +
          </button>
          <label
            class="log-viewport-link"
            title="Trigger-график показывает то же окно elapsed_sec (пусто, если данных нет)"
          >
            <input
              type="checkbox"
              :checked="viewportLinked"
              @change="onViewportLinkChange(($event.target as HTMLInputElement).checked)"
            />
            ↔ Trigger
          </label>
        </div>
        <button type="button" class="btn-clear btn-clear--mini" title="Live вкл/выкл (пробел)" @click="toggleFollowLive">
          ↻
        </button>
      </div>
    </div>

    <div v-show="settingsExpanded" class="log-setup">
    <div class="graph-tabs">
      <button
        v-for="(g, i) in graphGroups"
        :key="g.id"
        type="button"
        class="graph-tab"
        :class="{ active: activeGraphId === g.id }"
        @click="activeGraphId = g.id"
      >
        Граф {{ i + 1 }}
        <span v-if="g.fieldNames.length" class="graph-tab-count">{{ g.fieldNames.length }}</span>
      </button>
      <button
        type="button"
        class="graph-tab graph-tab-add"
        :disabled="graphGroups.length >= MAX_GRAPHS"
        title="Добавить график"
        @click="addGraph"
      >
        +
      </button>
      <button
        v-if="graphGroups.length > 1"
        type="button"
        class="graph-tab graph-tab-remove"
        title="Удалить активный график"
        @click="removeGraph(activeGraphId)"
      >
        −
      </button>
      <span class="graph-tabs-hint">Каналы добавляются на активный граф (один параметр — на нескольких)</span>
    </div>

    <div class="toolbar">
      <div class="field-picker">
        <label class="picker-label" for="chart-field-filter">Каналы log</label>
        <input
          id="chart-field-filter"
          ref="searchInputRef"
          v-model="fieldFilter"
          type="search"
          class="field-search"
          placeholder="Поиск по имени…"
          autocomplete="off"
          @focus="openSuggest"
          @blur="closeSuggestSoon"
          @input="updateSuggestPosition"
        />
        <Teleport to="body">
          <ul
            v-if="showSuggest"
            ref="suggestListRef"
            class="field-suggest field-suggest-portal"
            :style="suggestStyle"
            @mouseleave="clearSuggestPreview"
          >
            <li v-if="suggestEmptyHint" class="field-suggest-empty">
              {{ suggestEmptyHint }}
            </li>
            <li
              v-for="f in filteredFields"
              :key="f.name"
              :class="{
                active: isFieldOnActiveGraph(f.name),
                'on-other-graph': isFieldOnAnyGraph(f.name) && !isFieldOnActiveGraph(f.name),
                'suggest-preview-hint': suggestPreviewField === f.name,
              }"
            >
              <button
                type="button"
                @mousedown.prevent="onSuggestPick(f.name)"
                @pointerenter="setSuggestPreview(f.name)"
                @focus="setSuggestPreview(f.name)"
                @blur="onSuggestItemBlur"
              >
                {{ f.name }}
                <span v-if="f.units" class="units">{{ f.units }}</span>
              </button>
            </li>
          </ul>
        </Teleport>
      </div>

      <div class="selected-fields">
        <span
          v-for="item in legendItems"
          :key="item.slotKey"
          class="chip"
          :style="{ borderColor: item.color }"
        >
          <span class="chip-dot" :style="{ background: item.color }" />
          <span class="chip-graph">{{ item.graphLabel }}</span>
          <span class="chip-name">{{ item.name }}</span>
          <span v-if="item.value !== null" class="chip-val">
            {{ Number.isInteger(item.value) ? item.value : item.value.toFixed(2) }}
            <span v-if="item.units" class="chip-units">{{ item.units }}</span>
          </span>
          <button
            type="button"
            class="chip-remove"
            title="Убрать с этого графа"
            @click="removeField(item.name, item.graphId)"
          >
            ×
          </button>
        </span>
      </div>

      <div class="toolbar-actions">
        <span class="window-hint">окно {{ windowSeconds }} с · автопромотка</span>
        <label class="zoom-step zoom-step--wide" title="Шаг зума, % (колёсико и ±)">
          <span class="zoom-step-label">Шаг зума</span>
          <input
            v-model.number="zoomStepPct"
            type="range"
            class="zoom-step-range"
            min="1"
            max="40"
            step="1"
            @change="onZoomStepChange"
          />
          <input
            v-model.number="zoomStepPct"
            type="number"
            class="zoom-step-input"
            min="1"
            max="40"
            step="1"
            @change="onZoomStepChange"
          />
          <span class="zoom-step-suffix">%</span>
        </label>
        <button type="button" class="btn-clear" @click="clearHistory">Сброс</button>
      </div>
    </div>

    <div v-if="channelRows.length" class="channel-ranges">
      <p class="ranges-title">Диапазон Y · min / max (пусто = авто по окну)</p>
      <div class="ranges-grid">
        <div v-for="row in channelRows" :key="row.slotKey" class="range-row">
          <span class="range-dot" :style="{ background: row.color }" />
          <span class="range-name">{{ row.name }}</span>
          <label class="range-graph">
            <span>граф</span>
            <select
              class="range-select"
              :value="row.graphId"
              @change="
                moveFieldToGraph(
                  row.name,
                  row.graphId,
                  ($event.target as HTMLSelectElement).value,
                )
              "
            >
              <option v-for="(g, i) in graphGroups" :key="g.id" :value="g.id">
                {{ i + 1 }}
              </option>
            </select>
          </label>
          <label class="range-field">
            <span>min</span>
            <input
              type="number"
              class="range-input"
              :value="row.min"
              placeholder="авто"
              step="any"
              @input="setRangeMin(row.name, ($event.target as HTMLInputElement).value)"
            />
          </label>
          <label class="range-field">
            <span>max</span>
            <input
              type="number"
              class="range-input"
              :value="row.max"
              placeholder="авто"
              step="any"
              @input="setRangeMax(row.name, ($event.target as HTMLInputElement).value)"
            />
          </label>
        </div>
      </div>
    </div>
    </div>

    <div
      class="canvas-wrap"
      ref="canvasWrapRef"
      tabindex="0"
      :class="{ 'canvas-wrap--dragging': chartDragging, 'canvas-wrap--live': timelineStatus.followLive }"
      title="Клик — зум в точку, колёсико — масштаб, перетаскивание — время, пробел — live"
      @pointerenter="chartHover = true"
      @pointerleave="onChartPointerLeave"
      @pointerdown="onChartPointerDown"
      @pointermove="onChartPointerMove"
      @pointerup="onChartPointerUp"
      @pointercancel="onChartPointerUp"
      @wheel.prevent="onCanvasWheel"
    >
      <canvas ref="canvasRef" class="chart-canvas" />
      <div class="log-chart-overlay" aria-hidden="true">
        <div
          v-if="chartOverlay.crosshairX !== null"
          class="log-crosshair-v"
          :style="{ left: `${chartOverlay.crosshairX}px` }"
        />
        <span
          v-for="(lb, i) in chartOverlay.labels"
          :key="`lb-${i}`"
          class="log-ol-label"
          :style="labelStyle(lb)"
        >{{ lb.text }}</span>
        <template v-for="(mk, i) in chartOverlay.markers" :key="`mk-${i}`">
          <span class="log-crosshair-dot" :style="{ left: `${mk.x}px`, top: `${mk.y}px`, borderColor: mk.color, background: mk.color }" />
          <span class="log-crosshair-tag" :style="markerLabelStyle(mk, canvasWidth)">{{ mk.label }}</span>
        </template>
      </div>
      <p v-if="!hasAnyChannel" class="overlay-hint">
        Выберите параметры через поиск — они попадут на активный граф
      </p>
      <p v-else-if="!canPlotTimeline()" class="overlay-hint">
        Подключите ECU или откройте CSV-лог сессии
      </p>
    </div>

    <p v-if="snapshot.lastError" class="error">{{ snapshot.lastError }}</p>
  </div>
</template>

<style scoped>
.output-chart {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
  width: 100%;
  max-width: 100%;
  min-width: 0;
  box-sizing: border-box;
}

.output-chart.log-chart--compact {
  gap: 0.2rem;
}

.log-chrome {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.35rem 0.5rem;
  min-height: 1.6rem;
}

.log-setup-toggle {
  display: inline-flex;
  align-items: center;
  gap: 0.3rem;
  padding: 0.15rem 0.4rem;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-muted);
  font-size: 0.75rem;
  font-weight: 500;
  cursor: pointer;
}

.log-setup-toggle:hover {
  color: var(--color-text);
  background: var(--color-bg-muted);
}

.log-setup-chevron {
  display: inline-block;
  font-size: 0.65rem;
  transition: transform 0.15s ease;
  transform: rotate(0deg);
}

.log-setup-chevron.open {
  transform: rotate(90deg);
}

.timeline-nav {
  display: inline-flex;
  gap: 0.15rem;
  align-items: center;
}

.log-viewport-link {
  display: inline-flex;
  align-items: center;
  gap: 0.2rem;
  margin-left: 0.25rem;
  font-size: 0.72rem;
  color: var(--color-text-subtle);
  cursor: pointer;
  user-select: none;
}

.zoom-step {
  display: inline-flex;
  align-items: center;
  gap: 0.2rem;
  font-size: 0.72rem;
  color: var(--color-text-subtle);
}

.zoom-step--wide {
  flex-wrap: wrap;
  justify-content: flex-end;
  max-width: 14rem;
}

.zoom-step-label {
  width: 100%;
  font-size: 0.72rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-gray);
}

.zoom-step-input {
  width: 2.75rem;
  padding: 0.15rem 0.25rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-text);
  font-size: 0.78rem;
  text-align: right;
}

.zoom-step-range {
  flex: 1 1 5rem;
  min-width: 4rem;
  accent-color: var(--color-accent);
}

.zoom-step-suffix {
  font-size: 0.72rem;
}

.btn-clear--mini.active {
  color: var(--color-accent);
  border-color: var(--color-accent);
}

.log-compact-meta {
  display: flex;
  flex: 1;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.35rem 0.5rem;
  min-width: 0;
}

.log-compact-summary {
  font-size: 0.72rem;
  color: var(--color-text-subtle);
  white-space: nowrap;
}

.log-interact-hint {
  font-size: 0.68rem;
  color: var(--color-text-subtle);
  white-space: nowrap;
  opacity: 0.85;
}

.graph-tabs--inline {
  flex: 0 0 auto;
  gap: 0.2rem;
}

.graph-tab--mini {
  min-width: 1.5rem;
  padding: 0.12rem 0.35rem;
  font-size: 0.72rem;
}

.btn-clear--mini {
  margin-left: auto;
  padding: 0.12rem 0.4rem;
  font-size: 0.85rem;
  line-height: 1;
}

.log-setup {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}

.graph-tabs {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.35rem;
}

.graph-tab {
  display: inline-flex;
  align-items: center;
  gap: 0.3rem;
  padding: 0.28rem 0.55rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-text-muted);
  font-size: 0.78rem;
  cursor: pointer;
}

.graph-tab.active {
  border-color: var(--color-accent);
  color: var(--color-text);
  background: var(--color-bg-accent-soft);
}

.graph-tab-count {
  font-size: 0.68rem;
  opacity: 0.75;
}

.graph-tab-add,
.graph-tab-remove {
  min-width: 1.75rem;
  justify-content: center;
  font-weight: 600;
}

.graph-tab:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.graph-tabs-hint {
  font-size: 0.72rem;
  color: var(--color-text-subtle);
  margin-left: 0.25rem;
}

.toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 0.65rem;
  align-items: flex-start;
}

.field-picker {
  position: relative;
  flex: 1 1 14rem;
  min-width: 12rem;
}

.picker-label {
  display: block;
  font-size: 0.72rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-gray);
  margin-bottom: 0.3rem;
  font-weight: 500;
}

.field-search {
  width: 100%;
  padding: 0.45rem 0.6rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-text);
}

.field-suggest-portal {
  position: fixed;
  z-index: 10050;
  max-height: min(280px, 40vh);
  overflow: auto;
  margin: 0;
  padding: 0.25rem 0;
  list-style: none;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-card);
}

.field-suggest-empty {
  padding: 0.5rem 0.65rem;
  font-size: 0.82rem;
  color: var(--color-text-subtle);
}

.field-suggest-portal li.active button {
  background: var(--color-bg-accent-soft);
}

.field-suggest-portal li.on-other-graph button {
  border-left: 2px solid var(--color-accent);
}

.field-suggest-portal li.suggest-preview-hint button {
  outline: 1px solid var(--color-border-strong);
  background: var(--color-bg-muted);
}

.field-suggest-portal button {
  display: flex;
  width: 100%;
  gap: 0.5rem;
  justify-content: space-between;
  padding: 0.35rem 0.65rem;
  border: none;
  background: transparent;
  color: var(--color-text);
  text-align: left;
  font-size: 0.85rem;
  cursor: pointer;
}

.field-suggest-portal button:hover {
  background: var(--color-bg-muted);
}

.field-suggest-portal .units {
  color: var(--color-text-subtle);
  font-size: 0.78rem;
}

.selected-fields {
  display: flex;
  flex-wrap: wrap;
  gap: 0.35rem;
  flex: 2 1 20rem;
  align-content: flex-start;
  padding-top: 0;
}

.log-chart--compact .selected-fields {
  padding-top: 0;
}

.channel-ranges {
  padding: 0.65rem 0.75rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: var(--color-bg-muted);
}

.ranges-title {
  margin: 0 0 0.5rem;
  font-size: 0.72rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-gray);
  font-weight: 500;
}

.ranges-grid {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}

.range-row {
  display: grid;
  grid-template-columns: auto 1fr auto auto auto auto;
  gap: 0.5rem 0.75rem;
  align-items: center;
}

.range-graph {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  font-size: 0.72rem;
  color: var(--color-text-subtle);
}

.range-select {
  width: 3rem;
  padding: 0.25rem 0.3rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-text);
  font-size: 0.82rem;
}

.chip-graph {
  font-size: 0.68rem;
  color: var(--color-text-subtle);
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.range-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

.range-name {
  font-size: 0.82rem;
  font-weight: 500;
  font-family: ui-monospace, monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.range-field {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  font-size: 0.72rem;
  color: var(--color-text-subtle);
}

.range-input {
  width: 5.5rem;
  padding: 0.25rem 0.4rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-text);
  font-size: 0.82rem;
}

.range-input::placeholder {
  color: var(--color-text-subtle);
  opacity: 0.7;
}

@media (max-width: 640px) {
  .range-row {
    grid-template-columns: auto 1fr;
    grid-template-rows: auto auto;
  }

  .range-name {
    grid-column: 2;
  }

  .range-field {
    grid-column: 2;
  }
}

.chip {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  padding: 0.2rem 0.45rem 0.2rem 0.35rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border);
  background: var(--color-bg-muted);
  font-size: 0.78rem;
}

.chip-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}

.chip-name {
  font-weight: 500;
  color: var(--color-text);
}

.chip-val {
  font-variant-numeric: tabular-nums;
  color: var(--color-text-muted);
}

.chip-units {
  opacity: 0.85;
}

.chip-remove {
  border: none;
  background: transparent;
  color: var(--color-text-subtle);
  cursor: pointer;
  padding: 0 0.15rem;
  font-size: 1rem;
  line-height: 1;
}

.chip-remove:hover {
  color: var(--color-error);
}

.toolbar-actions {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 0.25rem;
  padding-top: 0;
}

.window-hint {
  font-size: 0.72rem;
  color: var(--color-text-subtle);
  white-space: nowrap;
}

.btn-clear {
  padding: 0.3rem 0.65rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-gray);
  font-size: 0.78rem;
  cursor: pointer;
}

.btn-clear:hover {
  background: var(--color-bg-muted);
}

.btn-load-log {
  padding: 0.2rem 0.6rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-gray);
  font-size: 0.78rem;
  cursor: pointer;
  white-space: nowrap;
  flex-shrink: 0;
}

.btn-load-log:hover:not(:disabled) {
  background: var(--color-bg-muted);
  color: var(--color-fg);
}

.btn-load-log:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.load-log-error {
  font-size: 0.75rem;
  color: var(--color-warning, #d97706);
  margin: 0;
  align-self: center;
}

.canvas-wrap:not(.canvas-wrap--dragging) {
  cursor: zoom-in;
}

.canvas-wrap {
  position: relative;
  width: 100%;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: var(--color-bg-elevated);
  overflow: hidden;
  cursor: grab;
  touch-action: none;
}

.canvas-wrap--dragging {
  cursor: grabbing;
}

.canvas-wrap--live {
  border-color: color-mix(in srgb, var(--color-accent) 45%, var(--color-border));
}

.chart-canvas {
  display: block;
  width: 100%;
  pointer-events: none;
}

.log-chart-overlay {
  position: absolute;
  inset: 0;
  pointer-events: none;
  overflow: hidden;
}

.log-ol-label {
  position: absolute;
  line-height: 1;
  white-space: nowrap;
}

.log-ol-label:not([style*="11px"]) {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}

.log-ol-label[style*="11px"] {
  font-family: "Segoe UI", system-ui, sans-serif;
}

.log-crosshair-v {
  position: absolute;
  top: 1px;
  bottom: 2px;
  width: 0;
  border-left: 1px dashed color-mix(in srgb, var(--color-accent) 45%, transparent);
  pointer-events: none;
}

.log-crosshair-dot {
  position: absolute;
  width: 8px;
  height: 8px;
  margin: -4px 0 0 -4px;
  border-radius: 50%;
  border: 1.5px solid var(--color-bg-elevated);
  box-sizing: border-box;
}

.log-crosshair-tag {
  position: absolute;
  transform: translateY(-50%);
  padding: 2px 4px;
  font: 600 10px "Segoe UI", system-ui, sans-serif;
  line-height: 1;
  white-space: nowrap;
  background: color-mix(in srgb, var(--color-bg-elevated) 92%, transparent);
  border: 1px solid;
  border-radius: 2px;
}

.overlay-hint {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  margin: 0;
  font-size: 0.9rem;
  color: var(--color-text-subtle);
  pointer-events: none;
  background: rgba(250, 247, 242, 0.55);
}

.error {
  margin: 0;
  font-size: 0.82rem;
  color: var(--color-error);
}
</style>
