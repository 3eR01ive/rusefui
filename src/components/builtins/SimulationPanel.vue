<script setup lang="ts">
import { computed, onUnmounted, ref } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useDataContext } from "../../core/data-context";
import { useOutputChannels } from "../../composables/useOutputChannels";
import { useRustComponent } from "../../composables/useRustComponent";
import { useInstanceBind } from "../../composables/useInstanceBind";
import {
  runStimulatorRamp,
  type RampCurve,
} from "../../composables/useStimulatorRamp";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const { state, dispatch, error } = useRustComponent(props.instance, props.path);
const instanceRef = computed(() => props.instance);
const { paramStringOr } = useInstanceBind(instanceRef);
const dataCtx = useDataContext();
const { getField } = useOutputChannels();

const ecuRpm = computed(() => getField(paramStringOr("rpmField", "RPMValue")));

const rpm = computed({
  get: () => Number(state.value.rpm ?? 1500),
  set: (value: number) => void dispatch("set_rpm", { rpm: value }),
});

const rpmMin = computed(() => Number(state.value.rpmMin ?? 0));
const rpmMax = computed(() => Number(state.value.rpmMax ?? 30000));
const connected = computed(() => dataCtx.connection.value.connected);
const active = computed(() => Boolean(state.value.active));
const busy = computed(() => Boolean(state.value.busy));
const message = computed(() => (state.value.message as string) ?? null);
const messageIsError = computed(() => Boolean(state.value.messageIsError));

/** Холостые / цель разгона */
const idleRpm = ref(800);
const peakRpm = ref(4500);
const rampUpSec = ref(4);
const rampDownSec = ref(4);
const rampCurve = ref<RampCurve>("linear");

const rampRunning = ref(false);
const rampPhase = ref<"up" | "down" | null>(null);
const rampError = ref<string | null>(null);
const liveRampRpm = ref<number | null>(null);

let rampAbort: AbortController | null = null;

const canStart = computed(
  () => connected.value && !busy.value && !active.value && !rampRunning.value && rpm.value > 0,
);
const canStop = computed(
  () => connected.value && !busy.value && active.value && !rampRunning.value,
);

const canRamp = computed(
  () =>
    connected.value &&
    active.value &&
    !busy.value &&
    !rampRunning.value &&
    peakRpm.value !== idleRpm.value,
);

function syncIdleFromSlider(): void {
  idleRpm.value = rpm.value;
}

function start() {
  return dispatch("start");
}

function stop() {
  return dispatch("stop");
}

function cancelRamp(): void {
  rampAbort?.abort();
}

async function startRamp(): Promise<void> {
  if (!canRamp.value) return;
  rampError.value = null;
  rampRunning.value = true;
  rampPhase.value = "up";
  rampAbort = new AbortController();

  try {
    await runStimulatorRamp(
      {
        idleRpm: idleRpm.value,
        peakRpm: peakRpm.value,
        rampUpSec: rampUpSec.value,
        rampDownSec: rampDownSec.value,
        curve: rampCurve.value,
        stepMs: 100,
      },
      rpmMin.value,
      rpmMax.value,
      rampAbort.signal,
      (stepRpm, phase) => {
        liveRampRpm.value = stepRpm;
        rampPhase.value = phase;
      },
    );
    void dispatch("set_rpm", { rpm: idleRpm.value });
    rampError.value = null;
  } catch (e) {
    if (e instanceof DOMException && e.name === "AbortError") {
      rampError.value = "Разгон отменён.";
    } else {
      rampError.value = e instanceof Error ? e.message : String(e);
    }
  } finally {
    rampRunning.value = false;
    rampPhase.value = null;
    liveRampRpm.value = null;
    rampAbort = null;
  }
}

onUnmounted(() => {
  rampAbort?.abort();
});
</script>

<template>
  <div class="simulation-panel">
    <div class="field">
      <label for="sim-rpm">Trigger Simulator RPM (холостые / старт)</label>
      <div class="rpm-row">
        <input
          id="sim-rpm"
          v-model.number="rpm"
          type="range"
          :min="rpmMin"
          :max="rpmMax"
          step="50"
          :disabled="!connected || busy || active || rampRunning"
          @change="syncIdleFromSlider"
        />
        <input
          v-model.number="rpm"
          type="number"
          class="rpm-input"
          :min="rpmMin"
          :max="rpmMax"
          step="50"
          :disabled="!connected || busy || active || rampRunning"
          @change="syncIdleFromSlider"
        />
      </div>
      <p class="hint">Диапазон {{ rpmMin }}–{{ rpmMax }} RPM. Команда <code>E</code> + <code>rpm N</code>.</p>
    </div>

    <div class="actions">
      <button type="button" class="btn primary" :disabled="!canStart" @click="start">
        {{ busy ? "…" : "Запустить стимуляцию" }}
      </button>
      <button type="button" class="btn secondary" :disabled="!canStop" @click="stop">
        Остановить
      </button>
    </div>

    <section class="ramp-section">
      <h4 class="ramp-title">Эмуляция разгона</h4>
      <p class="hint">
        При включённой стимуляции плавно меняет RPM (<code>rpm N</code> без disable/enable), затем
        возвращает на холостые.
      </p>

      <div class="ramp-grid">
        <label class="ramp-field">
          <span>Холостые RPM</span>
          <input
            v-model.number="idleRpm"
            type="number"
            :min="rpmMin"
            :max="rpmMax"
            step="50"
            :disabled="rampRunning"
          />
        </label>
        <label class="ramp-field">
          <span>Конечные RPM</span>
          <input
            v-model.number="peakRpm"
            type="number"
            :min="rpmMin"
            :max="rpmMax"
            step="50"
            :disabled="rampRunning"
          />
        </label>
        <label class="ramp-field">
          <span>Разгон, с</span>
          <input
            v-model.number="rampUpSec"
            type="number"
            min="0.1"
            max="120"
            step="0.5"
            :disabled="rampRunning"
          />
        </label>
        <label class="ramp-field">
          <span>Сброс, с</span>
          <input
            v-model.number="rampDownSec"
            type="number"
            min="0.1"
            max="120"
            step="0.5"
            :disabled="rampRunning"
          />
        </label>
        <label class="ramp-field ramp-field--curve">
          <span>Кривая</span>
          <select v-model="rampCurve" :disabled="rampRunning">
            <option value="linear">Линейная</option>
            <option value="smooth">Плавная (smoothstep)</option>
          </select>
        </label>
      </div>

      <div class="actions">
        <button
          type="button"
          class="btn accent"
          :disabled="!canRamp"
          @click="startRamp"
        >
          {{ rampRunning ? "Разгон…" : "Разгон" }}
        </button>
        <button
          v-if="rampRunning"
          type="button"
          class="btn ghost"
          @click="cancelRamp"
        >
          Отмена
        </button>
        <button
          type="button"
          class="btn ghost"
          :disabled="rampRunning || active"
          title="Взять текущее значение слайдера"
          @click="syncIdleFromSlider"
        >
          Холостые = слайдер
        </button>
      </div>

      <p v-if="!active && connected" class="message warn">
        Сначала запустите стимуляцию, затем «Разгон».
      </p>
      <p v-if="rampRunning" class="message active">
        {{ rampPhase === "down" ? "Сброс на холостые" : "Разгон" }}:
        <strong>{{ liveRampRpm ?? "—" }}</strong> RPM
      </p>
      <p v-if="rampError" class="message error">{{ rampError }}</p>
    </section>

    <p v-if="!connected" class="message warn">Подключите ECU на вкладке «Подключение».</p>

    <p
      v-if="message || error"
      class="message"
      :class="{
        error: messageIsError || !!error,
        success: active && !messageIsError && !error,
      }"
    >
      {{ error ?? message }}
    </p>

    <div v-if="active" class="status-box active">
      <p class="status-label">Стимуляция активна</p>
      <p class="status-value">
        Задано {{ liveRampRpm ?? rpm }} RPM
        <span v-if="rampRunning" class="status-ramp">(разгон)</span>
      </p>
      <p v-if="ecuRpm != null" class="status-ecu">
        RPMValue с ECU: <strong>{{ Math.round(ecuRpm) }}</strong>
      </p>
    </div>
  </div>
</template>

<style scoped>
.simulation-panel {
  display: grid;
  gap: 1rem;
  width: 100%;
  max-width: 40rem;
}

.field label,
.ramp-field span {
  display: block;
  margin-bottom: 0.35rem;
  font-size: 0.78rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-gray);
  font-weight: 500;
}

.rpm-row {
  display: flex;
  gap: 0.75rem;
  align-items: center;
}

.rpm-row input[type="range"] {
  flex: 1;
}

.rpm-input,
.ramp-field input,
.ramp-field select {
  width: 100%;
  padding: 0.5rem 0.65rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-text);
  box-sizing: border-box;
}

.rpm-input {
  width: 6.5rem;
}

.hint {
  margin: 0.35rem 0 0;
  font-size: 0.82rem;
  color: var(--color-text-subtle);
}

.hint code {
  font-size: 0.9em;
}

.ramp-section {
  padding: 1rem;
  border-radius: var(--radius-md);
  border: 1px dashed var(--color-border-strong);
  background: var(--color-bg-subtle, var(--color-bg-elevated));
}

.ramp-title {
  margin: 0 0 0.5rem;
  font-size: 0.95rem;
  font-weight: 600;
}

.ramp-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(7rem, 1fr));
  gap: 0.75rem;
  margin: 0.75rem 0;
}

.ramp-field--curve {
  grid-column: span 2;
}

.actions {
  display: flex;
  gap: 0.5rem;
  flex-wrap: wrap;
}

.btn {
  padding: 0.55rem 1.1rem;
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

.btn.accent {
  background: var(--color-success-text, #2d6a4f);
  color: var(--color-on-accent, #fff);
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

.message {
  margin: 0;
  font-size: 0.9rem;
  color: var(--color-text-muted);
}

.message.warn {
  color: var(--color-text-subtle);
}

.message.active {
  color: var(--color-success-text);
}

.message.success {
  color: var(--color-success-text);
}

.message.error {
  color: var(--color-error);
  background: var(--color-error-bg);
  padding: 0.5rem 0.65rem;
  border-radius: var(--radius-sm);
  border-left: 3px solid var(--color-accent);
}

.status-box.active {
  padding: 1rem;
  border-radius: var(--radius-md);
  background: var(--color-bg-accent-soft);
  border: 1px solid var(--color-success-border);
}

.status-label {
  margin: 0 0 0.35rem;
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-accent-hover);
  font-weight: 600;
}

.status-value {
  margin: 0;
  font-size: 1.25rem;
  font-weight: 600;
}

.status-ramp {
  font-size: 0.85rem;
  font-weight: 500;
  color: var(--color-text-muted);
}

.status-ecu {
  margin: 0.5rem 0 0;
  font-size: 0.9rem;
  color: var(--color-text-muted);
}
</style>
