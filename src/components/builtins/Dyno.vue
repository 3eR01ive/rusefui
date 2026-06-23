<script setup lang="ts">
import {
  computed,
  nextTick,
  onMounted,
  onUnmounted,
  reactive,
  ref,
  shallowRef,
  watch,
} from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useDataContext } from "../../core/data-context";
import { initOutputChannels, useOutputChannels } from "../../composables/useOutputChannels";
import { initConfig, useConfig } from "../../composables/useConfig";
import type { DynoRunPoint } from "../../composables/dynoTypes";
import {
  buildDynoChartOverlay,
  buildDynoCrosshairMarkers,
  dynoCrosshairMarkerStyle,
  dynoLayoutSignature,
  type DynoChartOverlay,
} from "../../composables/dynoChartOverlay";
import { dynoChartRenderer } from "../../composables/dynoChartRenderer";
import type { DynoCrosshairSpec, DynoChartLayout, DynoAxisRange } from "../../composables/dynoChartLayout";
import { DEFAULT_DYNO_AXIS, normalizeDynoAxisRange } from "../../composables/dynoChartLayout";
import { clampSmoothStrength, smoothDynoPoints } from "../../composables/smoothDynoCurve";
import {
  initProject,
  PERSIST_KEY_DYNO,
  projectUiEpoch,
  registerProjectUiFlushHook,
  useProject,
  workspaceResetEpoch,
  type DynoUiSettings,
} from "../../composables/useProject";
import { useRustComponent, type ComponentViewState } from "../../composables/useRustComponent";
import { useInstanceBind } from "../../composables/useInstanceBind";
import { useTabActivity, useTabFrozenDisplay } from "../../composables/useTabActivity";
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

const SMOOTH_MAX = 20;
const CHART_HEIGHT_MIN = 180;
const CHART_HEIGHT_MAX = 720;

const chartSizeOverride = { height: null as number | null };

const chartHeight = computed(() => {
  if (chartSizeOverride.height !== null && chartSizeOverride.height > CHART_HEIGHT_MIN) {
    return chartSizeOverride.height;
  }
  const h = Number(props.props.height ?? 360);
  return h >= CHART_HEIGHT_MIN ? h : 360;
});

const { state, dispatch, error, hasLogic, ready, mounting } = useRustComponent(
  props.instance,
  props.path,
);
const instanceRef = computed(() => props.instance);
const { paramStringOr, source: bindSource } = useInstanceBind(instanceRef);
const dataCtx = useDataContext();
const { snapshot } = useOutputChannels();
const { snapshot: configSnapshot } = useConfig();
const { isActive: tabActive } = useTabActivity();
const { getProjectUi, setProjectUi } = useProject();

let applyingProjectUi = false;
let saveDynoUiTimer = 0;
let channelsConfigured = false;

const rpmField = computed(() =>
  String(state.value.rpmField ?? paramStringOr("rpmField", "RPMValue")),
);
const tpsField = computed(() =>
  String(state.value.tpsField ?? paramStringOr("tpsField", "TPSValue")),
);

watch(ready, (isReady) => {
  if (!isReady || channelsConfigured) return;
  if (bindSource.value && bindSource.value !== "outputChannels") {
    console.warn(
      `[dyno] bind.source должен быть outputChannels (каналы RPM/TPS), получен ${bindSource.value}`,
    );
  }
  const rpm = paramStringOr("rpmField", "RPMValue");
  const tps = paramStringOr("tpsField", "TPSValue");
  channelsConfigured = true;
  void dispatch("set_channels", { rpmField: rpm, tpsField: tps });
});

const liveRpm = useTabFrozenDisplay(
  () => snapshot.value.values[rpmField.value] ?? null,
  null as number | null,
);
const liveTps = useTabFrozenDisplay(
  () => snapshot.value.values[tpsField.value] ?? null,
  null as number | null,
);

const recording = computed(() => Boolean(state.value.recording));
const localRunPoints = shallowRef<DynoRunPoint[]>([]);
const localPreviousRunPoints = shallowRef<DynoRunPoint[]>([]);
const currentTorque = computed(() => Number(state.value.currentTorque ?? 0));
const currentHp = computed(() => Number(state.value.currentHp ?? 0));
const message = computed(() => (state.value.message as string) ?? null);

function isDynoDelta(s: ComponentViewState): boolean {
  return s.dynoDelta === true;
}

function syncFullDynoState(s: ComponentViewState): void {
  if (Array.isArray(s.runPoints)) {
    localRunPoints.value = s.runPoints as DynoRunPoint[];
  }
  if (Array.isArray(s.previousRunPoints)) {
    localPreviousRunPoints.value = s.previousRunPoints as DynoRunPoint[];
  }
}

function applyDynoDelta(s: ComponentViewState): void {
  const len = Number(s.runPointsLen ?? 0);
  const pt = s.lastRunPoint as DynoRunPoint | undefined;
  if (!pt || len < 1) return;

  const curLen = localRunPoints.value.length;
  if (len === 1 && curLen > 1) {
    localRunPoints.value = [pt];
    return;
  }
  if (len < curLen) {
    localRunPoints.value = localRunPoints.value.slice(0, len);
  }
  if (len === localRunPoints.value.length + 1) {
    localRunPoints.value = [...localRunPoints.value, pt];
  }
}

const chartPoints = shallowRef<DynoRunPoint[]>([]);
const chartPreviousPoints = shallowRef<DynoRunPoint[]>([]);

function syncChartPointBuffers(): void {
  const strength = smoothStrength.value;
  chartPoints.value =
    strength > 0
      ? smoothDynoPoints(localRunPoints.value, strength)
      : localRunPoints.value;
  chartPreviousPoints.value =
    strength > 0
      ? smoothDynoPoints(localPreviousRunPoints.value, strength)
      : localPreviousRunPoints.value;
}

watch(state, (s) => {
  if (isDynoDelta(s)) {
    applyDynoDelta(s);
  } else {
    syncFullDynoState(s);
  }
  if (!tabActive.value) return;
  syncChartPointBuffers();
  scheduleRedraw();
});

watch(localRunPoints, () => {
  if (!tabActive.value || !recording.value) return;
  syncChartPointBuffers();
  scheduleRedraw();
});

watch(recording, (rec, wasRec) => {
  if (rec && !wasRec) {
    dynoChartRenderer.resetLiveCache();
    cachedOverlaySig = "";
  }
  if (!tabActive.value) return;
  syncChartPointBuffers();
  scheduleRedraw();
});

const ignoreTpsMin = ref(false);
const minRpm = ref(0);
const smoothStrength = ref(0);
const settingsOpen = ref(false);
const chartRpmMin = ref(DEFAULT_DYNO_AXIS.rpmMin);
const chartRpmMax = ref(DEFAULT_DYNO_AXIS.rpmMax);
const chartNmMin = ref(DEFAULT_DYNO_AXIS.nmMin);
const chartNmMax = ref(DEFAULT_DYNO_AXIS.nmMax);
const chartHpMin = ref(DEFAULT_DYNO_AXIS.hpMin);
const chartHpMax = ref(DEFAULT_DYNO_AXIS.hpMax);

// ---- Параметры расчёта (в настройках компонента; настройки MCU игнорируем) ----
const DYNO_PARAM_DEFAULTS = {
  dynoRpmStep: 100,
  dynoSaeTemperatureC: 20,
  dynoSaeRelativeHumidity: 80,
  dynoSaeBaro: 101.33,
  dynoCarWheelDiaInch: 18,
  dynoCarWheelAspectRatio: 55,
  dynoCarWheelTireWidthMm: 180,
  dynoCarGearPrimaryReduction: 1.0,
  dynoCarGearRatio: 1.0,
  dynoCarGearFinalDrive: 3.5,
  dynoCarCarMassKg: 1200,
  dynoCarCargoMassKg: 80,
  dynoCarCoeffOfDrag: 0.32,
  dynoCarFrontalAreaM2: 2.2,
} as const;
type DynoParamKey = keyof typeof DYNO_PARAM_DEFAULTS;
const DYNO_PARAM_KEYS = Object.keys(DYNO_PARAM_DEFAULTS) as DynoParamKey[];
const carParams = reactive<Record<DynoParamKey, number>>({ ...DYNO_PARAM_DEFAULTS });

/** Отправить параметры расчёта в Rust-логику дино. */
async function pushDynoConfig(): Promise<void> {
  if (!ready.value) return;
  await dispatch("set_dyno_config", { ...carParams });
}

const chartAxes = computed((): DynoAxisRange =>
  normalizeDynoAxisRange(
    {
      rpmMin: chartRpmMin.value,
      rpmMax: chartRpmMax.value,
      nmMin: chartNmMin.value,
      nmMax: chartNmMax.value,
      hpMin: chartHpMin.value,
      hpMax: chartHpMax.value,
    },
    true,
  ),
);

const connected = computed(
  () => Boolean(state.value.connected ?? dataCtx.connection.value.connected),
);
const configLoaded = computed(
  () => Boolean(state.value.configLoaded ?? configSnapshot.value.loaded),
);

const peakTorque = computed(() =>
  chartPoints.value.reduce((m, p) => Math.max(m, p.torqueNm), 0),
);
const peakHp = computed(() => chartPoints.value.reduce((m, p) => Math.max(m, p.hp), 0));

const showRunPeaks = computed(
  () => !recording.value && localRunPoints.value.length > 0,
);

const displayTorque = computed(() =>
  showRunPeaks.value ? peakTorque.value : currentTorque.value,
);
const displayHp = computed(() => (showRunPeaks.value ? peakHp.value : currentHp.value));

const canToggleRecord = computed(() => {
  if (recording.value) return true;
  return (
    ready.value &&
    connected.value &&
    configLoaded.value &&
    hasLogic.value
  );
});
const canClear = computed(
  () =>
    !recording.value &&
    (localRunPoints.value.length > 0 || localPreviousRunPoints.value.length > 0),
);

const statusMode = computed(() => {
  if (recording.value) return "recording";
  if (!connected.value) return "offline";
  if (!configLoaded.value) return "noconfig";
  if (localRunPoints.value.length > 0) return "done";
  return "idle";
});

const statusLabel = computed(() => {
  switch (statusMode.value) {
    case "recording":
      return "Запись";
    case "offline":
      return "Нет ECU";
    case "noconfig":
      return "Нет config";
    case "done":
      return "Есть прогон";
    default:
      return "Готов";
  }
});

const smoothPct = computed(() => (smoothStrength.value / SMOOTH_MAX) * 100);
const smoothTrackRef = ref<HTMLElement | null>(null);
const smoothDisabled = computed(() => localRunPoints.value.length < 3);

function smoothFromClientX(clientX: number): number {
  const el = smoothTrackRef.value;
  if (!el) return smoothStrength.value;
  const rect = el.getBoundingClientRect();
  if (rect.width <= 0) return smoothStrength.value;
  const t = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
  return clampSmoothStrength(Math.round(t * SMOOTH_MAX));
}

function onSmoothTrackPointerDown(event: MouseEvent): void {
  if (smoothDisabled.value) return;
  event.preventDefault();
  smoothStrength.value = smoothFromClientX(event.clientX);
  const onMove = (ev: MouseEvent) => {
    smoothStrength.value = smoothFromClientX(ev.clientX);
  };
  const onUp = () => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
    scheduleSaveDynoUiToProject();
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
}

function onSmoothTrackKeydown(event: KeyboardEvent): void {
  if (smoothDisabled.value) return;
  if (event.key === "ArrowRight" || event.key === "ArrowUp") {
    smoothStrength.value = clampSmoothStrength(smoothStrength.value + 1);
  } else if (event.key === "ArrowLeft" || event.key === "ArrowDown") {
    smoothStrength.value = clampSmoothStrength(smoothStrength.value - 1);
  } else {
    return;
  }
  event.preventDefault();
  scheduleSaveDynoUiToProject();
}

function toggleRecording(): void {
  if (recording.value) {
    void dispatch("stop_run");
  } else {
    void dispatch("start_run").then((next) => {
      if (!next) return;
      syncFullDynoState(next);
      dynoChartRenderer.resetLiveCache();
      cachedOverlaySig = "";
      if (!tabActive.value) return;
      syncChartPointBuffers();
      scheduleRedraw();
    });
  }
}

function clearRun() {
  return dispatch("clear");
}

function buildDynoUiSettings(): DynoUiSettings {
  const axes = normalizeDynoAxisRange({
    rpmMin: chartRpmMin.value,
    rpmMax: chartRpmMax.value,
    nmMin: chartNmMin.value,
    nmMax: chartNmMax.value,
    hpMin: chartHpMin.value,
    hpMax: chartHpMax.value,
  });
  return {
    ignoreTpsMin: ignoreTpsMin.value,
    minRpm: Math.max(0, Math.round(minRpm.value)),
    smoothStrength: clampSmoothStrength(smoothStrength.value),
    chartHeight: chartHeight.value,
    settingsOpen: settingsOpen.value,
    chartRpmMin: axes.rpmMin,
    chartRpmMax: axes.rpmMax,
    chartNmMin: axes.nmMin,
    chartNmMax: axes.nmMax,
    chartHpMin: axes.hpMin,
    chartHpMax: axes.hpMax,
    ...carParams,
  };
}

async function syncOptionsToRust(): Promise<void> {
  if (!ready.value) return;
  await dispatch("set_options", {
    ignoreTpsMin: ignoreTpsMin.value,
    minRpm: Math.max(0, Math.round(minRpm.value)),
  });
}

async function applyDynoUiFromProject(opts?: { reloadPanelState?: boolean }): Promise<void> {
  if (!tabActive.value) return;
  applyingProjectUi = true;
  try {
    const ui = await getProjectUi<DynoUiSettings>(PERSIST_KEY_DYNO);
    ignoreTpsMin.value = ui.ignoreTpsMin;
    minRpm.value = ui.minRpm;
    smoothStrength.value = clampSmoothStrength(ui.smoothStrength);
    chartSizeOverride.height = ui.chartHeight > CHART_HEIGHT_MIN ? ui.chartHeight : null;
    if (opts?.reloadPanelState !== false) {
      settingsOpen.value = ui.settingsOpen;
    }
    const axes = normalizeDynoAxisRange({
      rpmMin: ui.chartRpmMin,
      rpmMax: ui.chartRpmMax,
      nmMin: ui.chartNmMin,
      nmMax: ui.chartNmMax,
      hpMin: ui.chartHpMin,
      hpMax: ui.chartHpMax,
    });
    chartRpmMin.value = axes.rpmMin;
    chartRpmMax.value = axes.rpmMax;
    chartNmMin.value = axes.nmMin;
    chartNmMax.value = axes.nmMax;
    chartHpMin.value = axes.hpMin;
    chartHpMax.value = axes.hpMax;
    for (const k of DYNO_PARAM_KEYS) {
      const v = ui[k];
      carParams[k] = Number.isFinite(v) ? Number(v) : DYNO_PARAM_DEFAULTS[k];
    }
  } catch {
    ignoreTpsMin.value = Boolean(state.value.ignoreTpsMin);
    minRpm.value = Number(state.value.minRpm ?? 0);
  } finally {
    applyingProjectUi = false;
  }
  await syncOptionsToRust();
  await pushDynoConfig();
  scheduleRedraw();
}

async function flushDynoUiToProject(): Promise<void> {
  if (saveDynoUiTimer !== 0) {
    window.clearTimeout(saveDynoUiTimer);
    saveDynoUiTimer = 0;
  }
  if (applyingProjectUi) return;
  await setProjectUi(PERSIST_KEY_DYNO, buildDynoUiSettings());
}

function scheduleSaveDynoUiToProject(): void {
  if (applyingProjectUi) return;
  if (saveDynoUiTimer !== 0) window.clearTimeout(saveDynoUiTimer);
  saveDynoUiTimer = window.setTimeout(() => {
    saveDynoUiTimer = 0;
    void flushDynoUiToProject();
  }, 400);
}

function toggleSettings(): void {
  settingsOpen.value = !settingsOpen.value;
  scheduleSaveDynoUiToProject();
}

function onChartAxisChange(): void {
  if (applyingProjectUi) return;
  dynoChartRenderer.resetLiveCache();
  cachedOverlaySig = "";
  scheduleSaveDynoUiToProject();
  if (tabActive.value) scheduleRedraw();
}

function onChartHeightChange(event: Event): void {
  const raw = Number((event.target as HTMLInputElement).value);
  if (!Number.isFinite(raw)) return;
  const h = Math.min(CHART_HEIGHT_MAX, Math.max(CHART_HEIGHT_MIN, Math.round(raw)));
  chartSizeOverride.height = h;
  scheduleSaveDynoUiToProject();
  scheduleRedraw();
}

const canvasRef = ref<HTMLCanvasElement | null>(null);
const chartWrapRef = ref<HTMLElement | null>(null);
const rootRef = ref<HTMLDivElement | null>(null);
const chartOverlay = ref<DynoChartOverlay>({ labels: [] });
const chartHover = ref(false);
const crosshairX = ref<number | null>(null);
const chRpmEl = ref<HTMLElement | null>(null);
const chNmEl = ref<HTMLElement | null>(null);
const chHpEl = ref<HTMLElement | null>(null);
const webglFailed = ref(false);
let rendererAttached = false;
let tabBootGen = 0;
let cachedOverlaySig = "";
let cachedChartWidth = 1;
let crosshairRaf = 0;

function crosshairSpec(): DynoCrosshairSpec | null {
  if (
    !chartHover.value ||
    crosshairX.value === null ||
    chartPoints.value.length < 2
  ) {
    return null;
  }
  return { x: crosshairX.value };
}

function updateCrosshairFromEvent(e: PointerEvent): void {
  const wrap = chartWrapRef.value;
  if (!wrap) return;
  const rect = wrap.getBoundingClientRect();
  crosshairX.value = e.clientX - rect.left;
}

function applyCrosshairLabelStyle(el: HTMLElement, style: Record<string, string>): void {
  for (const [key, val] of Object.entries(style)) {
    el.style.setProperty(key.replace(/([A-Z])/g, "-$1").toLowerCase(), val);
  }
}

function updateCrosshairLabels(layout: DynoChartLayout, crosshair: DynoCrosshairSpec | null): void {
  const els = [chRpmEl.value, chNmEl.value, chHpEl.value];
  if (!crosshair || recording.value) {
    for (const el of els) {
      if (el) el.hidden = true;
    }
    return;
  }
  const markers = buildDynoCrosshairMarkers(layout, chartPoints.value, crosshair);
  for (let i = 0; i < els.length; i += 1) {
    const el = els[i];
    const mk = markers[i];
    if (!el || !mk) continue;
    el.textContent = mk.label;
    el.hidden = false;
    applyCrosshairLabelStyle(el, dynoCrosshairMarkerStyle(mk, cachedChartWidth));
  }
}

function onChartPointerMove(e: PointerEvent): void {
  if (!chartHover.value || recording.value || chartPoints.value.length < 2) return;
  updateCrosshairFromEvent(e);
  cancelAnimationFrame(crosshairRaf);
  crosshairRaf = requestAnimationFrame(() => {
    const crosshair = crosshairSpec();
    dynoChartRenderer.repaintCrosshair(crosshair);
    const layout = dynoChartRenderer.lastLayout();
    if (layout) updateCrosshairLabels(layout, crosshair);
  });
}

function onChartPointerLeave(): void {
  chartHover.value = false;
  crosshairX.value = null;
  cancelAnimationFrame(crosshairRaf);
  dynoChartRenderer.repaintCrosshair(null);
  const layout = dynoChartRenderer.lastLayout();
  if (layout) updateCrosshairLabels(layout, null);
}

function releaseChartRenderer(): void {
  tabBootGen += 1;
  cancelAnimationFrame(redrawRaf);
  cancelAnimationFrame(crosshairRaf);
  dynoChartRenderer.detach();
  rendererAttached = false;
}

async function bootChart(bootGen: number): Promise<void> {
  if (rendererAttached || webglFailed.value) return;
  await Promise.all([initOutputChannels(), initConfig(), initProject()]);
  if (bootGen !== tabBootGen) return;
  for (let attempt = 0; attempt < 8 && !rendererAttached; attempt += 1) {
    await nextTick();
    if (bootGen !== tabBootGen) return;
    await ensureChartRenderer();
  }
}

async function onTabActivated(): Promise<void> {
  const gen = ++tabBootGen;
  await nextTick();
  if (gen !== tabBootGen || !tabActive.value || !ready.value) return;
  if (!isDynoDelta(state.value)) {
    syncFullDynoState(state.value);
  }
  await bootChart(gen);
  if (gen !== tabBootGen || !rendererAttached) return;
  syncChartPointBuffers();
  await applyDynoUiFromProject({ reloadPanelState: true });
  scheduleRedraw();
}

async function ensureChartRenderer(): Promise<void> {
  if (rendererAttached || webglFailed.value) return;
  await nextTick();
  const canvas = canvasRef.value;
  if (!canvas) return;
  if (!dynoChartRenderer.attach(canvas)) {
    webglFailed.value = true;
    return;
  }
  rendererAttached = true;
}

function redraw(): void {
  const canvas = canvasRef.value;
  if (!canvas || webglFailed.value || !rendererAttached) return;
  cachedChartWidth = measureChartWidth(chartWrapRef.value ?? rootRef.value, 1);
  const h = chartHeight.value;
  if (cachedChartWidth < 1) return;

  const crosshair = recording.value ? null : crosshairSpec();
  const layout = dynoChartRenderer.paint({
    width: cachedChartWidth,
    height: h,
    points: chartPoints.value,
    previousPoints: chartPreviousPoints.value,
    crosshair,
    recording: recording.value,
    axes: chartAxes.value,
  });

  const overlaySig = dynoLayoutSignature(layout, cachedChartWidth, h);
  if (overlaySig !== cachedOverlaySig) {
    cachedOverlaySig = overlaySig;
    chartOverlay.value = buildDynoChartOverlay(cachedChartWidth, h, layout, {
      showPreviousLegend: chartPreviousPoints.value.length >= 2,
    });
  }

  if (!recording.value) {
    updateCrosshairLabels(layout, crosshair);
  } else {
    updateCrosshairLabels(layout, null);
  }
}

let redrawRaf = 0;
function scheduleRedraw(): void {
  if (!tabActive.value) return;
  cancelAnimationFrame(redrawRaf);
  redrawRaf = requestAnimationFrame(redraw);
}

useChartCanvasLayout(chartWrapRef, scheduleRedraw);

// Параметры расчёта — это настройки компонента (config MCU игнорируем). При
// изменении сохраняем в проект и отправляем в Rust-логику дино.
watch(
  () => ({ ...carParams }),
  () => {
    if (applyingProjectUi) return;
    void pushDynoConfig();
    scheduleSaveDynoUiToProject();
  },
  { deep: true },
);

watch([tabActive, ready], ([active, r], [wasActive]) => {
  if (active && r) void onTabActivated();
  else if (wasActive && !active) releaseChartRenderer();
}, { immediate: true });

watch(ready, (r) => {
  if (r && tabActive.value) void onTabActivated();
});

watch(projectUiEpoch, () => {
  if (tabActive.value) void applyDynoUiFromProject({ reloadPanelState: false });
});

watch(workspaceResetEpoch, () => {
  if (tabActive.value) void applyDynoUiFromProject({ reloadPanelState: true });
});

watch([ignoreTpsMin, minRpm], () => {
  if (applyingProjectUi) return;
  void syncOptionsToRust();
  scheduleSaveDynoUiToProject();
});

watch(
  [chartRpmMin, chartRpmMax, chartNmMin, chartNmMax, chartHpMin, chartHpMax],
  onChartAxisChange,
);

watch(smoothStrength, (v) => {
  const c = clampSmoothStrength(v);
  if (c !== v) smoothStrength.value = c;
  if (!tabActive.value) return;
  syncChartPointBuffers();
  scheduleRedraw();
});

watch(chartHeight, () => {
  scheduleSaveDynoUiToProject();
  if (tabActive.value) scheduleRedraw();
});

let unregUiFlush: (() => void) | null = null;

onMounted(() => {
  void initProject().then(() => {
    unregUiFlush = registerProjectUiFlushHook(flushDynoUiToProject);
  });
});

onUnmounted(() => {
  unregUiFlush?.();
  releaseChartRenderer();
  if (saveDynoUiTimer !== 0) window.clearTimeout(saveDynoUiTimer);
});
</script>

<template>
  <div ref="rootRef" class="dyno-card">
    <p v-if="mounting" class="dyno-hint">Подключение…</p>

    <template v-else-if="ready || !hasLogic">
      <header class="dyno-header">
        <div class="dyno-status" :data-mode="statusMode">
          <span class="dyno-status-dot" aria-hidden="true" />
          <span>{{ statusLabel }}</span>
        </div>
        <button
          type="button"
          class="dyno-gear"
          :class="{ 'dyno-gear--open': settingsOpen }"
          title="Настройки dyno"
          aria-label="Настройки dyno"
          @click="toggleSettings"
        >
          ⚙
        </button>
      </header>

      <div class="dyno-metrics">
        <div class="dyno-metric">
          <span class="dyno-metric-label">RPM</span>
          <span class="dyno-metric-value">
            {{ liveRpm != null ? Math.round(liveRpm).toLocaleString("ru-RU") : "—" }}
          </span>
        </div>
        <div class="dyno-metric">
          <span class="dyno-metric-label">TPS</span>
          <span class="dyno-metric-value">
            {{ liveTps != null ? `${liveTps.toFixed(1)}%` : "—" }}
          </span>
        </div>
        <div class="dyno-metric" :class="{ 'dyno-metric--live': recording || showRunPeaks }">
          <span class="dyno-metric-label">Nm</span>
          <span class="dyno-metric-value">{{ displayTorque.toFixed(1) }}</span>
        </div>
        <div class="dyno-metric" :class="{ 'dyno-metric--live': recording || showRunPeaks }">
          <span class="dyno-metric-label">HP</span>
          <span class="dyno-metric-value">{{ displayHp.toFixed(1) }}</span>
        </div>
      </div>

      <div
        ref="chartWrapRef"
        class="dyno-chart-wrap"
        :class="{ 'dyno-chart-wrap--hover': chartHover && chartPoints.length >= 2 && !webglFailed }"
        @pointerenter="chartHover = true"
        @pointerleave="onChartPointerLeave"
        @pointermove="onChartPointerMove"
      >
        <canvas ref="canvasRef" class="dyno-canvas" />
        <div v-if="webglFailed" class="dyno-chart-empty">
          WebGL недоступен — график dyno не может отображаться
        </div>
        <template v-else>
          <div class="dyno-chart-overlay" aria-hidden="true">
            <span
              v-for="(lb, i) in chartOverlay.labels"
              :key="`dyno-ol-${i}`"
              class="dyno-ol-label"
              :style="{
                left: `${lb.left}px`,
                top: `${lb.top}px`,
                color: lb.color,
                textAlign: lb.align,
                transform: lb.transform ?? 'translate(-50%, -50%)',
              }"
            >{{ lb.text }}</span>
            <span ref="chRpmEl" class="dyno-crosshair-tag" hidden />
            <span ref="chNmEl" class="dyno-crosshair-tag" hidden />
            <span ref="chHpEl" class="dyno-crosshair-tag" hidden />
          </div>
        </template>
        <p
          v-if="!webglFailed && localRunPoints.length === 0 && !recording && localPreviousRunPoints.length === 0"
          class="dyno-chart-empty"
        >
          Start → разгон → Stop
        </p>
      </div>

      <div class="dyno-actions">
        <button
          type="button"
          class="dyno-btn dyno-btn--toggle"
          :class="{ 'dyno-btn--toggle-recording': recording }"
          :disabled="!canToggleRecord"
          @click="toggleRecording"
        >
          {{ recording ? "Stop" : "Start" }}
        </button>
      </div>

      <Transition name="dyno-settings">
        <section v-if="settingsOpen" class="dyno-settings">
          <div class="dyno-settings-block">
            <h3 class="dyno-settings-title">Параметры записи</h3>

          <label class="dyno-check">
            <input v-model="ignoreTpsMin" type="checkbox" :disabled="recording" />
            <span>Без ограничения TPS (≥ 30%)</span>
          </label>

          <label class="dyno-field">
            <span>Мин. RPM</span>
            <input
              v-model.number="minRpm"
              type="number"
              min="0"
              max="20000"
              step="100"
              :disabled="recording"
            />
            <span class="dyno-field-hint">0 — не использовать</span>
          </label>

          <div class="dyno-field">
            <span>Сглаживание графика</span>
            <div
              ref="smoothTrackRef"
              class="dyno-smooth-track"
              :class="{ 'dyno-smooth-track--disabled': smoothDisabled }"
              role="slider"
              tabindex="0"
              aria-valuemin="0"
              :aria-valuemax="SMOOTH_MAX"
              :aria-valuenow="smoothStrength"
              aria-label="Сглаживание кривой"
              @mousedown="onSmoothTrackPointerDown"
              @keydown="onSmoothTrackKeydown"
            >
              <div class="dyno-smooth-rail" />
              <div class="dyno-smooth-fill" :style="{ width: `${smoothPct}%` }" />
              <div class="dyno-smooth-thumb" :style="{ left: `${smoothPct}%` }" />
            </div>
            <span class="dyno-field-hint">
              {{ clampSmoothStrength(smoothStrength) }} / {{ SMOOTH_MAX }} — только отображение
            </span>
          </div>

          <label class="dyno-field">
            <span>Высота графика, px</span>
            <input
              :value="chartHeight"
              type="number"
              :min="CHART_HEIGHT_MIN"
              :max="CHART_HEIGHT_MAX"
              step="20"
              @change="onChartHeightChange"
            />
          </label>

          <button
            type="button"
            class="dyno-link"
            :disabled="!canClear"
            @click="clearRun"
          >
            Очистить график
          </button>
          </div>

          <div class="dyno-settings-block">
            <h3 class="dyno-settings-title">Оси графика</h3>
            <p class="dyno-field-hint">Фиксированный масштаб — не подстраивается под данные.</p>
            <div class="dyno-axis-grid">
              <label class="dyno-field">
                <span>RPM min</span>
                <input v-model.number="chartRpmMin" type="number" min="0" max="20000" step="100" />
              </label>
              <label class="dyno-field">
                <span>RPM max</span>
                <input v-model.number="chartRpmMax" type="number" min="0" max="20000" step="100" />
              </label>
              <label class="dyno-field">
                <span>Nm min</span>
                <input v-model.number="chartNmMin" type="number" min="0" max="2000" step="10" />
              </label>
              <label class="dyno-field">
                <span>Nm max</span>
                <input v-model.number="chartNmMax" type="number" min="0" max="2000" step="10" />
              </label>
              <label class="dyno-field">
                <span>HP min</span>
                <input v-model.number="chartHpMin" type="number" min="0" max="2000" step="10" />
              </label>
              <label class="dyno-field">
                <span>HP max</span>
                <input v-model.number="chartHpMax" type="number" min="0" max="2000" step="10" />
              </label>
            </div>
          </div>

          <div class="dyno-settings-block">
            <h3 class="dyno-settings-title">Параметры расчёта (компонент)</h3>
            <p class="dyno-field-hint">
              Хранятся в настройках компонента, не в ECU. Настройки MCU не используются.
            </p>
            <div class="dyno-axis-grid">
              <label class="dyno-field">
                <span>Шаг RPM</span>
                <input v-model.number="carParams.dynoRpmStep" type="number" min="1" max="250" step="1" />
              </label>
              <label class="dyno-field">
                <span>SAE темп., °C</span>
                <input v-model.number="carParams.dynoSaeTemperatureC" type="number" min="-80" max="80" step="1" />
              </label>
              <label class="dyno-field">
                <span>SAE влажн., %</span>
                <input v-model.number="carParams.dynoSaeRelativeHumidity" type="number" min="0" max="100" step="1" />
              </label>
              <label class="dyno-field">
                <span>SAE баро, kPa</span>
                <input v-model.number="carParams.dynoSaeBaro" type="number" min="30" max="110" step="0.1" />
              </label>
              <label class="dyno-field">
                <span>Диск, дюйм</span>
                <input v-model.number="carParams.dynoCarWheelDiaInch" type="number" min="0" max="24" step="0.5" />
              </label>
              <label class="dyno-field">
                <span>Профиль, %</span>
                <input v-model.number="carParams.dynoCarWheelAspectRatio" type="number" min="0" max="100" step="1" />
              </label>
              <label class="dyno-field">
                <span>Ширина, мм</span>
                <input v-model.number="carParams.dynoCarWheelTireWidthMm" type="number" min="0" max="400" step="5" />
              </label>
              <label class="dyno-field">
                <span>Главная пара</span>
                <input v-model.number="carParams.dynoCarGearPrimaryReduction" type="number" min="0" max="10" step="0.01" />
              </label>
              <label class="dyno-field">
                <span>Передача</span>
                <input v-model.number="carParams.dynoCarGearRatio" type="number" min="0" max="10" step="0.01" />
              </label>
              <label class="dyno-field">
                <span>Главный редуктор</span>
                <input v-model.number="carParams.dynoCarGearFinalDrive" type="number" min="0" max="10" step="0.01" />
              </label>
              <label class="dyno-field">
                <span>Масса авто, кг</span>
                <input v-model.number="carParams.dynoCarCarMassKg" type="number" min="0" max="5000" step="10" />
              </label>
              <label class="dyno-field">
                <span>Груз, кг</span>
                <input v-model.number="carParams.dynoCarCargoMassKg" type="number" min="0" max="1000" step="5" />
              </label>
              <label class="dyno-field">
                <span>Cx (drag)</span>
                <input v-model.number="carParams.dynoCarCoeffOfDrag" type="number" min="0" max="1" step="0.01" />
              </label>
              <label class="dyno-field">
                <span>Площадь, м²</span>
                <input v-model.number="carParams.dynoCarFrontalAreaM2" type="number" min="0" max="100" step="0.1" />
              </label>
            </div>
          </div>
        </section>
      </Transition>

      <p v-if="!connected" class="dyno-note dyno-note--warn">Подключите ECU для live output.</p>
      <p v-else-if="!configLoaded" class="dyno-note dyno-note--warn">
        Загрузите config — нужен для live-каналов RPM/TPS.
      </p>
      <p
        v-if="message || error"
        class="dyno-note"
        :class="{
          'dyno-note--error': !!error,
          'dyno-note--ok': recording && !error,
        }"
      >
        {{ error ?? message }}
      </p>
    </template>
  </div>
</template>

<style scoped>
.dyno-card {
  width: 100%;
  max-width: 100%;
  min-width: 0;
  align-self: stretch;
  box-sizing: border-box;
  padding: 1.15rem 1.25rem 1.25rem;
  border-radius: var(--radius-lg, 12px);
  border: 1px solid var(--color-border);
  background: var(--color-bg-elevated);
}

.dyno-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  margin-bottom: 0.85rem;
}

.dyno-status {
  display: inline-flex;
  align-items: center;
  gap: 0.45rem;
  padding: 0.25rem 0.65rem;
  border-radius: 999px;
  font-size: 0.78rem;
  font-weight: 600;
  letter-spacing: 0.03em;
  text-transform: uppercase;
  background: var(--color-bg-subtle, rgba(255, 255, 255, 0.06));
  color: var(--color-text-muted);
}

.dyno-status-dot {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 50%;
  background: currentColor;
  opacity: 0.85;
}

.dyno-status[data-mode="recording"] {
  color: var(--color-accent-hover, var(--color-accent));
}

.dyno-status[data-mode="done"] {
  color: var(--color-success-text, #6ecf8a);
  background: var(--color-bg-accent-soft, rgba(110, 207, 138, 0.12));
}

.dyno-status[data-mode="offline"],
.dyno-status[data-mode="noconfig"] {
  opacity: 0.65;
}

.dyno-gear {
  width: 2.1rem;
  height: 2.1rem;
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-md, 8px);
  background: transparent;
  color: var(--color-text-muted);
  font-size: 1rem;
  line-height: 1;
  cursor: pointer;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}

.dyno-gear:hover {
  color: var(--color-text);
  border-color: var(--color-accent);
}

.dyno-gear--open {
  background: var(--color-bg-accent-soft, rgba(255, 255, 255, 0.08));
  color: var(--color-accent);
  border-color: var(--color-accent);
}

.dyno-metrics {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 0.5rem;
  margin-bottom: 0.85rem;
}

.dyno-metric {
  padding: 0.55rem 0.65rem;
  border-radius: var(--radius-md, 8px);
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  text-align: center;
}

.dyno-metric--live {
  border-color: var(--color-accent);
  background: var(--color-bg-accent-soft, rgba(255, 255, 255, 0.04));
}

.dyno-metric-label {
  display: block;
  font-size: 0.68rem;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--color-text-subtle);
  margin-bottom: 0.2rem;
}

.dyno-metric-value {
  display: block;
  font-size: 1.05rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: var(--color-text);
}

.dyno-chart-wrap {
  position: relative;
  width: 100%;
  min-height: 200px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md, 8px);
  background: var(--color-bg);
  overflow: hidden;
  margin-bottom: 0.75rem;
}

.dyno-chart-wrap--hover {
  cursor: crosshair;
}

.dyno-canvas {
  display: block;
  width: 100%;
  pointer-events: none;
}

.dyno-chart-overlay {
  position: absolute;
  inset: 0;
  pointer-events: none;
  overflow: hidden;
}

.dyno-ol-label {
  position: absolute;
  font: 10px Segoe UI, system-ui, sans-serif;
  color: var(--color-text-subtle);
  white-space: nowrap;
  line-height: 1;
}

.dyno-crosshair-tag {
  position: absolute;
  padding: 2px 5px;
  font: 600 10px "Segoe UI", system-ui, sans-serif;
  line-height: 1;
  white-space: nowrap;
  background: color-mix(in srgb, var(--color-bg-elevated) 92%, transparent);
  border: 1px solid;
  border-radius: 2px;
  pointer-events: none;
}

.dyno-chart-empty {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  margin: 0;
  font-size: 0.88rem;
  color: var(--color-text-subtle);
  pointer-events: none;
}

.dyno-actions {
  display: block;
}

.dyno-btn {
  width: 100%;
  padding: 0.72rem 0.75rem;
  border-radius: var(--radius-md, 8px);
  border: 1px solid transparent;
  font-weight: 600;
  font-size: 0.88rem;
  cursor: pointer;
  transition: opacity 0.15s, transform 0.1s;
}

.dyno-btn:active:not(:disabled) {
  transform: scale(0.98);
}

.dyno-btn:disabled {
  opacity: 0.38;
  cursor: not-allowed;
}

.dyno-btn--toggle {
  background: var(--color-accent);
  color: var(--color-on-accent);
}

.dyno-btn--toggle-recording {
  background: var(--color-gray);
  color: var(--color-on-gray);
}

.dyno-settings {
  margin-top: 1rem;
  padding-top: 1rem;
  border-top: 1px solid var(--color-border);
  display: grid;
  gap: 0.85rem;
}

.dyno-settings-block {
  display: grid;
  gap: 0.75rem;
}

.dyno-settings-block + .dyno-settings-block {
  padding-top: 0.85rem;
  border-top: 1px dashed var(--color-border);
}

.dyno-chars-host {
  display: grid;
  gap: 0.65rem;
}

.dyno-chars-host :deep(.section) {
  width: auto;
  padding: 0.75rem 0.85rem;
  box-shadow: none;
}

.dyno-chars-host :deep(.section-title) {
  margin-bottom: 0.65rem;
  font-size: 0.85rem;
}

.dyno-chars-host :deep(.enum-field),
.dyno-chars-host :deep(.scalar-field),
.dyno-chars-host :deep(.string-field) {
  width: auto;
  max-width: 100%;
}

.dyno-chars-host :deep(.field-select),
.dyno-chars-host :deep(.field-input) {
  width: 100%;
  max-width: 14rem;
  box-sizing: border-box;
}

.dyno-settings-title {
  margin: 0;
  font-size: 0.82rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-text-muted);
}

.dyno-check {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.88rem;
  color: var(--color-text);
  cursor: pointer;
}

.dyno-check input {
  width: 1rem;
  height: 1rem;
}

.dyno-field {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.dyno-field > span:first-child {
  font-size: 0.72rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-text-subtle);
}

.dyno-field input[type="number"] {
  max-width: 8rem;
  padding: 0.45rem 0.55rem;
  border-radius: var(--radius-md, 8px);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 0.88rem;
}

.dyno-field-hint {
  font-size: 0.75rem;
  color: var(--color-text-subtle);
}

.dyno-axis-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 0.65rem 1rem;
}

@media (max-width: 520px) {
  .dyno-axis-grid {
    grid-template-columns: 1fr;
  }
}

.dyno-smooth-track {
  position: relative;
  height: 1.25rem;
  max-width: 16rem;
  cursor: pointer;
  touch-action: none;
}

.dyno-smooth-track--disabled {
  opacity: 0.38;
  pointer-events: none;
  cursor: not-allowed;
}

.dyno-smooth-track:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: 3px;
  border-radius: 999px;
}

.dyno-smooth-rail {
  position: absolute;
  left: 0;
  right: 0;
  top: 50%;
  height: 0.3rem;
  transform: translateY(-50%);
  border-radius: 999px;
  background: var(--color-border-strong);
}

.dyno-smooth-fill {
  position: absolute;
  top: 50%;
  left: 0;
  height: 0.3rem;
  transform: translateY(-50%);
  border-radius: 999px;
  background: var(--color-accent);
  pointer-events: none;
}

.dyno-smooth-thumb {
  position: absolute;
  top: 50%;
  width: 1rem;
  height: 1rem;
  margin-left: -0.5rem;
  border-radius: 50%;
  background: var(--color-bg-elevated);
  border: 2px solid var(--color-accent);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.2);
  transform: translateY(-50%);
  pointer-events: none;
}

.dyno-note {
  margin: 0.85rem 0 0;
  font-size: 0.82rem;
  color: var(--color-text-muted);
}

.dyno-note--warn {
  color: var(--color-text-subtle);
}

.dyno-note--error {
  color: var(--color-error);
}

.dyno-note--ok {
  color: var(--color-success-text);
}

.dyno-hint {
  margin: 0;
  font-size: 0.88rem;
  color: var(--color-text-muted);
}

.dyno-settings-enter-active,
.dyno-settings-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}

.dyno-settings-enter-from,
.dyno-settings-leave-to {
  opacity: 0;
  transform: translateY(-6px);
}

@media (max-width: 520px) {
  .dyno-metrics {
    grid-template-columns: repeat(2, 1fr);
  }
}
</style>
