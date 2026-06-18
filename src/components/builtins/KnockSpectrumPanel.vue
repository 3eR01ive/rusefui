<script setup lang="ts">
import {
  computed,
  nextTick,
  onMounted,
  onUnmounted,
  ref,
  watch,
  type Ref,
} from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useDataContext } from "../../core/data-context";
import { initConfig, useConfig } from "../../composables/useConfig";
import {
  b64ToArrayBuffer,
  knockSpectrogramU8ToDbfs,
  mountKnockSpectrogramGl,
  knockSpectrogramGlStats,
  type KnockSpectrogramGl,
} from "../../composables/knockSpectrogramGl";
import { buildKnockMarkerOverlay } from "../../composables/knockSpectrogramMarkers";
import {
  initKnockScope,
  onKnockSpectrogramGlReset,
  panKnockSpectrogram,
  setKnockSpectrogramFollowLive,
  formatKnockCaptureStats,
  refreshKnockScopeSnapshot,
  subscribeKnockSpectrogramGpu,
  useKnockScope,
} from "../../composables/useKnockScope";
import { invoke } from "@tauri-apps/api/core";
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
/** Общий id Rust-логики knock (делится с KnockThresholdPanel). */
const sharedLogicId = paramStringOr("logicId", "") || undefined;

const { state, dispatch, error, hasLogic, ready, mounting } = useRustComponent(
  props.instance,
  props.path,
  undefined,
  sharedLogicId,
);
const dataCtx = useDataContext();
const { snapshot: configSnapshot } = useConfig();
const {
  snapshot: knockScopeSnapshot,
  spectrogramWidth,
  spectrogramHeight,
  spectrogramPeakHz,
  spectrogramPatchPixelMax,
  spectrogramMarkers,
} = useKnockScope();
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
const recordingSpectrum = computed(
  () => recording.value && mode.value === "spectrumCapture",
);

const detectedHz = computed(() => state.value.detectedFrequencyHz as number | null | undefined);
const message = computed(() => (state.value.message as string) ?? null);
const momentumPhase = computed(() => String(state.value.momentumPhase ?? "idle"));

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
const displayFreq = useThrottledNumber(() => {
  const v = detectedHz.value;
  return v != null && Number.isFinite(Number(v)) ? Number(v) : null;
});

// --- Опции (спектрограмма + momentum knock) ---
const momentumSafeRpmMin = ref(2000);
const momentumSafeRpmMax = ref(3500);
const momentumMinLoad = ref(40);
const momentumAdvanceAddDeg = ref(6);
const momentumDurationMs = ref(800);
const spectrogramWindowMs = ref(500);
const spectrogramAutocontrast = ref(true);
const spectrogramGainPercent = ref(100);
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
const canToggleSpectrum = computed(() => recordingSpectrum.value || canRun.value);
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
    default:
      return "Готов";
  }
});

/** Поля, которыми владеет именно эта панель (для слияния при сохранении). */
function ownUiSettings(): Partial<KnockUiSettings> {
  return {
    momentumSafeRpmMin: Math.round(momentumSafeRpmMin.value),
    momentumSafeRpmMax: Math.round(momentumSafeRpmMax.value),
    momentumMinLoad: momentumMinLoad.value,
    momentumAdvanceAddDeg: momentumAdvanceAddDeg.value,
    momentumDurationMs: Math.round(momentumDurationMs.value),
    spectrogramWindowMs: Math.round(spectrogramWindowMs.value),
    spectrogramAutocontrast: spectrogramAutocontrast.value,
    spectrogramGainPercent: Math.round(spectrogramGainPercent.value),
    spectrumSettingsOpen: settingsOpen.value,
  };
}

async function syncOptionsToRust(): Promise<void> {
  if (!ready.value) return;
  await dispatch("set_options", {
    momentumSafeRpmMin: Math.round(momentumSafeRpmMin.value),
    momentumSafeRpmMax: Math.round(momentumSafeRpmMax.value),
    momentumMinLoad: momentumMinLoad.value,
    momentumAdvanceAddDeg: momentumAdvanceAddDeg.value,
    momentumDurationMs: Math.round(momentumDurationMs.value),
    spectrogramWindowMs: Math.round(spectrogramWindowMs.value),
  });
}

async function applyUiFromProject(): Promise<void> {
  const gen = ++applyUiGeneration;
  applyingProjectUi = true;
  try {
    const ui = await getProjectUi<KnockUiSettings>(PERSIST_KEY_KNOCK);
    if (gen !== applyUiGeneration) return;
    momentumSafeRpmMin.value = ui.momentumSafeRpmMin;
    momentumSafeRpmMax.value = ui.momentumSafeRpmMax;
    momentumMinLoad.value = ui.momentumMinLoad;
    momentumAdvanceAddDeg.value = ui.momentumAdvanceAddDeg;
    momentumDurationMs.value = ui.momentumDurationMs;
    spectrogramWindowMs.value = ui.spectrogramWindowMs;
    spectrogramAutocontrast.value = ui.spectrogramAutocontrast ?? true;
    spectrogramGainPercent.value = ui.spectrogramGainPercent ?? 100;
    settingsOpen.value = ui.spectrumSettingsOpen ?? false;
    pushSpectrogramDisplay();
  } catch {
    /* нет сохранённых настроек — оставляем дефолты */
  } finally {
    if (gen === applyUiGeneration) applyingProjectUi = false;
  }
  if (gen !== applyUiGeneration) return;
  await syncOptionsToRust();
  scheduleSpectrogramRedraw();
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

function toggleSettings(): void {
  settingsOpen.value = !settingsOpen.value;
  void flushUiToProject();
}

async function toggleSpectrumRun(): Promise<void> {
  if (recordingSpectrum.value) {
    await stopSpectrumRun();
  } else {
    spectrogramGl?.reset();
    await dispatch("start_spectrum_run");
    try {
      const b64 = await invoke<string>("knock_scope_gpu_buffer");
      if (b64) applySpectrogramGpuB64(b64);
    } catch {
      /* not in tauri */
    }
  }
}

async function stopSpectrumRun(): Promise<void> {
  await dispatch("stop_run", { applyThreshold: false });
  await refreshKnockScopeSnapshot();
  pushSpectrogramDisplay();
  scheduleSpectrogramRedraw();
}

// --- Спектрограмма (рендер — не трогать, оптимизировано по скорости) ---
const spectrogramCanvasRef = ref<HTMLCanvasElement | null>(null);
const spectrogramContainerRef = ref<HTMLDivElement | null>(null);
const knockStepsRowRef = ref<HTMLElement | null>(null);
const chartsInView = ref(true);
let spectrogramGl: KnockSpectrogramGl | null = null;
let unsubSpectrogramGpu: (() => void) | null = null;
let unsubSpectrogramReset: (() => void) | null = null;

const spectrogramDebugRef = ref<HTMLElement | null>(null);
let debugLineTimer = 0;
const spectrogramLayoutTick = ref(0);

const markerOverlay = computed(() => {
  void spectrogramLayoutTick.value;
  const wrapW = spectrogramContainerRef.value?.clientWidth ?? 0;
  const texW = spectrogramWidth.value || knockSpectrogramGlStats.value.texW;
  return buildKnockMarkerOverlay(spectrogramMarkers.value, texW, wrapW);
});

const spectrogramHint = computed(() => {
  const s = knockScopeSnapshot.value;
  if (s.lastError) return s.lastError;
  if (s.statusMessage) return s.statusMessage;
  if (recordingSpectrum.value && spectrogramWidth.value < 1) {
    return "Ждём захват knock scope и FFT…";
  }
  return null;
});

function bindSpectrogramGl(): void {
  spectrogramGl?.destroy();
  const canvas = spectrogramCanvasRef.value;
  spectrogramGl = canvas ? mountKnockSpectrogramGl(canvas) : null;
  pushSpectrogramDisplay();
}

function applySpectrogramGpuB64(b64: string): void {
  if (!spectrogramGl) return;
  pushSpectrogramDisplay();
  spectrogramGl.applyBuffer(b64ToArrayBuffer(b64));
  if (chartsInView.value) spectrogramGl.draw();
  scheduleDebugLineUpdate();
}

function redrawSpectrogramChart(): void {
  if (!chartsInView.value) return;
  if (!spectrogramContainerRef.value || chartWidthPx(spectrogramContainerRef.value) < 1) return;
  spectrogramGl?.draw();
  scheduleDebugLineUpdate();
}

function chartWidthPx(container: HTMLElement | null | undefined): number {
  if (!container) return 0;
  const w = container.clientWidth;
  return w > 0 ? Math.floor(w) : 0;
}

function pushSpectrogramDisplay(): void {
  spectrogramGl?.setDisplay({
    autocontrast: spectrogramAutocontrast.value,
    gainPercent: spectrogramGainPercent.value,
  });
  const texW = spectrogramWidth.value || knockSpectrogramGlStats.value.texW;
  spectrogramGl?.setMarkers(spectrogramMarkers.value, texW);
}

function redrawSpectrogramNow(): void {
  pushSpectrogramDisplay();
  if (!spectrogramContainerRef.value || chartWidthPx(spectrogramContainerRef.value) < 1) return;
  spectrogramGl?.draw();
}

let spectrogramRedrawRaf = 0;
let lastSpectrogramDrawMs = 0;
const SPECTROGRAM_REDRAW_MIN_MS = 50;

function scheduleSpectrogramRedraw(): void {
  if (!tabActive.value || !chartsInView.value) return;
  if (spectrogramRedrawRaf !== 0) return;
  const tick = () => {
    spectrogramRedrawRaf = 0;
    const now = performance.now();
    if (now - lastSpectrogramDrawMs < SPECTROGRAM_REDRAW_MIN_MS) {
      spectrogramRedrawRaf = requestAnimationFrame(tick);
      return;
    }
    lastSpectrogramDrawMs = now;
    redrawSpectrogramChart();
  };
  spectrogramRedrawRaf = requestAnimationFrame(tick);
}

const spectrogramDirty = computed(
  () => recordingSpectrum.value || spectrogramWidth.value > 0,
);

function buildSpectrogramDebugLine(): string {
  const s = knockScopeSnapshot.value;
  const g = knockSpectrogramGlStats.value;
  const parts: string[] = [];
  parts.push(formatKnockCaptureStats(s, spectrogramWindowMs.value));
  parts.push(`rustMax=${spectrogramPatchPixelMax.value}`);
  if (g.uploads > 0) {
    parts.push(`${g.packetKind} px=${g.pixelMin}…${g.pixelMax}`);
    parts.push(`disp ${g.displayMinU8}…${g.displayMaxU8} ×${g.displayGainScale.toFixed(2)}`);
    parts.push(
      `dBFS ${Math.round(knockSpectrogramU8ToDbfs(g.pixelMin))}…${Math.round(knockSpectrogramU8ToDbfs(g.pixelMax))}`,
    );
    if (g.texW > 0) parts.push(`tex=${g.texW}×${g.texH} nz=${g.nonzeroPixels}`);
  }
  if (spectrogramPeakHz.value != null) {
    parts.push(`FFT-пик ${Math.round(spectrogramPeakHz.value)} Hz`);
  }
  const cyl = knockScopeSnapshot.value.lastCylinder;
  if (cyl != null) {
    parts.push(`цил ${cyl + 1}`);
  }
  return parts.join(" · ");
}

function scheduleDebugLineUpdate(): void {
  if (debugLineTimer !== 0) return;
  debugLineTimer = window.setTimeout(() => {
    debugLineTimer = 0;
    const el = spectrogramDebugRef.value;
    if (el) el.textContent = buildSpectrogramDebugLine();
  }, 300);
}

function onSpectrogramWheel(e: WheelEvent): void {
  const s = knockScopeSnapshot.value;
  if ((s.captureCount ?? 0) < 1 && !recordingSpectrum.value) return;
  const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
  if (delta === 0) return;
  e.preventDefault();
  const cols = Math.max(1, Math.round(delta / 12));
  void panKnockSpectrogram(delta > 0 ? cols : -cols);
}

function onSpectrogramDblClick(): void {
  void setKnockSpectrogramFollowLive(true);
}

// --- Watchers / observers ---
watch(ready, (r) => {
  if (r) void applyUiFromProject();
});
watch(workspaceResetEpoch, () => void applyUiFromProject());

watch(
  [
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

watch([spectrogramAutocontrast, spectrogramGainPercent], () => {
  if (applyingProjectUi) return;
  scheduleSaveUiToProject();
  redrawSpectrogramNow();
});

watch(
  [chartHeight, spectrogramDirty],
  () => {
    if (spectrogramDirty.value) scheduleSpectrogramRedraw();
  },
);

watch(spectrogramCanvasRef, (canvas) => {
  if (canvas) {
    bindSpectrogramGl();
    if (spectrogramDirty.value) scheduleSpectrogramRedraw();
  } else {
    spectrogramGl?.destroy();
    spectrogramGl = null;
  }
});

watch(tabActive, (active, was) => {
  if (active && !was) void nextTick(() => scheduleSpectrogramRedraw());
});

watch(chartsInView, (visible) => {
  if (visible) scheduleSpectrogramRedraw();
});

watch(
  [spectrogramWidth, spectrogramHeight, spectrogramPatchPixelMax, spectrogramPeakHz, spectrogramMarkers, recordingSpectrum],
  () => scheduleDebugLineUpdate(),
);

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
  const el = spectrogramContainerRef.value;
  if (!el || typeof ResizeObserver === "undefined") return;
  let lastW = 0;
  resizeObserver = new ResizeObserver(() => {
    if (!tabActive.value) return;
    spectrogramLayoutTick.value += 1;
    const w = el.clientWidth;
    if (w < 1) return;
    if (Math.abs(w - lastW) < 1) return;
    lastW = w;
    if (spectrogramDirty.value) scheduleSpectrogramRedraw();
  });
  resizeObserver.observe(el);
}

watch([ready, spectrogramContainerRef, knockStepsRowRef], async () => {
  if (!ready.value) return;
  await nextTick();
  setupResizeObserver();
  setupChartsIntersection();
  if (spectrogramDirty.value) scheduleSpectrogramRedraw();
});

let unregUiFlush: (() => void) | null = null;

onMounted(async () => {
  await Promise.all([initConfig(), initProject(), initKnockScope()]);
  unregUiFlush = registerProjectUiFlushHook(flushUiToProject);
  await nextTick();
  bindSpectrogramGl();
  unsubSpectrogramGpu = subscribeKnockSpectrogramGpu((b64) => {
    if (!tabActive.value) return;
    applySpectrogramGpuB64(b64);
  });
  unsubSpectrogramReset = onKnockSpectrogramGlReset(() => spectrogramGl?.reset());
  try {
    const b64 = await invoke<string>("knock_scope_gpu_buffer");
    if (b64) applySpectrogramGpuB64(b64);
  } catch {
    /* not in tauri */
  }
});

onUnmounted(() => {
  unregUiFlush?.();
  unsubSpectrogramGpu?.();
  unsubSpectrogramGpu = null;
  unsubSpectrogramReset?.();
  unsubSpectrogramReset = null;
  spectrogramGl?.destroy();
  spectrogramGl = null;
  cancelAnimationFrame(spectrogramRedrawRaf);
  resizeObserver?.disconnect();
  chartsIo?.disconnect();
  if (debugLineTimer !== 0) window.clearTimeout(debugLineTimer);
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
        <div class="knock-metric" :class="{ 'knock-metric--live': displayFreq != null }">
          <span class="knock-metric-label">Freq</span>
          <span class="knock-metric-value">
            {{ displayFreq != null ? `${Math.round(displayFreq)} Hz` : "—" }}
          </span>
        </div>
      </div>

      <div ref="knockStepsRowRef" class="knock-steps-row">
        <section class="knock-step knock-step--spectrum">
          <header class="knock-step-header">
            <h3 class="knock-step-title">Частота детонации</h3>
            <p class="knock-step-hint">
              Спектрограмма на прогоне и Momentum knock в безопасной зоне.
            </p>
          </header>
          <div
            ref="spectrogramContainerRef"
            class="knock-chart-wrap knock-chart-wrap--spectrogram"
            :style="{ height: `${chartHeight}px` }"
            @wheel.prevent="onSpectrogramWheel"
            @dblclick="onSpectrogramDblClick"
          >
            <canvas ref="spectrogramCanvasRef" class="knock-canvas knock-canvas--spectrogram" />
            <div class="knock-spectrogram-markers" aria-hidden="true">
              <div
                v-for="(mk, i) in markerOverlay"
                :key="`cyl-${mk.cylinder}-${mk.x}-${i}`"
                class="knock-spectrogram-marker"
                :style="{ left: `${mk.x}px` }"
              >
                <span class="knock-spectrogram-marker-label">{{ mk.label }}</span>
              </div>
            </div>
            <div class="knock-spectrogram-labels" aria-hidden="true">
              <span class="knock-spectrogram-label knock-spectrogram-label--y">Frequency (kHz)</span>
              <span class="knock-spectrogram-label knock-spectrogram-label--x">Time (s)</span>
              <span class="knock-spectrogram-label knock-spectrogram-label--db">dBFS</span>
              <span class="knock-spectrogram-label knock-spectrogram-label--hz0">DC</span>
              <span class="knock-spectrogram-label knock-spectrogram-label--hz5">5</span>
              <span class="knock-spectrogram-label knock-spectrogram-label--hz10">10</span>
              <span class="knock-spectrogram-label knock-spectrogram-label--hz15">15</span>
              <span class="knock-spectrogram-label knock-spectrogram-label--hz20">20</span>
            </div>
            <p v-if="spectrogramHint" class="knock-chart-overlay">{{ spectrogramHint }}</p>
          </div>
          <p
            v-if="recordingSpectrum || spectrogramWidth > 0"
            ref="spectrogramDebugRef"
            class="knock-spectrogram-debug"
          />
          <div class="knock-step-actions">
            <button
              type="button"
              class="knock-btn knock-btn--toggle"
              :class="{ 'knock-btn--toggle-recording': recordingSpectrum }"
              :disabled="!canToggleSpectrum"
              @click="toggleSpectrumRun"
            >
              {{ recordingSpectrum ? "Стоп запись" : "Запись спектрограммы (прогон)" }}
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
          </div>
        </section>
      </div>

      <Transition name="knock-settings">
        <section v-if="settingsOpen" class="knock-settings">
          <div class="knock-settings-groups">
            <div class="knock-settings-group">
              <h4 class="knock-settings-title">Спектрограмма</h4>
              <div class="knock-settings-fields">
                <label class="knock-field knock-field--wide">
                  <span>Окно спектрограммы, ms</span>
                  <input
                    v-model.number="spectrogramWindowMs"
                    type="number"
                    min="100"
                    max="3000"
                    step="50"
                  />
                </label>
                <label class="knock-field knock-field--check">
                  <span>Автоконтраст</span>
                  <input v-model="spectrogramAutocontrast" type="checkbox" />
                </label>
                <label class="knock-field">
                  <span>Яркость, %</span>
                  <input
                    v-model.number="spectrogramGainPercent"
                    type="range"
                    min="1"
                    max="400"
                    step="1"
                  />
                  <span class="knock-field-hint">{{ spectrogramGainPercent }}%</span>
                </label>
                <label class="knock-field">
                  <span>Яркость (точно)</span>
                  <input
                    v-model.number="spectrogramGainPercent"
                    type="number"
                    min="1"
                    max="400"
                    step="1"
                  />
                </label>
              </div>
            </div>

            <div class="knock-settings-group">
              <h4 class="knock-settings-title">Momentum knock</h4>
              <div class="knock-settings-fields">
                <label class="knock-field">
                  <span>Безоп. RPM min</span>
                  <input
                    v-model.number="momentumSafeRpmMin"
                    type="number"
                    min="500"
                    max="20000"
                    step="100"
                  />
                </label>
                <label class="knock-field">
                  <span>Безоп. RPM max</span>
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
                <label class="knock-field knock-field--wide">
                  <span>Длительность, ms</span>
                  <input
                    v-model.number="momentumDurationMs"
                    type="number"
                    min="100"
                    max="5000"
                    step="50"
                  />
                </label>
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

.knock-chart-wrap--spectrogram {
  min-height: 0;
  background: #000;
}

.knock-canvas--spectrogram {
  display: block;
  width: 100%;
  height: 100%;
}

.knock-spectrogram-markers {
  position: absolute;
  inset: 0;
  pointer-events: none;
  overflow: hidden;
  z-index: 2;
}

.knock-spectrogram-marker {
  position: absolute;
  top: 12px;
  bottom: 32px;
  width: 0;
}

.knock-spectrogram-marker-label {
  position: absolute;
  left: 4px;
  top: 0;
  padding: 1px 4px;
  font-size: 10px;
  font-weight: 600;
  line-height: 1.2;
  color: #fff;
  background: rgba(0, 0, 0, 0.65);
  border: 1px solid rgba(255, 255, 255, 0.35);
  border-radius: 3px;
  white-space: nowrap;
}

.knock-spectrogram-labels {
  position: absolute;
  inset: 0;
  pointer-events: none;
  font: 11px ui-monospace, SFMono-Regular, Menlo, monospace;
  color: #fff;
}

.knock-spectrogram-label {
  position: absolute;
}

.knock-spectrogram-label--y {
  left: 6px;
  top: 50%;
  transform: rotate(-90deg) translateX(-50%);
  transform-origin: left center;
}

.knock-spectrogram-label--x {
  left: 52px;
  right: 56px;
  bottom: 4px;
  text-align: center;
}

.knock-spectrogram-label--db {
  right: 8px;
  bottom: 18px;
}

.knock-spectrogram-label--hz0 { left: 38px; bottom: 36px; }
.knock-spectrogram-label--hz5 { left: 38px; bottom: calc(36px + 25%); }
.knock-spectrogram-label--hz10 { left: 34px; bottom: calc(36px + 50%); }
.knock-spectrogram-label--hz15 { left: 34px; bottom: calc(36px + 75%); }
.knock-spectrogram-label--hz20 { left: 34px; top: 16px; }

.knock-canvas {
  display: block;
}

.knock-spectrogram-debug {
  margin: 0;
  font-size: 11px;
  font-family: ui-monospace, monospace;
  color: var(--color-text-muted);
  line-height: 1.4;
  word-break: break-word;
}

.knock-chart-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  margin: 0;
  padding: 0 1rem;
  text-align: center;
  font-size: 0.82rem;
  color: var(--color-text-muted);
  pointer-events: none;
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

.knock-field--wide {
  grid-column: 1 / -1;
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

.knock-field-hint {
  margin: 0;
  font-size: 0.82rem;
  color: var(--color-text-muted);
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
