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
  panKnockSpectrogram,
  setKnockSpectrogramFollowLive,
  setKnockSpectrogramViewportColumns,
  formatKnockCaptureStats,
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
  knockSpectrogramGlStats,
  mountKnockSpectrogramGl,
  type KnockSpectrogramGl,
} from "../../composables/knockSpectrogramGl";
import { buildKnockMarkerOverlay } from "../../composables/knockSpectrogramMarkers";

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

/** Длина скользящего окна на графике (мс) — используется если windowEvents не задан. */
const windowMs = computed(() => {
  const w = Number(yamlProps.props.windowMs ?? 500);
  return w >= 50 ? w : 500;
});

/** Ширина окна в событиях (FFT-столбцах). Если > 0 — имеет приоритет над windowMs. */
const windowEvents = computed(() => {
  const v = yamlProps.props.windowEvents;
  if (v == null) return 0;
  const n = Math.round(Number(v));
  return n >= 1 ? n : 0;
});

const chartRef = ref<HTMLCanvasElement | null>(null);
const spectrogramRef = ref<HTMLCanvasElement | null>(null);
const {
  snapshot,
  spectrogramWidth,
  spectrogramMarkers,
  waveformRing,
  setScopeEnabled,
  setWaveformWindowMs,
} = useKnockScope();
const { isActive: tabActive } = useTabActivity();

let spectrogramGl: KnockSpectrogramGl | null = null;
let unsubSpectrogramGpu: (() => void) | null = null;
let unsubSpectrogramReset: (() => void) | null = null;
let redrawRaf = 0;

// Drag panning
let dragActive = false;
let dragLastX = 0;
let dragAccumPx = 0;

if (bindSource.value && bindSource.value !== "knockScope") {
  console.warn(
    `[spectrogram] ожидался bind.source=knockScope, получен ${bindSource.value}`,
  );
}

const spectrogramTitle = computed(() => {
  const w = spectrogramWidth.value;
  const total = snapshot.value.spectrogramTotalColumns ?? 0;
  if (w < 1) return "Спектрограмма (0–20 kHz, dBFS)";
  const follow = snapshot.value.spectrogramFollowLive !== false;
  const inView = snapshot.value.spectrogramViewCaptures ?? w;
  const start = snapshot.value.spectrogramViewStart ?? 0;
  const winStr = windowEvents.value > 0
    ? `окно ${windowEvents.value} событий`
    : `окно ${windowMs.value} ms`;
  const parts = [`Спектрограмма · ${winStr}`];
  if (total > inView) {
    parts.push(`${start + 1}–${start + inView}/${total}`);
  }
  parts.push(follow ? "live" : "просмотр");
  if (!follow) parts.push("dblclick → live");
  return parts.join(" · ");
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
const lastCylinder = computed(() => snapshot.value.lastCylinder);
const lastChannel = computed(() => snapshot.value.lastChannel);

const spectrogramWrapRef = ref<HTMLElement | null>(null);
const spectrogramLayoutTick = ref(0);

const markerOverlay = computed(() => {
  void spectrogramLayoutTick.value;
  const wrapW = spectrogramWrapRef.value?.clientWidth ?? 0;
  const texW = spectrogramWidth.value || knockSpectrogramGlStats.value.texW;
  return buildKnockMarkerOverlay(spectrogramMarkers.value, texW, wrapW);
});

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
  parts.push(formatKnockCaptureStats(snapshot.value, windowMs.value));
  if (waveformRing.value.length > 0) {
    parts.push(
      `окно ~${ringDurationMs.value.toFixed(0)} ms (${waveformRing.value.length} pts)`,
    );
    parts.push(`ADC ${displayMin.value.toFixed(0)}…${displayMax.value.toFixed(0)}`);
  } else   if (lastByteLen.value > 0 && chunkDurationMs.value > 0) {
    parts.push(`чанк ~${chunkDurationMs.value.toFixed(1)} ms`);
  }
  if (lastCylinder.value != null) {
    parts.push(`цил ${lastCylinder.value + 1}`);
  }
  if (lastChannel.value != null) {
    parts.push(`ch ${lastChannel.value}`);
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
    return "Старт scope — окна software knock по цилиндрам (склеиваются на графике).";
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
  const texW = spectrogramWidth.value || knockSpectrogramGlStats.value.texW;
  spectrogramGl?.setMarkers(spectrogramMarkers.value, texW);
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
watch([spectrogramHeight, chartHeight, markerOverlay], () => scheduleRedraw());

watch(windowMs, (ms) => {
  setWaveformWindowMs(ms);
});

watch(windowEvents, (n) => {
  if (n > 0) void setKnockSpectrogramViewportColumns(n);
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
  if (windowEvents.value > 0) void setKnockSpectrogramViewportColumns(windowEvents.value);
  bindSpectrogramGl();
  scheduleRedraw();
  const observeTarget = panelRef.value ?? chartRef.value;
  if (observeTarget) {
    resizeObs = new ResizeObserver(() => {
      spectrogramLayoutTick.value += 1;
      scheduleRedraw();
    });
    resizeObs.observe(observeTarget);
  }
  if (spectrogramWrapRef.value) {
    resizeObs?.observe(spectrogramWrapRef.value);
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
  if (!scopeEnabled.value) {
    const texW = spectrogramWidth.value || knockSpectrogramGlStats.value.texW;
    spectrogramGl?.setMarkers(spectrogramMarkers.value, texW);
    scheduleRedraw();
  }
}

function onSpectrogramWheel(e: WheelEvent): void {
  if (!scopeEnabled.value && captureCount.value < 1) return;
  const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
  if (delta === 0) return;
  const cols = Math.max(1, Math.round(delta / 12));
  void panKnockSpectrogram(delta > 0 ? cols : -cols);
}

function onSpectrogramDblClick(): void {
  void setKnockSpectrogramFollowLive(true);
}

const SPECTROGRAM_MARGIN_LEFT = 52;
const SPECTROGRAM_MARGIN_RIGHT = 56;

function onHeatmapPointerDown(e: PointerEvent): void {
  if (!scopeEnabled.value && captureCount.value < 1) return;
  dragActive = true;
  dragLastX = e.clientX;
  dragAccumPx = 0;
  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
}

function onHeatmapPointerMove(e: PointerEvent): void {
  if (!dragActive) return;
  const dx = e.clientX - dragLastX;
  dragLastX = e.clientX;
  if (dx === 0) return;
  dragAccumPx += dx;
  const wrap = spectrogramWrapRef.value;
  const plotW = Math.max(1, (wrap?.clientWidth ?? 800) - SPECTROGRAM_MARGIN_LEFT - SPECTROGRAM_MARGIN_RIGHT);
  const viewW = spectrogramWidth.value;
  if (viewW < 1) return;
  const colsPerPx = viewW / plotW;
  const cols = Math.round(dragAccumPx * colsPerPx);
  if (cols !== 0) {
    dragAccumPx -= cols / colsPerPx;
    // drag right → показать более старые данные (отрицательный сдвиг)
    void panKnockSpectrogram(-cols);
  }
}

function onHeatmapPointerUp(e: PointerEvent): void {
  if (!dragActive) return;
  dragActive = false;
  dragAccumPx = 0;
  try { (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId); } catch {}
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
    <div
      ref="spectrogramWrapRef"
      class="spectrogram-heatmap-wrap"
      :style="{ height: `${spectrogramHeight}px` }"
      @wheel.prevent="onSpectrogramWheel"
      @dblclick="onSpectrogramDblClick"
      @pointerdown="onHeatmapPointerDown"
      @pointermove="onHeatmapPointerMove"
      @pointerup="onHeatmapPointerUp"
      @pointercancel="onHeatmapPointerUp"
    >
      <canvas ref="spectrogramRef" class="spectrogram-canvas spectrogram-canvas--gl" />
      <div class="spectrogram-markers" aria-hidden="true">
        <div
          v-for="(mk, i) in markerOverlay"
          :key="`cyl-${mk.cylinder}-${mk.x}-${i}`"
          class="spectrogram-marker"
          :style="{ left: `${mk.x}px` }"
        >
          <span class="spectrogram-marker-label">{{ mk.label }}</span>
        </div>
      </div>
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
  cursor: grab;
  touch-action: none;
}
.spectrogram-heatmap-wrap:active {
  cursor: grabbing;
}

.spectrogram-markers {
  position: absolute;
  inset: 0;
  pointer-events: none;
  overflow: hidden;
}

.spectrogram-marker {
  position: absolute;
  top: 12px;
  bottom: 32px;
  width: 0;
}

.spectrogram-marker-label {
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
