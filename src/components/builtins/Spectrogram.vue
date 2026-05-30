<script setup lang="ts">
import {
  computed,
  onMounted,
  onUnmounted,
  ref,
  watch,
} from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useInstanceBind } from "../../composables/useInstanceBind";
import {
  initKnockScope,
  onKnockSpectrogramGlReset,
  subscribeKnockSpectrogramGpu,
  useKnockScope,
} from "../../composables/useKnockScope";
import { useTabActivity } from "../../composables/useTabActivity";
import {
  downsampleMinMax,
  drawKnockWaveform,
} from "../../composables/drawKnockWaveform";
import {
  b64ToArrayBuffer,
  mountKnockSpectrogramGl,
  type KnockSpectrogramGl,
} from "../../composables/knockSpectrogramGl";

const yamlProps = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const instanceRef = computed(() => yamlProps.instance);
const { source: bindSource } = useInstanceBind(instanceRef);

const chartHeight = computed(() => {
  const h = Number(yamlProps.props.height ?? 280);
  return h >= 180 ? h : 280;
});

const spectrogramHeight = computed(() => {
  const h = Number(yamlProps.props.spectrogramHeight ?? 200);
  return h >= 120 ? h : 200;
});

/** Длина скользящего окна на графике (мс). */
const windowMs = computed(() => {
  const w = Number(yamlProps.props.windowMs ?? 500);
  return w >= 50 ? w : 500;
});

const chartRef = ref<HTMLCanvasElement | null>(null);
const spectrogramRef = ref<HTMLCanvasElement | null>(null);
const {
  snapshot,
  spectrogramWidth,
  waveformRing,
  setScopeEnabled,
  setWaveformWindowMs,
} = useKnockScope();
const { isActive: tabActive } = useTabActivity();

let spectrogramGl: KnockSpectrogramGl | null = null;
let unsubSpectrogramGpu: (() => void) | null = null;
let unsubSpectrogramReset: (() => void) | null = null;
let redrawRaf = 0;

if (bindSource.value && bindSource.value !== "knockScope") {
  console.warn(
    `[spectrogram] ожидался bind.source=knockScope, получен ${bindSource.value}`,
  );
}

const spectrogramTitle = computed(() => {
  const w = spectrogramWidth.value;
  if (w < 1) return "Спектрограмма (0–20 kHz, dBFS)";
  return `Спектрограмма · ${w} cols · 0–20 kHz`;
});

const connected = computed(() => snapshot.value.connected);
const scopeEnabled = computed(() => snapshot.value.scopeEnabled);
const captureCount = computed(() => snapshot.value.captureCount ?? 0);
const sampleRateHz = computed(() => snapshot.value.sampleRateHz ?? 218_750);
const chunkDurationMs = computed(() => Number(snapshot.value.bufferDurationMs ?? 0));
const lastByteLen = computed(() => snapshot.value.lastByteLen ?? 0);
const knockScopeReady = computed(() => Boolean(snapshot.value.knockScopeReady));
const enableInConfig = computed(() => snapshot.value.enableKnockScopeInConfig);
const statusMessage = computed(() => snapshot.value.statusMessage ?? null);
const lastError = computed(() => snapshot.value.lastError ?? null);
const polling = computed(() => snapshot.value.polling);

const ringDurationMs = computed(() =>
  waveformRing.value.length > 0
    ? (waveformRing.value.length / sampleRateHz.value) * 1000
    : 0,
);

const displaySamples = computed(() => {
  const ring = waveformRing.value;
  if (ring.length < 2) return [...ring];
  const w = chartRef.value?.clientWidth ?? 800;
  const target = Math.max(200, Math.min(ring.length, w * 2));
  return downsampleMinMax([...ring], target);
});

function ringMinMax(ring: number[]): { min: number; max: number } {
  if (ring.length === 0) return { min: 0, max: 0 };
  let min = ring[0]!;
  let max = min;
  for (let i = 1; i < ring.length; i++) {
    const v = ring[i]!;
    if (v < min) min = v;
    if (v > max) max = v;
  }
  return { min, max };
}

const displayMin = computed(() => ringMinMax([...waveformRing.value]).min);
const displayMax = computed(() => ringMinMax([...waveformRing.value]).max);

const statusLine = computed(() => {
  const parts: string[] = [];
  parts.push(connected.value ? "ECU: подключена" : "ECU: нет связи");
  if (scopeEnabled.value) {
    parts.push(polling.value ? "live" : "scope");
    if (knockScopeReady.value) parts.push("ready");
    if (enableInConfig.value === false) parts.push("enableKnockScope=no");
  }
  parts.push(`захватов: ${captureCount.value}`);
  if (spectrogramWidth.value > 0) {
    parts.push(`FFT cols: ${spectrogramWidth.value}`);
  }
  if (waveformRing.value.length > 0) {
    parts.push(
      `окно ~${ringDurationMs.value.toFixed(0)} ms (${waveformRing.value.length} pts)`,
    );
    parts.push(`ADC ${displayMin.value.toFixed(0)}…${displayMax.value.toFixed(0)}`);
  } else if (lastByteLen.value > 0 && chunkDurationMs.value > 0) {
    parts.push(`чанк ~${chunkDurationMs.value.toFixed(1)} ms`);
  }
  return parts.join(" · ");
});

const hint = computed(() => {
  if (lastError.value) return lastError.value;
  if (statusMessage.value) return statusMessage.value;
  if (!connected.value) {
    return "Подключите ECU. В tune: enableKnockScope = yes, прошивка с KNOCK_SCOPE.";
  }
  if (!scopeEnabled.value) {
    return "Старт scope — непрерывная волна (скользящее окно на графике).";
  }
  if (waveformRing.value.length < 2) {
    return "Ждём первые сэмплы…";
  }
  return null;
});

function bindSpectrogramGl(): void {
  spectrogramGl?.destroy();
  const canvas = spectrogramRef.value;
  spectrogramGl = canvas ? mountKnockSpectrogramGl(canvas) : null;
}

function applySpectrogramGpuB64(b64: string): void {
  if (!spectrogramGl) return;
  spectrogramGl.applyBuffer(b64ToArrayBuffer(b64));
  spectrogramGl.draw();
}

function scheduleRedraw() {
  if (!tabActive.value) return;
  if (redrawRaf !== 0) return;
  redrawRaf = requestAnimationFrame(() => {
    redrawRaf = 0;
    redraw();
  });
}

function redrawWaveform() {
  const canvas = chartRef.value;
  if (!canvas) return;
  drawKnockWaveform(canvas, displaySamples.value, {
    min: displayMin.value,
    max: displayMax.value,
    title: `Knock live · ${(ringDurationMs.value / 1000).toFixed(2)} s @ ${(sampleRateHz.value / 1000).toFixed(1)} kHz`,
  });
}

function redrawSpectrogram() {
  spectrogramGl?.draw();
}

function redraw() {
  redrawWaveform();
  redrawSpectrogram();
}

watch(
  () => snapshot.value.captureCount,
  () => {
    if (tabActive.value) scheduleRedraw();
  },
);
watch([displaySamples, displayMin, displayMax], () => scheduleRedraw());
watch([spectrogramHeight, chartHeight], () => scheduleRedraw());

watch(windowMs, (ms) => {
  setWaveformWindowMs(ms);
});

watch(tabActive, (active, wasActive) => {
  if (active && !wasActive) {
    scheduleRedraw();
  }
});

watch(spectrogramRef, (canvas) => {
  if (canvas) {
    bindSpectrogramGl();
    scheduleRedraw();
  } else {
    spectrogramGl?.destroy();
    spectrogramGl = null;
  }
});

let resizeObs: ResizeObserver | null = null;

const panelRef = ref<HTMLElement | null>(null);

onMounted(async () => {
  setWaveformWindowMs(windowMs.value);
  unsubSpectrogramGpu = subscribeKnockSpectrogramGpu((b64) => {
    if (!tabActive.value) return;
    applySpectrogramGpuB64(b64);
  });
  unsubSpectrogramReset = onKnockSpectrogramGlReset(() => spectrogramGl?.reset());
  await initKnockScope();
  bindSpectrogramGl();
  scheduleRedraw();
  const observeTarget = panelRef.value ?? chartRef.value;
  if (observeTarget) {
    resizeObs = new ResizeObserver(() => scheduleRedraw());
    resizeObs.observe(observeTarget);
  }
  if (spectrogramRef.value) {
    resizeObs?.observe(spectrogramRef.value);
  }
});

onUnmounted(() => {
  resizeObs?.disconnect();
  unsubSpectrogramGpu?.();
  unsubSpectrogramGpu = null;
  unsubSpectrogramReset?.();
  unsubSpectrogramReset = null;
  spectrogramGl?.destroy();
  spectrogramGl = null;
  if (redrawRaf !== 0) cancelAnimationFrame(redrawRaf);
  if (scopeEnabled.value) {
    void setScopeEnabled(false);
  }
});

async function toggleScope() {
  setWaveformWindowMs(windowMs.value);
  await setScopeEnabled(!scopeEnabled.value, windowMs.value);
}
</script>

<template>
  <div ref="panelRef" class="spectrogram-panel">
    <div class="spectrogram-toolbar">
      <button
        type="button"
        class="btn"
        :disabled="!connected"
        @click="toggleScope"
      >
        {{ scopeEnabled ? "Стоп scope" : "Старт scope" }}
      </button>
      <span class="spectrogram-status">{{ statusLine }}</span>
    </div>
    <p v-if="hint" class="spectrogram-hint">{{ hint }}</p>
    <canvas
      ref="chartRef"
      class="spectrogram-canvas"
      :style="{ height: `${chartHeight}px` }"
    />
    <p class="spectrogram-heatmap-title">{{ spectrogramTitle }}</p>
    <div class="spectrogram-heatmap-wrap" :style="{ height: `${spectrogramHeight}px` }">
      <canvas ref="spectrogramRef" class="spectrogram-canvas spectrogram-canvas--gl" />
    </div>
  </div>
</template>

<style scoped>
.spectrogram-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 200px;
}

.spectrogram-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
}

.spectrogram-status {
  font-size: 12px;
  color: var(--color-text-muted);
}

.spectrogram-hint {
  margin: 0;
  font-size: 12px;
  color: var(--color-text-muted);
}

.spectrogram-heatmap-title {
  margin: 0;
  font-size: 12px;
  color: var(--color-text-muted);
}

.spectrogram-heatmap-wrap {
  position: relative;
  width: 100%;
  overflow: hidden;
  border-radius: 6px;
  border: 1px solid var(--color-border);
  background: #000;
}

.spectrogram-canvas--gl {
  display: block;
  width: 100%;
  height: 100%;
  background: #000;
}

.spectrogram-canvas {
  display: block;
  width: 100%;
  border-radius: 6px;
  border: 1px solid var(--color-border);
  background: var(--color-bg-panel);
}
</style>
