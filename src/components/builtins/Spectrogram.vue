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
import { useInstanceBind } from "../../composables/useInstanceBind";
import {
  initKnockScope,
  useKnockScope,
} from "../../composables/useKnockScope";
import {
  appendKnockWaveformRing,
  downsampleMinMax,
  drawKnockWaveform,
} from "../../composables/drawKnockWaveform";
import {
  drawKnockSpectrogram,
  type KnockSpectrogramView,
} from "../../composables/drawKnockSpectrogram";

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
const { snapshot, setScopeEnabled } = useKnockScope();

if (bindSource.value && bindSource.value !== "knockScope") {
  console.warn(
    `[spectrogram] ожидался bind.source=knockScope, получен ${bindSource.value}`,
  );
}

const spectrogramView = computed((): KnockSpectrogramView => {
  const s = snapshot.value.spectrogram;
  return {
    width: s?.width ?? 0,
    height: s?.height ?? 0,
    freqStartHz: s?.freqStartHz ?? 4000,
    freqStepHz: s?.freqStepHz ?? 0,
    pixels: s?.pixels ? [...s.pixels] : [],
  };
});

const spectrogramTitle = computed(() => {
  const v = spectrogramView.value;
  if (v.width < 1) return "Спектрограмма (FFT, Rust)";
  const fEnd = v.freqStartHz + v.freqStepHz * Math.max(0, v.height - 1);
  return `Спектрограмма · ${v.width} cols · ${Math.round(v.freqStartHz)}–${Math.round(fEnd)} Hz`;
});

/** Непрерывная лента сэмплов (склеенные захваты). */
const waveformRing = shallowRef<number[]>([]);
let lastCaptureCount = 0;

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

const ringMaxSamples = computed(() =>
  Math.max(4096, Math.round((sampleRateHz.value * windowMs.value) / 1000)),
);

const ringDurationMs = computed(() =>
  waveformRing.value.length > 0
    ? (waveformRing.value.length / sampleRateHz.value) * 1000
    : 0,
);

const displaySamples = computed(() => {
  const ring = waveformRing.value;
  if (ring.length < 2) return ring;
  const w = chartRef.value?.clientWidth ?? 800;
  const target = Math.max(200, Math.min(ring.length, w * 2));
  return downsampleMinMax(ring, target);
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

const displayMin = computed(() => ringMinMax(waveformRing.value).min);
const displayMax = computed(() => ringMinMax(waveformRing.value).max);

const statusLine = computed(() => {
  const parts: string[] = [];
  parts.push(connected.value ? "ECU: подключена" : "ECU: нет связи");
  if (scopeEnabled.value) {
    parts.push(polling.value ? "live" : "scope");
    if (knockScopeReady.value) parts.push("ready");
    if (enableInConfig.value === false) parts.push("enableKnockScope=no");
  }
  parts.push(`захватов: ${captureCount.value}`);
  if (spectrogramView.value.width > 0) {
    parts.push(`FFT cols: ${spectrogramView.value.width}`);
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

function clearRing() {
  waveformRing.value = [];
  lastCaptureCount = 0;
}

function ingestSnapshot(snap: typeof snapshot.value) {
  if (!snap.scopeEnabled) return;
  if (snap.captureCount === 0) {
    clearRing();
    return;
  }
  if (snap.captureCount > lastCaptureCount) {
    const chunk = snap.samples ?? [];
    if (chunk.length > 0) {
      waveformRing.value = appendKnockWaveformRing(
        waveformRing.value,
        [...chunk],
        ringMaxSamples.value,
      );
    }
    lastCaptureCount = snap.captureCount;
  }
}

watch(() => snapshot.value, (snap) => ingestSnapshot(snap), { flush: "sync" });

let redrawRaf = 0;

function scheduleRedraw() {
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
  const canvas = spectrogramRef.value;
  if (!canvas) return;
  drawKnockSpectrogram(canvas, spectrogramView.value, {
    title: spectrogramTitle.value,
  });
}

function redraw() {
  redrawWaveform();
  redrawSpectrogram();
}

watch([displaySamples, displayMin, displayMax], () => scheduleRedraw());
watch(waveformRing, () => scheduleRedraw());
watch(spectrogramView, () => scheduleRedraw(), { deep: true });

let resizeObs: ResizeObserver | null = null;
let liveRedrawRaf = 0;

function startLiveRedraw() {
  const tick = () => {
    if (!scopeEnabled.value) {
      liveRedrawRaf = 0;
      return;
    }
    scheduleRedraw();
    liveRedrawRaf = requestAnimationFrame(tick);
  };
  if (liveRedrawRaf === 0) {
    liveRedrawRaf = requestAnimationFrame(tick);
  }
}

function stopLiveRedraw() {
  if (liveRedrawRaf !== 0) {
    cancelAnimationFrame(liveRedrawRaf);
    liveRedrawRaf = 0;
  }
}

watch(scopeEnabled, (on) => {
  if (on) {
    startLiveRedraw();
  } else {
    clearRing();
    stopLiveRedraw();
  }
});

const panelRef = ref<HTMLElement | null>(null);

onMounted(async () => {
  await initKnockScope();
  ingestSnapshot(snapshot.value);
  scheduleRedraw();
  if (scopeEnabled.value) startLiveRedraw();
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
  stopLiveRedraw();
  if (redrawRaf !== 0) cancelAnimationFrame(redrawRaf);
  if (scopeEnabled.value) {
    void setScopeEnabled(false);
  }
});

async function toggleScope() {
  if (scopeEnabled.value) {
    clearRing();
  }
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
    <canvas
      ref="spectrogramRef"
      class="spectrogram-canvas spectrogram-heatmap"
      :style="{ height: `${spectrogramHeight}px` }"
    />
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

.spectrogram-canvas {
  width: 100%;
  border-radius: 6px;
  border: 1px solid var(--color-border);
  background: var(--color-bg-panel);
}
</style>
