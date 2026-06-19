<script setup lang="ts">
import {
  computed,
  inject,
  nextTick,
  onMounted,
  onUnmounted,
  ref,
  watch,
  type Ref,
} from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useDataContext } from "../../core/data-context";
import { configDataRevision, initConfig, useConfig } from "../../composables/useConfig";
import {
  drawKnockThresholdChart,
  type KnockRpmValuePoint,
  type KnockRunPoint,
} from "../../composables/drawKnockChart";
import {
  initProject,
  PERSIST_KEY_KNOCK,
  registerProjectUiFlushHook,
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

const chartHeight = computed(() => {
  const h = Number(props.props.height ?? 360);
  return h >= CHART_HEIGHT_MIN ? h : 360;
});

const instanceRef = computed(() => props.instance);
const { paramStringOr } = useInstanceBind(instanceRef);
/** Общий id Rust-логики knock (делится с KnockSpectrumPanel). */
const sharedLogicId = paramStringOr("logicId", "") || undefined;

const { state, dispatch, error, hasLogic, ready, mounting } = useRustComponent(
  props.instance,
  props.path,
  undefined,
  sharedLogicId,
);
const dataCtx = useDataContext();
const { snapshot: configSnapshot, getArray: getConfigArray } = useConfig();
const { isActive: tabActive } = useTabActivity();
const { getProjectUi, setProjectUi } = useProject();

let applyingProjectUi = false;
let saveUiTimer = 0;
let applyUiGeneration = 0;
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
const recordingThreshold = computed(
  () => recording.value && mode.value === "thresholdAutotune",
);

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

const METRIC_UI_MS = 250;

function useThrottledNumber(source: () => number | null | undefined): Ref<number | null> {
  const out = ref<number | null>(null) as Ref<number | null>;
  let timer = 0;
  const flush = () => {
    timer = 0;
    const v = source();
    out.value = v != null && Number.isFinite(Number(v)) ? Number(v) : null;
  };
  watch(source, () => {
    if (timer !== 0) return;
    timer = window.setTimeout(flush, METRIC_UI_MS);
  }, { immediate: true });
  return out;
}

const displayRpm = useThrottledNumber(() => liveRpm.value);
const displayLevel = useThrottledNumber(() => liveLevel.value);
const displayThreshold = useThrottledNumber(() => configThresholdLive.value);

const message = computed(() => (state.value.message as string) ?? null);

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

// --- Опции (threshold + бездетон) ---
const ignoreTpsMin = ref(true);
const minRpm = ref(800);
const cutoffRpm = ref(6500);
const thresholdGapDb = ref(3);
const tempTargetLambda = ref(0.85);
const tempIgnitionRetardDeg = ref(8);
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
const canToggleThreshold = computed(
  () =>
    recordingThreshold.value ||
    (ready.value && connected.value && configLoaded.value && hasLogic.value),
);
const canClear = computed(
  () =>
    !recording.value &&
    (runPoints.value.length > 0 || previousRunPoints.value.length > 0),
);

const statusMode = computed(() => {
  if (recording.value) return "recording";
  if (!connected.value) return "offline";
  if (!configLoaded.value) return "noconfig";
  if (runPoints.value.length > 0) return "done";
  return "idle";
});

const statusLabel = computed(() => {
  switch (statusMode.value) {
    case "recording":
      return "Прогон";
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

/** Поля, которыми владеет именно эта панель (для слияния при сохранении). */
function ownUiSettings(): Partial<KnockUiSettings> {
  return {
    ignoreTpsMin: ignoreTpsMin.value,
    minRpm: Math.max(0, Math.round(minRpm.value)),
    cutoffRpm: Math.max(0, Math.round(cutoffRpm.value)),
    thresholdGapDb: thresholdGapDb.value,
    tempTargetLambda: tempTargetLambda.value,
    tempIgnitionRetardDeg: tempIgnitionRetardDeg.value,
    thresholdSettingsOpen: settingsOpen.value,
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
  });
}

async function applyUiFromProject(): Promise<void> {
  const gen = ++applyUiGeneration;
  applyingProjectUi = true;
  try {
    const ui = await getProjectUi<KnockUiSettings>(PERSIST_KEY_KNOCK);
    if (gen !== applyUiGeneration) return;
    ignoreTpsMin.value = ui.ignoreTpsMin;
    minRpm.value = ui.minRpm;
    cutoffRpm.value = ui.cutoffRpm;
    thresholdGapDb.value = ui.thresholdGapDb;
    tempTargetLambda.value = ui.tempTargetLambda;
    tempIgnitionRetardDeg.value = ui.tempIgnitionRetardDeg;
    settingsOpen.value = ui.thresholdSettingsOpen ?? false;
  } catch {
    if (gen !== applyUiGeneration) return;
    ignoreTpsMin.value = Boolean(state.value.ignoreTpsMin);
    minRpm.value = Number(state.value.minRpm ?? 800);
    cutoffRpm.value = Number(state.value.cutoffRpm ?? 6500);
  } finally {
    if (gen === applyUiGeneration) applyingProjectUi = false;
  }
  if (gen !== applyUiGeneration) return;
  await syncOptionsToRust();
  scheduleThresholdRedraw();
}

async function flushUiToProject(): Promise<void> {
  if (saveUiTimer !== 0) {
    window.clearTimeout(saveUiTimer);
    saveUiTimer = 0;
  }
  if (applyingProjectUi) return;
  let existing: Partial<KnockUiSettings> = {};
  try {
    existing = await getProjectUi<KnockUiSettings>(PERSIST_KEY_KNOCK);
  } catch {
    /* первый запуск */
  }
  await setProjectUi(PERSIST_KEY_KNOCK, { ...existing, ...ownUiSettings() });
}

function scheduleSaveUiToProject(): void {
  if (applyingProjectUi) return;
  if (saveUiTimer !== 0) window.clearTimeout(saveUiTimer);
  saveUiTimer = window.setTimeout(() => {
    saveUiTimer = 0;
    void flushUiToProject();
  }, 400);
}

/** Поднять окно canvas над соседями, пока открыты настройки (см. CanvasWindow). */
const setWindowElevated = inject<((on: boolean) => void) | null>("cwSetElevated", null);
watch(settingsOpen, (open) => setWindowElevated?.(open), { immediate: true });

function toggleSettings(): void {
  settingsOpen.value = !settingsOpen.value;
  void flushUiToProject();
}

async function toggleThresholdAutotune(): Promise<void> {
  if (recordingThreshold.value) {
    await stopThresholdRun();
  } else {
    await dispatch("start_threshold_autotune");
  }
}

async function stopThresholdRun(): Promise<void> {
  await dispatch("stop_run", { applyThreshold: true });
  await reloadConfigThresholdCurve();
  scheduleThresholdRedraw();
}

// --- График порога (рендер — не трогать) ---
const thresholdCanvasRef = ref<HTMLCanvasElement | null>(null);
const thresholdContainerRef = ref<HTMLDivElement | null>(null);
let thresholdCanvasPixelW = 0;
let thresholdCanvasPixelH = 0;
const knockStepsRowRef = ref<HTMLElement | null>(null);
const chartsInView = ref(true);

function chartWidthPx(container: HTMLElement | null | undefined): number {
  if (!container) return 0;
  const w = container.clientWidth;
  return w > 0 ? Math.floor(w) : 0;
}

function redrawThresholdChart(): void {
  const canvas = thresholdCanvasRef.value;
  const container = thresholdContainerRef.value;
  if (!canvas || !container) return;
  const dpr = window.devicePixelRatio || 1;
  const w = chartWidthPx(container);
  const h = chartHeight.value;
  if (w < 1 || h < 1) return;
  const pixelW = Math.floor(w * dpr);
  const pixelH = Math.floor(h * dpr);
  if (pixelW !== thresholdCanvasPixelW || pixelH !== thresholdCanvasPixelH) {
    canvas.width = pixelW;
    canvas.height = pixelH;
    canvas.style.height = `${h}px`;
    thresholdCanvasPixelW = pixelW;
    thresholdCanvasPixelH = pixelH;
  }
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  drawKnockThresholdChart(ctx, w, h, thresholdChartCurve.value, knockLevelChartPoints.value, {
    previousRunLevels: previousKnockLevelChartPoints.value,
    liveRpm: liveRpm.value,
    liveLevel: liveLevel.value,
    baselineThreshold: recordingThreshold.value ? configThresholdCurve.value : [],
    thresholdGapDb: thresholdGapDb.value,
    recording: recordingThreshold.value,
  });
}

let thresholdRedrawRaf = 0;
let lastThresholdDrawMs = 0;
const THRESHOLD_REDRAW_MIN_MS = 50;

function scheduleThresholdRedraw(): void {
  if (!tabActive.value || !chartsInView.value) return;
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

// --- Watchers / observers ---
watch(ready, (r) => {
  if (r) void applyUiFromProject();
});
watch(workspaceResetEpoch, () => void applyUiFromProject());
watch(configLoaded, (loaded) => {
  if (loaded) void reloadConfigThresholdCurve();
});
watch(configDataRevision, () => {
  if (configLoaded.value) void reloadConfigThresholdCurve();
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
  ],
  () => {
    if (applyingProjectUi) return;
    void syncOptionsToRust();
    scheduleSaveUiToProject();
  },
);

watch(
  [
    liveLevel,
    liveRpm,
    thresholdChartCurve,
    knockLevelChartPoints,
    chartHeight,
    recordingThreshold,
    configThresholdCurve,
  ],
  () => scheduleThresholdRedraw(),
);

watch(tabActive, (active, was) => {
  if (active && !was) void nextTick(() => scheduleThresholdRedraw());
});

watch(chartsInView, (visible) => {
  if (visible) scheduleThresholdRedraw();
});

let chartsIo: IntersectionObserver | undefined;

function setupChartsIntersection(): void {
  chartsIo?.disconnect();
  chartsIo = undefined;
  const el = knockStepsRowRef.value;
  if (!el || typeof IntersectionObserver === "undefined") {
    chartsInView.value = true;
    return;
  }
  chartsIo = new IntersectionObserver(
    (entries) => {
      chartsInView.value = entries.some((e) => e.isIntersecting && e.intersectionRatio > 0.02);
    },
    { threshold: [0, 0.02, 0.08] },
  );
  chartsIo.observe(el);
}

let resizeObserver: ResizeObserver | undefined;

function setupResizeObserver(): void {
  resizeObserver?.disconnect();
  resizeObserver = undefined;
  const el = thresholdContainerRef.value;
  if (!el || typeof ResizeObserver === "undefined") return;
  let lastW = 0;
  resizeObserver = new ResizeObserver(() => {
    if (!tabActive.value) return;
    const w = el.clientWidth;
    if (w < 1) return;
    if (Math.abs(w - lastW) < 1) return;
    lastW = w;
    scheduleThresholdRedraw();
  });
  resizeObserver.observe(el);
}

watch([ready, thresholdContainerRef, knockStepsRowRef], async () => {
  if (!ready.value) return;
  await nextTick();
  setupResizeObserver();
  setupChartsIntersection();
  scheduleThresholdRedraw();
});

let unregUiFlush: (() => void) | null = null;

onMounted(async () => {
  await Promise.all([initConfig(), initProject()]);
  unregUiFlush = registerProjectUiFlushHook(flushUiToProject);
  await nextTick();
  if (configLoaded.value) await reloadConfigThresholdCurve();
});

onUnmounted(() => {
  unregUiFlush?.();
  cancelAnimationFrame(thresholdRedrawRaf);
  resizeObserver?.disconnect();
  chartsIo?.disconnect();
  if (saveUiTimer !== 0) window.clearTimeout(saveUiTimer);
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
            {{ displayRpm != null ? Math.round(displayRpm).toLocaleString("ru-RU") : "—" }}
          </span>
        </div>
        <div class="knock-metric knock-metric--live">
          <span class="knock-metric-label">Knock</span>
          <span class="knock-metric-value">
            {{ displayLevel != null ? displayLevel.toFixed(2) : "—" }}
          </span>
        </div>
        <div class="knock-metric">
          <span class="knock-metric-label">Threshold (config)</span>
          <span class="knock-metric-value">
            {{ displayThreshold != null ? displayThreshold.toFixed(2) : "—" }}
          </span>
        </div>
      </div>

      <div ref="knockStepsRowRef" class="knock-steps-row">
        <section class="knock-step knock-step--threshold">
          <header class="knock-step-header">
            <h3 class="knock-step-title">Threshold autotune</h3>
            <p class="knock-step-hint">
              Шум по RPM-бинам, порог autotune (peak + Δ) и зазор до knockBaseNoise.
            </p>
          </header>
          <div
            ref="thresholdContainerRef"
            class="knock-chart-wrap"
            :style="{ height: `${chartHeight}px` }"
          >
            <canvas ref="thresholdCanvasRef" class="knock-canvas" />
          </div>
          <div class="knock-step-actions">
            <button
              type="button"
              class="knock-btn knock-btn--toggle"
              :class="{ 'knock-btn--toggle-recording': recordingThreshold }"
              :disabled="!canToggleThreshold"
              @click="toggleThresholdAutotune"
            >
              {{ recordingThreshold ? "Стоп autotune" : "Старт Threshold Autotune" }}
            </button>
            <button
              type="button"
              class="knock-btn knock-btn--secondary"
              :disabled="!canRun"
              @click="dispatch('apply_temp_detune')"
            >
              Временные бездетоновые настройки
            </button>
          </div>
        </section>
      </div>

      <Transition name="knock-settings">
        <section v-if="settingsOpen" class="knock-settings">
          <div class="knock-settings-groups">
            <div class="knock-settings-group">
              <h4 class="knock-settings-title">Прогон</h4>
              <div class="knock-settings-fields">
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
              </div>
            </div>

            <div class="knock-settings-group">
              <h4 class="knock-settings-title">Бездетоновые настройки</h4>
              <div class="knock-settings-fields">
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
                <div class="knock-settings-tools">
                  <button type="button" class="knock-link" :disabled="!canClear" @click="dispatch('clear')">
                    Очистить график
                  </button>
                </div>
              </div>
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
  max-width: none;
  box-sizing: border-box;
  padding: 1.15rem 1.25rem 1.25rem;
  border-radius: var(--radius-lg, 12px);
  border: 1px solid var(--color-border);
  background: linear-gradient(
    165deg,
    var(--color-bg-elevated) 0%,
    var(--color-bg-subtle, var(--color-bg-elevated)) 100%
  );
  contain: layout style;
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
  grid-template-columns: repeat(3, 1fr);
  gap: 0.5rem;
  margin-bottom: 0.85rem;
  content-visibility: auto;
  contain-intrinsic-size: auto 3.5rem;
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
  position: relative;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md, 8px);
  margin-bottom: 0.75rem;
  overflow: hidden;
  background: var(--color-surface-2, #1a1d24);
  contain: strict;
  isolation: isolate;
  transform: translateZ(0);
}

.knock-canvas {
  display: block;
}

.knock-steps-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 1rem 1.25rem;
  align-items: stretch;
  margin-bottom: 1rem;
  padding-top: 0.5rem;
  border-top: 1px solid var(--color-border);
  contain: layout paint;
}

.knock-step {
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}

.knock-step-header {
  min-height: 3.25rem;
  margin-bottom: 0.5rem;
}

.knock-step-title {
  margin: 0 0 0.25rem;
  font-size: 0.92rem;
}

.knock-step-hint {
  margin: 0;
  font-size: 0.78rem;
  line-height: 1.35;
  color: var(--color-text-muted);
}

.knock-step-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  margin-top: auto;
  padding-top: 0.15rem;
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

.knock-btn--toggle {
  background: var(--color-accent);
  border-color: var(--color-accent);
  color: var(--color-on-accent, #111);
}

.knock-btn--toggle-recording {
  background: var(--color-gray, #666);
  border-color: var(--color-gray, #666);
  color: var(--color-on-gray, #fff);
}

.knock-btn--secondary {
  background: transparent;
}

.knock-settings {
  margin-top: 1rem;
  padding-top: 0.85rem;
  border-top: 1px solid var(--color-border);
}

.knock-settings-groups {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(16rem, 1fr));
  gap: 0.85rem;
}

.knock-settings-group {
  padding: 0.85rem 0.95rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md, 8px);
  background: var(--color-bg-subtle, rgba(255, 255, 255, 0.03));
}

.knock-settings-title {
  margin: 0 0 0.65rem;
  font-size: 0.72rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-text-muted);
}

.knock-settings-fields {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0.55rem 0.65rem;
}

.knock-field,
.knock-check {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  font-size: 0.82rem;
}

.knock-check {
  grid-column: 1 / -1;
  flex-direction: row;
  align-items: center;
  gap: 0.5rem;
  cursor: pointer;
}

.knock-check input {
  width: 1rem;
  height: 1rem;
}

.knock-field > span:first-child {
  font-size: 0.68rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-text-subtle);
}

.knock-field input[type="number"] {
  width: 100%;
  max-width: none;
  padding: 0.4rem 0.5rem;
  border-radius: var(--radius-md, 8px);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 0.85rem;
  box-sizing: border-box;
}

.knock-settings-tools {
  grid-column: 1 / -1;
  display: flex;
  align-items: center;
  padding-top: 0.15rem;
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

.knock-hint {
  margin: 0;
  font-size: 0.85rem;
  color: var(--color-text-muted);
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
