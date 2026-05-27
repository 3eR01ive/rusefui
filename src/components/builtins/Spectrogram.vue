<script setup lang="ts">
import {
  computed,
  onMounted,
  onUnmounted,
  ref,
  watch,
} from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useRustComponent } from "../../composables/useRustComponent";
import { drawKnockWaveform } from "../../composables/drawKnockWaveform";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const chartRef = ref<HTMLCanvasElement | null>(null);

const { state, dispatch, error, ready } = useRustComponent(
  props.instance,
  props.path,
);

const samples = computed(() => {
  const raw = state.value.samples;
  if (!Array.isArray(raw)) return [] as number[];
  return raw.map((v) => Number(v)).filter((v) => Number.isFinite(v));
});

const connected = computed(() => Boolean(state.value.connected));
const captureCount = computed(() => Number(state.value.captureCount ?? 0));
const scopeEnabled = computed(() => Boolean(state.value.scopeEnabled));
const readyFieldPresent = computed(() => Boolean(state.value.readyFieldPresent));
const knockScopeReady = computed(() => Boolean(state.value.knockScopeReady));
const message = computed(() => (state.value.message as string) ?? null);
const sampleRateHz = computed(() => Number(state.value.sampleRateHz ?? 218750));
const bufferDurationMs = computed(() => Number(state.value.bufferDurationMs ?? 0));
const sampleMin = computed(() => Number(state.value.sampleMin ?? 0));
const sampleMax = computed(() => Number(state.value.sampleMax ?? 0));
const lastByteLen = computed(() => Number(state.value.lastByteLen ?? 0));

const statusLine = computed(() => {
  const parts: string[] = [];
  parts.push(connected.value ? "ECU: подключена" : "ECU: нет связи");
  if (!readyFieldPresent.value) {
    parts.push("поле knockScopeReady нет в INI");
  } else if (knockScopeReady.value) {
    parts.push("буфер готов");
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

onMounted(() => {
  redraw();
  if (chartRef.value) {
    resizeObs = new ResizeObserver(() => redraw());
    resizeObs.observe(chartRef.value);
  }
});

onUnmounted(() => {
  resizeObs?.disconnect();
});

async function toggleScope() {
  await dispatch(scopeEnabled.value ? "disable_scope" : "enable_scope");
}
</script>

<template>
  <div class="spectrogram-panel">
    <div class="spectrogram-toolbar">
      <button
        type="button"
        class="btn"
        :disabled="!ready"
        @click="toggleScope"
      >
        {{ scopeEnabled ? "Стоп scope" : "Старт scope" }}
      </button>
      <span class="spectrogram-status">{{ statusLine }}</span>
    </div>
    <p v-if="message" class="spectrogram-hint">{{ message }}</p>
    <p v-if="error" class="spectrogram-error">{{ error }}</p>
    <canvas
      ref="chartRef"
      class="spectrogram-canvas"
      :style="{ height: `${Number(props.props.height ?? 280)}px` }"
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

.spectrogram-error {
  margin: 0;
  font-size: 12px;
  color: var(--color-danger, #e57373);
}

.spectrogram-canvas {
  width: 100%;
  border-radius: 6px;
  border: 1px solid var(--color-border);
  background: var(--color-bg-panel);
}
</style>
