<script setup lang="ts">
import {
  computed,
  onMounted,
  onUnmounted,
  ref,
  shallowRef,
  watch,
} from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useDataContext } from "../../core/data-context";
import { initOutputChannels, useOutputChannels } from "../../composables/useOutputChannels";
import { initConfig, useConfig } from "../../composables/useConfig";
import { drawDynoChart } from "../../composables/drawDynoChart";
import {
  DynoView,
  dynoConfigFromValues,
  DEFAULT_DYNO_RUN_OPTIONS,
  type DynoRunOptions,
  type DynoRunPoint,
} from "../../lib/dynoView";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const chartHeight = computed(() => {
  const h = Number(props.props.height ?? 320);
  return h >= 180 ? h : 320;
});

const dataCtx = useDataContext();
const { snapshot } = useOutputChannels();
const { snapshot: configSnapshot, getField: getConfigField } = useConfig();

const connected = computed(() => dataCtx.connection.value.connected);
const rpmField = computed(() => String(props.props.rpmField ?? "RPMValue"));
const tpsField = computed(() => String(props.props.tpsField ?? "TPSValue"));

const liveRpm = computed(() => snapshot.value.values[rpmField.value] ?? null);
const liveTps = computed(() => snapshot.value.values[tpsField.value] ?? null);

const recording = ref(false);
const runPoints = shallowRef<DynoRunPoint[]>([]);
const statusMessage = ref<string | null>(null);

const ignoreTpsMin = ref(DEFAULT_DYNO_RUN_OPTIONS.ignoreTpsMin);
const minRpm = ref(DEFAULT_DYNO_RUN_OPTIONS.minRpm);

function runOptions(): DynoRunOptions {
  const min = Math.max(0, Math.round(minRpm.value));
  return {
    ignoreTpsMin: ignoreTpsMin.value,
    minRpm: min,
  };
}

function applyRunOptions(): void {
  dyno?.setRunOptions(runOptions());
}

let dyno: DynoView | null = null;
let timeOffsetSec = 0;
let lastSampleSec = -1;

const currentTorque = computed(() => dyno?.currentTorque ?? 0);
const currentHp = computed(() => dyno?.currentHP ?? 0);
const peakTorque = computed(() =>
  runPoints.value.reduce((m, p) => Math.max(m, p.torqueNm), 0),
);
const peakHp = computed(() => runPoints.value.reduce((m, p) => Math.max(m, p.hp), 0));

const canStart = computed(
  () => connected.value && !recording.value && configSnapshot.value.loaded,
);
const canStop = computed(() => recording.value);
const canClear = computed(() => runPoints.value.length > 0 && !recording.value);

function timelineLiveSec(): number {
  const t = snapshot.value.timelineLiveSec;
  return t !== undefined && Number.isFinite(t) ? t : 0;
}

function rebuildDyno(): void {
  const cfg = dynoConfigFromValues(getConfigField);
  if (dyno) {
    dyno.updateConfig(cfg);
  } else {
    dyno = new DynoView(cfg);
  }
  applyRunOptions();
}

function recordingHint(): string {
  const parts: string[] = [];
  if (!ignoreTpsMin.value) {
    parts.push("TPS ≥ 30%");
  }
  if (minRpm.value > 0) {
    parts.push(`RPM ≥ ${Math.round(minRpm.value)}`);
  }
  if (parts.length === 0) {
    return "Запись: разгон по RPM (ограничения TPS/RPM сняты).";
  }
  return `Запись: ${parts.join(", ")}, без резкого сброса газа.`;
}

function processSample(): void {
  if (!recording.value || !dyno) return;

  const rpm = liveRpm.value;
  const tps = liveTps.value;
  if (rpm === null || tps === null) return;

  const timeSec = timelineLiveSec() - timeOffsetSec;
  if (timeSec <= lastSampleSec) return;
  lastSampleSec = timeSec;

  const point = dyno.onRpm(Math.round(rpm), timeSec, tps);
  if (point) {
    runPoints.value = [...runPoints.value, point];
    scheduleRedraw();
  }
}

function startRun(): void {
  if (!canStart.value) return;
  rebuildDyno();
  dyno?.reset();
  runPoints.value = [];
  timeOffsetSec = timelineLiveSec();
  lastSampleSec = -1;
  recording.value = true;
  statusMessage.value = recordingHint();
  scheduleRedraw();
}

function stopRun(): void {
  if (!recording.value) return;
  recording.value = false;
  statusMessage.value =
    runPoints.value.length > 0
      ? `Готово: ${runPoints.value.length} точек.`
      : `Запись остановлена без точек (${recordingHint().replace(/^Запись: /, "")}).`;
}

function clearRun(): void {
  dyno?.reset();
  runPoints.value = [];
  lastSampleSec = -1;
  statusMessage.value = null;
  scheduleRedraw();
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
  drawDynoChart(ctx, w, h, runPoints.value);
}

let redrawRaf = 0;
function scheduleRedraw(): void {
  cancelAnimationFrame(redrawRaf);
  redrawRaf = requestAnimationFrame(redraw);
}

let resizeObserver: ResizeObserver | undefined;

watch(snapshot, () => processSample(), { flush: "post" });

watch(
  () => configSnapshot.value.values,
  () => {
    if (!recording.value) rebuildDyno();
  },
  { deep: true },
);

watch([ignoreTpsMin, minRpm], () => {
  applyRunOptions();
  if (recording.value) {
    statusMessage.value = recordingHint();
  }
});

watch([runPoints, chartHeight, canvasWidth], () => scheduleRedraw(), { deep: true });

onMounted(async () => {
  await Promise.all([initOutputChannels(), initConfig()]);
  rebuildDyno();
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
      <p class="hint options-hint">
        Мин. RPM: ждём разгона до порога; при падении ниже — сброс заезда. Без TPS — для стимулятора.
      </p>
    </div>

    <p v-if="!connected" class="message warn">Подключите ECU для live output.</p>
    <p v-else-if="!configSnapshot.loaded" class="message warn">
      Загрузите config (проект или ECU) — параметры dyno из dynoChars.
    </p>
    <p
      v-if="statusMessage"
      class="message"
      :class="{ active: recording, muted: !recording }"
    >
      {{ statusMessage }}
    </p>

    <div ref="containerRef" class="dyno-chart-wrap">
      <canvas ref="canvasRef" class="dyno-canvas" />
    </div>

    <div v-if="runPoints.length > 0" class="peaks">
      <span>Пик Nm: <strong>{{ peakTorque.toFixed(1) }}</strong></span>
      <span>Пик HP: <strong>{{ peakHp.toFixed(1) }}</strong></span>
      <span>Точек: {{ runPoints.length }}</span>
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

.options-hint {
  flex: 1 1 100%;
  margin: 0;
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
