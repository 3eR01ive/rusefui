<script setup lang="ts">
import {
  computed,
  onMounted,
  onUnmounted,
  ref,
  watch,
} from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useDataContext } from "../../core/data-context";
import { initOutputChannels, useOutputChannels } from "../../composables/useOutputChannels";
import { initConfig, useConfig } from "../../composables/useConfig";
import { drawDynoChart, type DynoRunPoint } from "../../composables/drawDynoChart";
import { clampSmoothStrength, smoothDynoPoints } from "../../composables/smoothDynoCurve";
import {
  initProject,
  PERSIST_KEY_DYNO,
  projectUiEpoch,
  useProject,
  workspaceResetEpoch,
  type DynoUiSettings,
} from "../../composables/useProject";
import { useRustComponent } from "../../composables/useRustComponent";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const chartSizeOverride = { height: null as number | null };

const chartHeight = computed(() => {
  if (chartSizeOverride.height !== null && chartSizeOverride.height > 180) {
    return chartSizeOverride.height;
  }
  const h = Number(props.props.height ?? 320);
  return h >= 180 ? h : 320;
});

const { state, dispatch, error, hasLogic, ready } = useRustComponent(
  props.instance,
  props.path,
);
const dataCtx = useDataContext();
const { snapshot } = useOutputChannels();
const { snapshot: configSnapshot } = useConfig();
const { getProjectUi, setProjectUi } = useProject();

let applyingProjectUi = false;
let saveDynoUiTimer = 0;

const rpmField = computed(() => String(state.value.rpmField ?? props.props.rpmField ?? "RPMValue"));
const tpsField = computed(() => String(state.value.tpsField ?? props.props.tpsField ?? "TPSValue"));

const liveRpm = computed(() => snapshot.value.values[rpmField.value] ?? null);
const liveTps = computed(() => snapshot.value.values[tpsField.value] ?? null);

const recording = computed(() => Boolean(state.value.recording));
const runPoints = computed(
  () => (state.value.runPoints as DynoRunPoint[] | undefined) ?? [],
);
const currentTorque = computed(() => Number(state.value.currentTorque ?? 0));
const currentHp = computed(() => Number(state.value.currentHp ?? 0));
const message = computed(() => (state.value.message as string) ?? null);

const ignoreTpsMin = ref(false);
const minRpm = ref(0);

/** Сила сглаживания отображения (0 = выкл, только Vue/canvas). */
const smoothStrength = ref(0);

watch(smoothStrength, (v) => {
  const c = clampSmoothStrength(v);
  if (c !== v) {
    smoothStrength.value = c;
    return;
  }
  scheduleSaveDynoUiToProject();
});

const chartPoints = computed(() =>
  smoothDynoPoints(runPoints.value, smoothStrength.value),
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

const canStart = computed(
  () =>
    ready.value &&
    connected.value &&
    !recording.value &&
    configLoaded.value &&
    hasLogic.value,
);
const canStop = computed(() => recording.value);
const canClear = computed(() => runPoints.value.length > 0 && !recording.value);

function startRun() {
  return dispatch("start_run");
}

function stopRun() {
  return dispatch("stop_run");
}

function clearRun() {
  return dispatch("clear");
}

function buildDynoUiSettings(): DynoUiSettings {
  return {
    ignoreTpsMin: ignoreTpsMin.value,
    minRpm: Math.max(0, Math.round(minRpm.value)),
    smoothStrength: clampSmoothStrength(smoothStrength.value),
    chartHeight: chartHeight.value,
  };
}

async function syncOptionsToRust(): Promise<void> {
  if (!ready.value) return;
  await dispatch("set_options", {
    ignoreTpsMin: ignoreTpsMin.value,
    minRpm: Math.max(0, Math.round(minRpm.value)),
  });
}

async function applyDynoUiFromProject(): Promise<void> {
  applyingProjectUi = true;
  try {
    const ui = await getProjectUi<DynoUiSettings>(PERSIST_KEY_DYNO);
    ignoreTpsMin.value = ui.ignoreTpsMin;
    minRpm.value = ui.minRpm;
    smoothStrength.value = clampSmoothStrength(ui.smoothStrength);
    chartSizeOverride.height = ui.chartHeight > 180 ? ui.chartHeight : null;
  } catch {
    ignoreTpsMin.value = Boolean(state.value.ignoreTpsMin);
    minRpm.value = Number(state.value.minRpm ?? 0);
  } finally {
    applyingProjectUi = false;
  }
  await syncOptionsToRust();
  scheduleRedraw();
}

function scheduleSaveDynoUiToProject(): void {
  if (applyingProjectUi) return;
  if (saveDynoUiTimer !== 0) window.clearTimeout(saveDynoUiTimer);
  saveDynoUiTimer = window.setTimeout(() => {
    saveDynoUiTimer = 0;
    void setProjectUi(PERSIST_KEY_DYNO, buildDynoUiSettings());
  }, 400);
}

const canvasRef = ref<HTMLCanvasElement | null>(null);
const containerRef = ref<HTMLDivElement | null>(null);
const canvasWidth = ref(640);

function redraw(): void {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const dpr = window.devicePixelRatio || 1;
  const w = canvasWidth.value;
  const h = chartHeight.value;
  canvas.width = Math.floor(w * dpr);
  canvas.height = Math.floor(h * dpr);
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  drawDynoChart(ctx, w, h, chartPoints.value);
}

let redrawRaf = 0;
function scheduleRedraw(): void {
  cancelAnimationFrame(redrawRaf);
  redrawRaf = requestAnimationFrame(redraw);
}

let resizeObserver: ResizeObserver | undefined;

watch(
  () => configSnapshot.value.values,
  () => {
    if (!ready.value || recording.value) return;
    void dispatch("reload_config");
  },
  { deep: true },
);

watch(ready, (r) => {
  if (r) void applyDynoUiFromProject();
});

watch(projectUiEpoch, () => {
  void applyDynoUiFromProject();
});

watch(workspaceResetEpoch, () => {
  void applyDynoUiFromProject();
});

watch([ignoreTpsMin, minRpm], () => {
  if (applyingProjectUi) return;
  void syncOptionsToRust();
  scheduleSaveDynoUiToProject();
});

watch(chartHeight, () => {
  scheduleSaveDynoUiToProject();
  scheduleRedraw();
});

watch([runPoints, chartPoints, canvasWidth], () => scheduleRedraw(), { deep: true });

onMounted(async () => {
  await Promise.all([initOutputChannels(), initConfig(), initProject()]);
  scheduleRedraw();

  const el = containerRef.value;
  if (el && typeof ResizeObserver !== "undefined") {
    resizeObserver = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) {
        canvasWidth.value = Math.max(280, entry.contentRect.width);
      }
    });
    resizeObserver.observe(el);
  }
});

onUnmounted(() => {
  cancelAnimationFrame(redrawRaf);
  if (saveDynoUiTimer !== 0) window.clearTimeout(saveDynoUiTimer);
  resizeObserver?.disconnect();
});
</script>

<template>
  <div class="dyno-panel">
    <div class="dyno-toolbar">
      <button type="button" class="btn primary" :disabled="!canStart" @click="startRun">
        Start
      </button>
      <button type="button" class="btn secondary" :disabled="!canStop" @click="stopRun">
        Stop
      </button>
      <button type="button" class="btn ghost" :disabled="!canClear" @click="clearRun">
        Очистить
      </button>

      <div class="live-readouts">
        <span>RPM {{ liveRpm != null ? Math.round(liveRpm) : "—" }}</span>
        <span>TPS {{ liveTps != null ? liveTps.toFixed(1) : "—" }}%</span>
        <span v-if="recording || runPoints.length > 0">
          Tq {{ currentTorque.toFixed(1) }} Nm · HP {{ currentHp.toFixed(1) }}
        </span>
      </div>
    </div>

    <div class="dyno-options">
      <label class="option-check">
        <input v-model="ignoreTpsMin" type="checkbox" :disabled="recording" />
        <span>Без ограничения TPS</span>
      </label>
      <label class="option-rpm">
        <span>Мин. RPM</span>
        <input
          v-model.number="minRpm"
          type="number"
          min="0"
          max="20000"
          step="100"
          :disabled="recording"
          title="0 — не использовать; точки ниже порога не пишутся"
        />
      </label>
      <label class="option-smooth">
        <span>Сглаживание</span>
        <input
          v-model.number="smoothStrength"
          type="range"
          min="0"
          max="20"
          step="1"
          :disabled="runPoints.length < 3"
        />
        <span class="smooth-value">{{ clampSmoothStrength(smoothStrength) }}</span>
      </label>
      <p class="hint options-hint">
        Сглаживание только для графика (Vue): крайние точки фиксированы. 0 = сырая кривая.
      </p>
    </div>

    <p v-if="hasLogic && !ready && !error" class="message muted">Подключение к runtime…</p>

    <p v-if="!connected" class="message warn">Подключите ECU для live output.</p>
    <p v-else-if="!configLoaded" class="message warn">
      Загрузите config (проект или ECU) — параметры dyno из dynoChars.
    </p>
    <p
      v-if="message || error"
      class="message"
      :class="{ active: recording, muted: !recording, error: !!error }"
    >
      {{ error ?? message }}
    </p>

    <div ref="containerRef" class="dyno-chart-wrap">
      <canvas ref="canvasRef" class="dyno-canvas" />
    </div>

    <div v-if="runPoints.length > 0" class="peaks">
      <span>Пик Nm: <strong>{{ peakTorque.toFixed(1) }}</strong></span>
      <span>Пик HP: <strong>{{ peakHp.toFixed(1) }}</strong></span>
      <span>Точек: {{ runPoints.length }}</span>
      <span v-if="smoothStrength > 0" class="peaks-smooth-hint">(пики по сглаженной кривой)</span>
    </div>
  </div>
</template>

<style scoped>
.dyno-panel {
  display: grid;
  gap: 0.75rem;
  width: 100%;
}

.dyno-toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  align-items: center;
}

.btn {
  padding: 0.5rem 1rem;
  border-radius: var(--radius-md);
  border: 1px solid transparent;
  font-weight: 500;
}

.btn.primary {
  background: var(--color-accent);
  color: var(--color-on-accent);
}

.btn.secondary {
  background: var(--color-gray);
  color: var(--color-on-gray);
}

.btn.ghost {
  background: transparent;
  border-color: var(--color-border-strong);
  color: var(--color-text-muted);
}

.btn:disabled {
  opacity: 0.42;
  cursor: not-allowed;
}

.live-readouts {
  display: flex;
  flex-wrap: wrap;
  gap: 0.75rem;
  margin-left: auto;
  font-size: 0.88rem;
  color: var(--color-text-muted);
  font-variant-numeric: tabular-nums;
}

.dyno-options {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 1rem 1.25rem;
  padding: 0.65rem 0.85rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: var(--color-bg-elevated);
}

.option-check {
  display: flex;
  align-items: center;
  gap: 0.45rem;
  font-size: 0.88rem;
  color: var(--color-text);
  cursor: pointer;
}

.option-check input {
  width: 1rem;
  height: 1rem;
}

.option-rpm {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.88rem;
  color: var(--color-text-muted);
}

.option-rpm input {
  width: 5.5rem;
  padding: 0.35rem 0.5rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg);
  color: var(--color-text);
}

.option-smooth {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.88rem;
  color: var(--color-text-muted);
  flex: 1 1 12rem;
  min-width: 10rem;
}

.option-smooth input[type="range"] {
  flex: 1;
  min-width: 5rem;
}

.smooth-value {
  width: 1.5rem;
  text-align: right;
  font-variant-numeric: tabular-nums;
  color: var(--color-text);
}

.peaks-smooth-hint {
  font-size: 0.82rem;
  color: var(--color-text-subtle);
}

.options-hint {
  flex: 1 1 100%;
  margin: 0;
  font-size: 0.82rem;
  color: var(--color-text-subtle);
}

.message {
  margin: 0;
  font-size: 0.88rem;
}

.message.warn {
  color: var(--color-text-subtle);
}

.message.active {
  color: var(--color-success-text);
}

.message.muted {
  color: var(--color-text-muted);
}

.message.error {
  color: var(--color-error);
  background: var(--color-error-bg);
  padding: 0.5rem 0.65rem;
  border-radius: var(--radius-sm);
  border-left: 3px solid var(--color-accent);
}

.dyno-chart-wrap {
  width: 100%;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
  overflow: hidden;
}

.dyno-canvas {
  display: block;
  width: 100%;
}

.peaks {
  display: flex;
  flex-wrap: wrap;
  gap: 1.25rem;
  font-size: 0.9rem;
  color: var(--color-text-muted);
}
</style>
