<script setup lang="ts">
import {
  computed,
  nextTick,
  onMounted,
  onUnmounted,
  ref,
  shallowRef,
  watch,
} from "vue";
import { parse as parseYaml } from "yaml";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { childPath } from "../../core/instance";
import ComponentHost from "../ComponentHost.vue";
import { useDataContext } from "../../core/data-context";
import { initConfig, useConfig } from "../../composables/useConfig";
import {
  drawKnockThresholdChart,
  type KnockRpmValuePoint,
  type KnockRunPoint,
} from "../../composables/drawKnockChart";
import {
  drawKnockSpectrogram,
} from "../../composables/drawKnockSpectrogram";
import { initKnockScope, useKnockScope } from "../../composables/useKnockScope";
import {
  initProject,
  PERSIST_KEY_KNOCK,
  projectUiEpoch,
  useProject,
  workspaceResetEpoch,
  type KnockUiSettings,
} from "../../composables/useProject";
import { useRustComponent } from "../../composables/useRustComponent";
import { useInstanceBind } from "../../composables/useInstanceBind";
import { useTabActivity, useTabFrozenDisplay } from "../../composables/useTabActivity";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const CHART_HEIGHT_MIN = 180;
const CHART_HEIGHT_MAX = 720;
const SPECTROGRAM_HEIGHT_MIN = 160;
const SPECTROGRAM_HEIGHT_DEFAULT = 240;

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
const { paramStringOr } = useInstanceBind(instanceRef);
const dataCtx = useDataContext();
const { snapshot: configSnapshot, getArray: getConfigArray } = useConfig();
const { snapshot: knockScopeSnapshot, spectrogramView } = useKnockScope();
const { isActive: tabActive } = useTabActivity();
const { getProjectUi, setProjectUi } = useProject();

let applyingProjectUi = false;
let saveUiTimer = 0;
let channelsConfigured = false;

const rpmField = computed(() =>
  String(state.value.rpmField ?? paramStringOr("rpmField", "RPMValue")),
);
const knockLevelField = computed(() =>
  String(state.value.knockLevelField ?? paramStringOr("knockLevelField", "m_knockLevel")),
);
const knockThresholdField = computed(() =>
  String(
    state.value.knockThresholdField ??
      paramStringOr("knockThresholdField", "m_knockThreshold"),
  ),
);

watch(ready, (isReady) => {
  if (!isReady || channelsConfigured) return;
  channelsConfigured = true;
  void dispatch("set_channels", {
    rpmField: rpmField.value,
    tpsField: paramStringOr("tpsField", "TPSValue"),
    knockLevelField: knockLevelField.value,
    knockThresholdField: knockThresholdField.value,
    loadField: paramStringOr("loadField", "ignitionLoad"),
    advanceField: paramStringOr("advanceField", "ignitionAdvanceCyl1"),
  });
});

const liveRpm = useTabFrozenDisplay(() => {
  const v = state.value.liveRpm;
  return v != null && Number.isFinite(Number(v)) ? Number(v) : null;
}, null as number | null);
const liveLevel = useTabFrozenDisplay(() => {
  const v = state.value.liveKnockLevel;
  return v != null && Number.isFinite(Number(v)) ? Number(v) : null;
}, null as number | null);

const recording = computed(() => Boolean(state.value.recording));
const mode = computed(() => String(state.value.mode ?? "idle"));
const runPoints = computed((): KnockRunPoint[] => {
  const raw = (state.value.runPoints as Array<Record<string, unknown>> | undefined) ?? [];
  return raw.map((p) => ({
    rpm: Number(p.rpm ?? 0),
    knockLevel: Number(p.knockLevel ?? 0),
    threshold: Number(p.threshold ?? 0),
  }));
});
const previousRunPoints = computed((): KnockRunPoint[] => {
  const raw =
    (state.value.previousRunPoints as Array<Record<string, unknown>> | undefined) ?? [];
  return raw.map((p) => ({
    rpm: Number(p.rpm ?? 0),
    knockLevel: Number(p.knockLevel ?? 0),
    threshold: Number(p.threshold ?? 0),
  }));
});

const runLevelPoints = computed((): KnockRpmValuePoint[] =>
  runPoints.value.map((p) => ({ rpm: p.rpm, value: p.knockLevel })),
);
const previousRunLevelPoints = computed((): KnockRpmValuePoint[] =>
  previousRunPoints.value.map((p) => ({ rpm: p.rpm, value: p.knockLevel })),
);

function parsePeakCurve(raw: Array<Record<string, unknown>> | undefined): KnockRpmValuePoint[] {
  return (raw ?? [])
    .map((p) => ({ rpm: Number(p.rpm ?? 0), value: Number(p.value ?? 0) }))
    .filter((p) => p.rpm > 0 && Number.isFinite(p.value));
}

const runPeakCurve = computed((): KnockRpmValuePoint[] =>
  parsePeakCurve(state.value.runPeakCurve as Array<Record<string, unknown>> | undefined),
);
const previousRunPeakCurve = computed((): KnockRpmValuePoint[] =>
  parsePeakCurve(state.value.previousRunPeakCurve as Array<Record<string, unknown>> | undefined),
);

/** Кривая knock level для графика: по RPM-бинам (live), иначе точки прогона. */
const knockLevelChartPoints = computed((): KnockRpmValuePoint[] => {
  if (runPeakCurve.value.length > 0) return runPeakCurve.value;
  return runLevelPoints.value;
});
const previousKnockLevelChartPoints = computed((): KnockRpmValuePoint[] => {
  if (previousRunPeakCurve.value.length > 0) return previousRunPeakCurve.value;
  return previousRunLevelPoints.value;
});

const configThresholdCurve = ref<KnockRpmValuePoint[]>([]);

function interpolateConfigThreshold(rpm: number, curve: KnockRpmValuePoint[]): number | null {
  if (curve.length === 0 || !Number.isFinite(rpm)) return null;
  const sorted = [...curve].sort((a, b) => a.rpm - b.rpm);
  if (rpm <= sorted[0]!.rpm) return sorted[0]!.value;
  if (rpm >= sorted[sorted.length - 1]!.rpm) return sorted[sorted.length - 1]!.value;
  for (let i = 1; i < sorted.length; i += 1) {
    const a = sorted[i - 1]!;
    const b = sorted[i]!;
    if (rpm >= a.rpm && rpm <= b.rpm) {
      const t = (rpm - a.rpm) / (b.rpm - a.rpm);
      return a.value + t * (b.value - a.value);
    }
  }
  return null;
}

const previewThresholdCurve = computed((): KnockRpmValuePoint[] => {
  const raw =
    (state.value.previewThresholdCurve as Array<Record<string, unknown>> | undefined) ?? [];
  return raw
    .map((p) => ({ rpm: Number(p.rpm ?? 0), value: Number(p.value ?? 0) }))
    .filter((p) => p.rpm > 0 && Number.isFinite(p.value));
});

const thresholdChartCurve = computed((): KnockRpmValuePoint[] => {
  if (recordingThreshold.value) {
    if (previewThresholdCurve.value.length > 0) return previewThresholdCurve.value;
    return configThresholdCurve.value;
  }
  if (configThresholdCurve.value.length > 0) return configThresholdCurve.value;
  return previewThresholdCurve.value;
});

const configThresholdLive = computed(() => {
  const rpm = liveRpm.value;
  if (rpm == null) return null;
  return interpolateConfigThreshold(rpm, thresholdChartCurve.value);
});

async function reloadConfigThresholdCurve(): Promise<void> {
  if (!configSnapshot.value.loaded) {
    configThresholdCurve.value = [];
    return;
  }
  try {
    const [rpms, dbs] = await Promise.all([
      getConfigArray("knockNoiseRpmBins"),
      getConfigArray("knockBaseNoise"),
    ]);
    const n = Math.min(rpms.length, dbs.length);
    configThresholdCurve.value = Array.from({ length: n }, (_, i) => ({
      rpm: rpms[i] ?? 0,
      value: dbs[i] ?? 0,
    })).filter((p) => p.rpm > 0 || p.value !== 0);
  } catch {
    configThresholdCurve.value = [];
  }
  scheduleThresholdRedraw();
}

const spectrogramHint = computed(() => {
  const s = knockScopeSnapshot.value;
  if (s.lastError) return s.lastError;
  if (s.statusMessage) return s.statusMessage;
  if (recordingSpectrum.value && spectrogramView.value.width < 1) {
    return "Ждём захват knock scope и FFT…";
  }
  return null;
});

const recordingThreshold = computed(
  () => recording.value && mode.value === "thresholdAutotune",
);
const recordingSpectrum = computed(
  () => recording.value && mode.value === "spectrumCapture",
);

const detectedHz = computed(() => state.value.detectedFrequencyHz as number | null | undefined);
const message = computed(() => (state.value.message as string) ?? null);
const momentumPhase = computed(() => String(state.value.momentumPhase ?? "idle"));

const ignoreTpsMin = ref(true);
const minRpm = ref(800);
const cutoffRpm = ref(6500);
const thresholdGapDb = ref(3);
const tempTargetLambda = ref(0.85);
const tempIgnitionRetardDeg = ref(8);
const momentumSafeRpmMin = ref(2000);
const momentumSafeRpmMax = ref(3500);
const momentumMinLoad = ref(40);
const momentumAdvanceAddDeg = ref(6);
const momentumDurationMs = ref(800);
const spectrogramWindowMs = ref(500);
const settingsOpen = ref(false);

const connected = computed(
  () => Boolean(state.value.connected ?? dataCtx.connection.value.connected),
);
const configLoaded = computed(
  () => Boolean(state.value.configLoaded ?? configSnapshot.value.loaded),
);

const canRun = computed(
  () => ready.value && connected.value && configLoaded.value && hasLogic.value && !recording.value,
);
const canClear = computed(
  () =>
    !recording.value &&
    (runPoints.value.length > 0 || previousRunPoints.value.length > 0),
);
const canApplyFrequency = computed(
  () =>
    !recording.value &&
    detectedHz.value != null &&
    Number.isFinite(Number(detectedHz.value)),
);

const statusMode = computed(() => {
  if (recording.value) return "recording";
  if (momentumPhase.value === "waiting" || momentumPhase.value === "active") return "momentum";
  if (!connected.value) return "offline";
  if (!configLoaded.value) return "noconfig";
  if (runPoints.value.length > 0) return "done";
  return "idle";
});

const statusLabel = computed(() => {
  switch (statusMode.value) {
    case "recording":
      return mode.value === "spectrumCapture" ? "Спектр" : "Прогон";
    case "momentum":
      return momentumPhase.value === "active" ? "Momentum" : "Ждём зону";
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

function buildUiSettings(): KnockUiSettings {
  return {
    ignoreTpsMin: ignoreTpsMin.value,
    minRpm: Math.max(0, Math.round(minRpm.value)),
    cutoffRpm: Math.max(0, Math.round(cutoffRpm.value)),
    thresholdGapDb: thresholdGapDb.value,
    tempTargetLambda: tempTargetLambda.value,
    tempIgnitionRetardDeg: tempIgnitionRetardDeg.value,
    momentumSafeRpmMin: Math.round(momentumSafeRpmMin.value),
    momentumSafeRpmMax: Math.round(momentumSafeRpmMax.value),
    momentumMinLoad: momentumMinLoad.value,
    momentumAdvanceAddDeg: momentumAdvanceAddDeg.value,
    momentumDurationMs: Math.round(momentumDurationMs.value),
    spectrogramWindowMs: Math.round(spectrogramWindowMs.value),
    chartHeight: chartHeight.value,
    settingsOpen: settingsOpen.value,
  };
}

async function syncOptionsToRust(): Promise<void> {
  if (!ready.value) return;
  await dispatch("set_options", {
    ignoreTpsMin: ignoreTpsMin.value,
    minRpm: Math.max(0, Math.round(minRpm.value)),
    cutoffRpm: Math.max(0, Math.round(cutoffRpm.value)),
    thresholdGapDb: thresholdGapDb.value,
    tempTargetLambda: tempTargetLambda.value,
    tempIgnitionRetardDeg: tempIgnitionRetardDeg.value,
    momentumSafeRpmMin: Math.round(momentumSafeRpmMin.value),
    momentumSafeRpmMax: Math.round(momentumSafeRpmMax.value),
    momentumMinLoad: momentumMinLoad.value,
    momentumAdvanceAddDeg: momentumAdvanceAddDeg.value,
    momentumDurationMs: Math.round(momentumDurationMs.value),
    spectrogramWindowMs: Math.round(spectrogramWindowMs.value),
  });
}

async function applyUiFromProject(): Promise<void> {
  applyingProjectUi = true;
  try {
    const ui = await getProjectUi<KnockUiSettings>(PERSIST_KEY_KNOCK);
    ignoreTpsMin.value = ui.ignoreTpsMin;
    minRpm.value = ui.minRpm;
    cutoffRpm.value = ui.cutoffRpm;
    thresholdGapDb.value = ui.thresholdGapDb;
    tempTargetLambda.value = ui.tempTargetLambda;
    tempIgnitionRetardDeg.value = ui.tempIgnitionRetardDeg;
    momentumSafeRpmMin.value = ui.momentumSafeRpmMin;
    momentumSafeRpmMax.value = ui.momentumSafeRpmMax;
    momentumMinLoad.value = ui.momentumMinLoad;
    momentumAdvanceAddDeg.value = ui.momentumAdvanceAddDeg;
    momentumDurationMs.value = ui.momentumDurationMs;
    spectrogramWindowMs.value = ui.spectrogramWindowMs;
    chartSizeOverride.height = ui.chartHeight > CHART_HEIGHT_MIN ? ui.chartHeight : null;
    settingsOpen.value = ui.settingsOpen;
  } catch {
    ignoreTpsMin.value = Boolean(state.value.ignoreTpsMin);
    minRpm.value = Number(state.value.minRpm ?? 800);
    cutoffRpm.value = Number(state.value.cutoffRpm ?? 6500);
  } finally {
    applyingProjectUi = false;
  }
  await syncOptionsToRust();
  scheduleRedraw();
}

function scheduleSaveUiToProject(): void {
  if (applyingProjectUi) return;
  if (saveUiTimer !== 0) window.clearTimeout(saveUiTimer);
  saveUiTimer = window.setTimeout(() => {
    saveUiTimer = 0;
    void setProjectUi(PERSIST_KEY_KNOCK, buildUiSettings());
  }, 400);
}

function toggleSettings(): void {
  settingsOpen.value = !settingsOpen.value;
  if (settingsOpen.value) void ensureKnockSettingsPanel();
  scheduleSaveUiToProject();
}

async function stopThresholdRun(): Promise<void> {
  await dispatch("stop_run", { applyThreshold: true });
  await reloadConfigThresholdCurve();
  scheduleThresholdRedraw();
}

async function stopSpectrumRun(): Promise<void> {
  await dispatch("stop_run", { applyThreshold: false });
}

const knockSettingsChildren = shallowRef<ComponentInstance[]>([]);
const knockSettingsLoading = ref(false);
const knockSettingsError = ref<string | null>(null);
let knockSettingsLoaded = false;
const knockSettingsBasePath = computed(() => `${props.path}/knock-settings`);

async function ensureKnockSettingsPanel(): Promise<void> {
  if (knockSettingsLoaded || knockSettingsLoading.value) return;
  knockSettingsLoading.value = true;
  knockSettingsError.value = null;
  try {
    const panelId = paramStringOr("knockSettingsPanel", "generated/softwareknock.panel");
    const res = await fetch(`/config/components/${panelId}.yaml`);
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
    const doc = parseYaml(await res.text()) as { children?: ComponentInstance[] };
    knockSettingsChildren.value = doc.children ?? [];
    knockSettingsLoaded = true;
  } catch (e) {
    knockSettingsError.value = e instanceof Error ? e.message : String(e);
  } finally {
    knockSettingsLoading.value = false;
  }
}

const thresholdCanvasRef = ref<HTMLCanvasElement | null>(null);
const thresholdContainerRef = ref<HTMLDivElement | null>(null);
const thresholdCanvasWidth = ref(640);
let thresholdCanvasPixelW = 0;
let thresholdCanvasPixelH = 0;
const spectrogramCanvasRef = ref<HTMLCanvasElement | null>(null);
const spectrogramContainerRef = ref<HTMLDivElement | null>(null);

const spectrogramHeight = computed(() => {
  const h = Number(props.props.spectrogramHeight ?? SPECTROGRAM_HEIGHT_DEFAULT);
  return h >= SPECTROGRAM_HEIGHT_MIN ? h : SPECTROGRAM_HEIGHT_DEFAULT;
});

function redrawThresholdChart(): void {
  const canvas = thresholdCanvasRef.value;
  if (!canvas) return;
  const dpr = window.devicePixelRatio || 1;
  const w = thresholdCanvasWidth.value;
  const h = chartHeight.value;
  const pixelW = Math.floor(w * dpr);
  const pixelH = Math.floor(h * dpr);
  if (pixelW !== thresholdCanvasPixelW || pixelH !== thresholdCanvasPixelH) {
    canvas.width = pixelW;
    canvas.height = pixelH;
    canvas.style.width = `${w}px`;
    canvas.style.height = `${h}px`;
    thresholdCanvasPixelW = pixelW;
    thresholdCanvasPixelH = pixelH;
  }
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  drawKnockThresholdChart(
    ctx,
    w,
    h,
    thresholdChartCurve.value,
    knockLevelChartPoints.value,
    undefined,
    previousKnockLevelChartPoints.value,
    liveRpm.value,
    liveLevel.value,
    recordingThreshold.value ? configThresholdCurve.value : [],
  );
}

function redrawSpectrogramChart(): void {
  const canvas = spectrogramCanvasRef.value;
  if (!canvas) return;
  canvas.style.height = `${spectrogramHeight.value}px`;
  drawKnockSpectrogram(canvas, spectrogramView.value, {
    title: detectedHz.value != null ? `Пик ~${Math.round(Number(detectedHz.value))} Hz` : undefined,
  });
}

let thresholdRedrawRaf = 0;
let lastThresholdDrawMs = 0;
const THRESHOLD_REDRAW_MIN_MS = 50;

function scheduleThresholdRedraw(): void {
  if (!tabActive.value) return;
  if (thresholdRedrawRaf !== 0) return;
  const tick = () => {
    thresholdRedrawRaf = 0;
    const now = performance.now();
    if (now - lastThresholdDrawMs < THRESHOLD_REDRAW_MIN_MS) {
      thresholdRedrawRaf = requestAnimationFrame(tick);
      return;
    }
    lastThresholdDrawMs = now;
    redrawThresholdChart();
  };
  thresholdRedrawRaf = requestAnimationFrame(tick);
}

let spectrogramRedrawRaf = 0;
function scheduleSpectrogramRedraw(): void {
  if (!tabActive.value) return;
  cancelAnimationFrame(spectrogramRedrawRaf);
  spectrogramRedrawRaf = requestAnimationFrame(redrawSpectrogramChart);
}

function scheduleRedraw(): void {
  scheduleThresholdRedraw();
  scheduleSpectrogramRedraw();
}

watch(ready, (r) => {
  if (r) void applyUiFromProject();
});
watch(projectUiEpoch, () => void applyUiFromProject());
watch(workspaceResetEpoch, () => void applyUiFromProject());
watch(configLoaded, (loaded) => {
  if (loaded) void reloadConfigThresholdCurve();
});
watch(configSnapshot, (snap) => {
  if (snap.loaded) void reloadConfigThresholdCurve();
});
watch(recordingThreshold, (rec, wasRec) => {
  if (wasRec && !rec) void reloadConfigThresholdCurve();
});

watch(
  [
    ignoreTpsMin,
    minRpm,
    cutoffRpm,
    thresholdGapDb,
    tempTargetLambda,
    tempIgnitionRetardDeg,
    momentumSafeRpmMin,
    momentumSafeRpmMax,
    momentumMinLoad,
    momentumAdvanceAddDeg,
    momentumDurationMs,
    spectrogramWindowMs,
  ],
  () => {
    if (applyingProjectUi) return;
    void syncOptionsToRust();
    scheduleSaveUiToProject();
  },
);

watch(
  [
    () => state.value.runPeakCurve,
    () => state.value.previousRunPeakCurve,
    () => state.value.previewThresholdCurve,
    configThresholdCurve,
    chartHeight,
    recordingThreshold,
  ],
  () => scheduleThresholdRedraw(),
);
watch([liveLevel, liveRpm], () => scheduleThresholdRedraw());
watch(thresholdChartCurve, () => scheduleThresholdRedraw());
watch([spectrogramView, detectedHz, spectrogramHeight], () => scheduleSpectrogramRedraw());
watch(
  () => knockScopeSnapshot.value.captureCount,
  () => scheduleSpectrogramRedraw(),
);
watch(tabActive, (active, was) => {
  if (active && !was) scheduleRedraw();
});
watch(settingsOpen, (open) => {
  if (open) void ensureKnockSettingsPanel();
});

let resizeObserver: ResizeObserver | undefined;

function setupResizeObservers(): void {
  resizeObserver?.disconnect();
  if (typeof ResizeObserver === "undefined") return;
  resizeObserver = new ResizeObserver((entries) => {
    for (const entry of entries) {
      if (entry.target === thresholdContainerRef.value) {
        thresholdCanvasWidth.value = Math.max(280, entry.contentRect.width);
        scheduleThresholdRedraw();
      }
      if (entry.target === spectrogramContainerRef.value) {
        scheduleSpectrogramRedraw();
      }
    }
  });
  if (thresholdContainerRef.value) resizeObserver.observe(thresholdContainerRef.value);
  if (spectrogramContainerRef.value) resizeObserver.observe(spectrogramContainerRef.value);
}

watch([ready, thresholdContainerRef, spectrogramContainerRef], async () => {
  if (!ready.value) return;
  await nextTick();
  setupResizeObservers();
  scheduleRedraw();
});

onMounted(async () => {
  await Promise.all([initConfig(), initProject(), initKnockScope()]);
  if (configLoaded.value) await reloadConfigThresholdCurve();
});

onUnmounted(() => {
  cancelAnimationFrame(thresholdRedrawRaf);
  cancelAnimationFrame(spectrogramRedrawRaf);
  if (saveUiTimer !== 0) window.clearTimeout(saveUiTimer);
  resizeObserver?.disconnect();
});
</script>

<template>
  <div class="knock-card">
    <p v-if="mounting" class="knock-hint">Подключение…</p>

    <template v-else-if="ready || !hasLogic">
      <header class="knock-header">
        <div class="knock-status" :data-mode="statusMode">
          <span class="knock-status-dot" aria-hidden="true" />
          <span>{{ statusLabel }}</span>
        </div>
        <button
          type="button"
          class="knock-gear"
          :class="{ 'knock-gear--open': settingsOpen }"
          title="Настройки knock"
          aria-label="Настройки knock"
          @click="toggleSettings"
        >
          ⚙
        </button>
      </header>

      <div class="knock-metrics">
        <div class="knock-metric">
          <span class="knock-metric-label">RPM</span>
          <span class="knock-metric-value">
            {{ liveRpm != null ? Math.round(liveRpm).toLocaleString("ru-RU") : "—" }}
          </span>
        </div>
        <div class="knock-metric knock-metric--live">
          <span class="knock-metric-label">Knock</span>
          <span class="knock-metric-value">
            {{ liveLevel != null ? liveLevel.toFixed(2) : "—" }}
          </span>
        </div>
        <div class="knock-metric">
          <span class="knock-metric-label">Threshold (config)</span>
          <span class="knock-metric-value">
            {{ configThresholdLive != null ? configThresholdLive.toFixed(2) : "—" }}
          </span>
        </div>
        <div class="knock-metric" :class="{ 'knock-metric--live': detectedHz != null }">
          <span class="knock-metric-label">Freq</span>
          <span class="knock-metric-value">
            {{ detectedHz != null ? `${Math.round(detectedHz)} Hz` : "—" }}
          </span>
        </div>
      </div>

      <div class="knock-steps-row">
      <section class="knock-step knock-step--threshold">
        <h3 class="knock-step-title">1. Threshold autotune</h3>
        <p class="knock-step-hint">
          График: knockBaseNoise из config (пунктир) и knock level с прогона (сплошная).
        </p>
        <div ref="thresholdContainerRef" class="knock-chart-wrap">
          <canvas ref="thresholdCanvasRef" class="knock-canvas" />
        </div>
        <div class="knock-step-actions">
          <button
            type="button"
            class="knock-btn knock-btn--secondary"
            :disabled="!canRun"
            @click="dispatch('apply_temp_detune')"
          >
            Временные бездетоновые настройки
          </button>
          <button
            type="button"
            class="knock-btn knock-btn--primary"
            :disabled="!canRun"
            @click="dispatch('start_threshold_autotune')"
          >
            Start Threshold Autotune
          </button>
          <button
            type="button"
            class="knock-btn knock-btn--stop"
            :disabled="!recordingThreshold"
            @click="stopThresholdRun"
          >
            Stop
          </button>
        </div>
      </section>

      <section class="knock-step knock-step--spectrum">
        <h3 class="knock-step-title">2. Частота детонации</h3>
        <p class="knock-step-hint">
          Спектрограмма на прогоне + Momentum knock в безопасной зоне.
        </p>
        <div ref="spectrogramContainerRef" class="knock-chart-wrap knock-chart-wrap--spectrogram">
          <canvas ref="spectrogramCanvasRef" class="knock-canvas knock-canvas--spectrogram" />
        </div>
        <p v-if="spectrogramHint" class="knock-step-hint knock-step-hint--scope">{{ spectrogramHint }}</p>
        <div class="knock-step-actions">
          <button
            type="button"
            class="knock-btn knock-btn--primary"
            :disabled="!canRun"
            @click="dispatch('start_spectrum_run')"
          >
            Запись спектрограммы (прогон)
          </button>
          <button
            type="button"
            class="knock-btn knock-btn--secondary"
            :disabled="!connected || recording"
            @click="dispatch('start_momentum_knock')"
          >
            Momentum Knock
          </button>
          <button
            type="button"
            class="knock-btn knock-btn--secondary"
            :disabled="!canApplyFrequency"
            @click="dispatch('apply_frequency')"
          >
            Применить найденную частоту
          </button>
          <button
            type="button"
            class="knock-btn knock-btn--stop"
            :disabled="!recordingSpectrum"
            @click="stopSpectrumRun"
          >
            Stop
          </button>
        </div>
      </section>
      </div>

      <Transition name="knock-settings">
        <section v-if="settingsOpen" class="knock-settings">
          <div class="knock-settings-block">
            <h3 class="knock-settings-title">Прогон</h3>
            <label class="knock-check">
              <input v-model="ignoreTpsMin" type="checkbox" :disabled="recording" />
              <span>Без ограничения TPS (≥ 30%)</span>
            </label>
            <label class="knock-field">
              <span>Мин. RPM</span>
              <input v-model.number="minRpm" type="number" min="0" max="20000" step="100" />
            </label>
            <label class="knock-field">
              <span>Отсечка RPM</span>
              <input v-model.number="cutoffRpm" type="number" min="500" max="20000" step="100" />
            </label>
            <label class="knock-field">
              <span>Зазор threshold, dB</span>
              <input v-model.number="thresholdGapDb" type="number" min="0" max="20" step="0.5" />
            </label>
            <label class="knock-field">
              <span>λ (временно)</span>
              <input
                v-model.number="tempTargetLambda"
                type="number"
                min="0.6"
                max="1.2"
                step="0.01"
              />
            </label>
            <label class="knock-field">
              <span>Отступ УОЗ, °</span>
              <input
                v-model.number="tempIgnitionRetardDeg"
                type="number"
                min="0"
                max="30"
                step="0.5"
              />
            </label>
            <label class="knock-field">
              <span>Окно спектрограммы, ms</span>
              <input
                v-model.number="spectrogramWindowMs"
                type="number"
                min="100"
                max="3000"
                step="50"
              />
            </label>
            <h3 class="knock-settings-title">Momentum knock</h3>
            <label class="knock-field">
              <span>Безопасный RPM min</span>
              <input
                v-model.number="momentumSafeRpmMin"
                type="number"
                min="500"
                max="20000"
                step="100"
              />
            </label>
            <label class="knock-field">
              <span>Безопасный RPM max</span>
              <input
                v-model.number="momentumSafeRpmMax"
                type="number"
                min="500"
                max="20000"
                step="100"
              />
            </label>
            <label class="knock-field">
              <span>Мин. нагрузка, %</span>
              <input v-model.number="momentumMinLoad" type="number" min="0" max="100" step="1" />
            </label>
            <label class="knock-field">
              <span>Добавить УОЗ, °</span>
              <input
                v-model.number="momentumAdvanceAddDeg"
                type="number"
                min="0"
                max="30"
                step="0.5"
              />
            </label>
            <label class="knock-field">
              <span>Длительность, ms</span>
              <input
                v-model.number="momentumDurationMs"
                type="number"
                min="100"
                max="5000"
                step="50"
              />
            </label>
            <label class="knock-field">
              <span>Высота графика, px</span>
              <input
                :value="chartHeight"
                type="number"
                :min="CHART_HEIGHT_MIN"
                :max="CHART_HEIGHT_MAX"
                step="20"
                @change="
                  (e) => {
                    const h = Number((e.target as HTMLInputElement).value);
                    if (Number.isFinite(h))
                      chartSizeOverride.height = Math.min(
                        CHART_HEIGHT_MAX,
                        Math.max(CHART_HEIGHT_MIN, h),
                      );
                    scheduleSaveUiToProject();
                    scheduleRedraw();
                  }
                "
              />
            </label>
            <button type="button" class="knock-link" :disabled="!canClear" @click="dispatch('clear')">
              Очистить график
            </button>
          </div>
          <div class="knock-settings-block">
            <h3 class="knock-settings-title">Knock control (INI)</h3>
            <p v-if="knockSettingsLoading" class="knock-field-hint">Загрузка…</p>
            <p v-else-if="knockSettingsError" class="knock-note knock-note--error">
              {{ knockSettingsError }}
            </p>
            <div v-else class="knock-ini-host">
              <ComponentHost
                v-for="(child, index) in knockSettingsChildren"
                :key="child.id ?? `${index}`"
                :instance="child"
                :path="childPath(knockSettingsBasePath, index, child)"
              />
            </div>
          </div>
        </section>
      </Transition>

      <p v-if="!connected" class="knock-note knock-note--warn">Подключите ECU.</p>
      <p v-else-if="!configLoaded" class="knock-note knock-note--warn">Загрузите config.</p>
      <p
        v-if="message || error"
        class="knock-note"
        :class="{ 'knock-note--error': !!error, 'knock-note--ok': recording && !error }"
      >
        {{ error ?? message }}
      </p>
    </template>
  </div>
</template>

<style scoped>
.knock-card {
  width: 100%;
  max-width: 72rem;
  padding: 1.15rem 1.25rem 1.25rem;
  border-radius: var(--radius-lg, 12px);
  border: 1px solid var(--color-border);
  background: linear-gradient(
    165deg,
    var(--color-bg-elevated) 0%,
    var(--color-bg-subtle, var(--color-bg-elevated)) 100%
  );
  box-shadow: var(--shadow-card, 0 4px 24px rgba(0, 0, 0, 0.12));
  box-sizing: border-box;
}

.knock-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 0.85rem;
}

.knock-status {
  display: inline-flex;
  align-items: center;
  gap: 0.45rem;
  padding: 0.25rem 0.65rem;
  border-radius: 999px;
  font-size: 0.78rem;
  font-weight: 600;
  text-transform: uppercase;
  background: var(--color-bg-subtle, rgba(255, 255, 255, 0.06));
  color: var(--color-text-muted);
}

.knock-status-dot {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 50%;
  background: currentColor;
}

.knock-status[data-mode="recording"] {
  color: var(--color-accent-hover, var(--color-accent));
}

.knock-status[data-mode="momentum"] {
  color: var(--color-warning-text, #e6a23c);
}

.knock-gear {
  width: 2.1rem;
  height: 2.1rem;
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-md, 8px);
  background: transparent;
  cursor: pointer;
}

.knock-gear--open {
  border-color: var(--color-accent);
  color: var(--color-accent);
}

.knock-metrics {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 0.5rem;
  margin-bottom: 0.85rem;
}

.knock-metric {
  padding: 0.55rem 0.65rem;
  border-radius: var(--radius-md, 8px);
  border: 1px solid var(--color-border);
  text-align: center;
}

.knock-metric--live {
  border-color: var(--color-accent);
}

.knock-metric-label {
  display: block;
  font-size: 0.68rem;
  text-transform: uppercase;
  color: var(--color-text-subtle);
}

.knock-metric-value {
  font-size: 1.05rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.knock-chart-wrap {
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md, 8px);
  margin-bottom: 0.75rem;
  overflow: hidden;
}

.knock-chart-wrap--spectrogram {
  min-height: 12rem;
}

.knock-canvas {
  display: block;
  width: 100%;
}

.knock-canvas--spectrogram {
  min-height: 12rem;
}

.knock-steps-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 1rem 1.25rem;
  align-items: start;
  margin-bottom: 1rem;
  padding-top: 0.5rem;
  border-top: 1px solid var(--color-border);
}

@media (max-width: 960px) {
  .knock-steps-row {
    grid-template-columns: 1fr;
  }
}

.knock-step {
  min-width: 0;
  margin-bottom: 0;
  padding-top: 0;
}

.knock-step-title {
  margin: 0 0 0.35rem;
  font-size: 0.92rem;
}

.knock-step-hint {
  margin: 0 0 0.6rem;
  font-size: 0.82rem;
  color: var(--color-text-muted);
}

.knock-step-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.knock-btn {
  padding: 0.55rem 0.85rem;
  border-radius: var(--radius-md, 8px);
  border: 1px solid var(--color-border-strong);
  font-weight: 600;
  font-size: 0.82rem;
  cursor: pointer;
}

.knock-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.knock-btn--primary {
  background: var(--color-accent);
  border-color: var(--color-accent);
  color: var(--color-on-accent, #111);
}

.knock-btn--secondary {
  background: transparent;
}

.knock-btn--stop {
  border-color: var(--color-danger, #c45);
  color: var(--color-danger, #c45);
}

.knock-settings {
  margin-top: 1rem;
  padding-top: 0.75rem;
  border-top: 1px solid var(--color-border);
}

.knock-settings-block + .knock-settings-block {
  margin-top: 1rem;
}

.knock-settings-title {
  margin: 0 0 0.5rem;
  font-size: 0.88rem;
}

.knock-field,
.knock-check {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  margin-bottom: 0.5rem;
  font-size: 0.85rem;
}

.knock-field input[type="number"] {
  max-width: 10rem;
}

.knock-link {
  background: none;
  border: none;
  color: var(--color-accent);
  cursor: pointer;
  font-size: 0.85rem;
  padding: 0;
}

.knock-note {
  margin: 0.5rem 0 0;
  font-size: 0.82rem;
  color: var(--color-text-muted);
}

.knock-note--warn {
  color: var(--color-warning-text, #e6a23c);
}

.knock-note--error {
  color: var(--color-danger, #e55);
}

.knock-note--ok {
  color: var(--color-success-text, #6ecf8a);
}

.knock-settings-enter-active,
.knock-settings-leave-active {
  transition: opacity 0.15s ease;
}

.knock-settings-enter-from,
.knock-settings-leave-to {
  opacity: 0;
}
</style>
