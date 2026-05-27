<script setup lang="ts">
import {
  computed,
  onMounted,
  onUnmounted,
  ref,
  watch,
} from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import {
  initKnockScope,
  useKnockScope,
} from "../../composables/useKnockScope";
import { drawKnockWaveform } from "../../composables/drawKnockWaveform";

const yamlProps = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const chartHeight = computed(() => {
  const h = Number(yamlProps.props.height ?? 280);
  return h >= 180 ? h : 280;
});

const chartRef = ref<HTMLCanvasElement | null>(null);
const { snapshot, setScopeEnabled } = useKnockScope();

const samples = computed(() => snapshot.value.samples ?? []);
const connected = computed(() => snapshot.value.connected);
const scopeEnabled = computed(() => snapshot.value.scopeEnabled);
const captureCount = computed(() => snapshot.value.captureCount ?? 0);
const sampleRateHz = computed(() => snapshot.value.sampleRateHz ?? 218_750);
const bufferDurationMs = computed(() => snapshot.value.bufferDurationMs ?? 0);
const sampleMin = computed(() => snapshot.value.sampleMin ?? 0);
const sampleMax = computed(() => snapshot.value.sampleMax ?? 0);
const lastByteLen = computed(() => snapshot.value.lastByteLen ?? 0);
const lastError = computed(() => snapshot.value.lastError ?? null);
const polling = computed(() => snapshot.value.polling);

const statusLine = computed(() => {
  const parts: string[] = [];
  parts.push(connected.value ? "ECU: подключена" : "ECU: нет связи");
  if (scopeEnabled.value) {
    parts.push(polling.value ? "опрос knock scope" : "scope вкл");
  }
  parts.push(`захватов: ${captureCount.value}`);
  if (lastByteLen.value > 0) {
    parts.push(`${lastByteLen.value} байт`);
  }
  if (samples.value.length > 0) {
    parts.push(
      `${samples.value.length} отсчётов · ~${bufferDurationMs.value.toFixed(2)} ms`,
    );
    parts.push(`ADC ${sampleMin.value.toFixed(0)}…${sampleMax.value.toFixed(0)}`);
  }
  return parts.join(" · ");
});

const hint = computed(() => {
  if (lastError.value) return lastError.value;
  if (!connected.value) {
    return "Подключите ECU. В tune: enableKnockScope = yes, прошивка с KNOCK_SCOPE.";
  }
  if (!scopeEnabled.value) {
    return "Старт scope — отдельный поток `l`+8/10 (как composite logger), без опроса O.";
  }
  if (captureCount.value === 0) {
    return "Ждём буфер с ECU (ответ 0x84 = ещё не готов)…";
  }
  return null;
});

function redraw() {
  const canvas = chartRef.value;
  if (!canvas) return;
  drawKnockWaveform(canvas, samples.value, {
    min: sampleMin.value,
    max: sampleMax.value,
    title: `Knock raw @ ${(sampleRateHz.value / 1000).toFixed(1)} kHz`,
  });
}

watch([samples, sampleMin, sampleMax], () => redraw(), { deep: true });

let resizeObs: ResizeObserver | null = null;

onMounted(async () => {
  await initKnockScope();
  redraw();
  if (chartRef.value) {
    resizeObs = new ResizeObserver(() => redraw());
    resizeObs.observe(chartRef.value);
  }
});

onUnmounted(() => {
  resizeObs?.disconnect();
  if (scopeEnabled.value) {
    void setScopeEnabled(false);
  }
});

async function toggleScope() {
  await setScopeEnabled(!scopeEnabled.value);
}
</script>

<template>
  <div class="spectrogram-panel">
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
