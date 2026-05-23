<script setup lang="ts">
import { computed } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useDataContext } from "../../core/data-context";
import { useRustComponent } from "../../composables/useRustComponent";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const { state, dispatch, error } = useRustComponent(props.instance, props.path);
const dataCtx = useDataContext();

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

const canStart = computed(
  () => connected.value && !busy.value && !active.value && rpm.value > 0,
);
const canStop = computed(() => connected.value && !busy.value && active.value);

function start() {
  return dispatch("start");
}

function stop() {
  return dispatch("stop");
}
</script>

<template>
  <div class="simulation-panel">
    <div class="field">
      <label for="sim-rpm">Trigger Simulator RPM</label>
      <div class="rpm-row">
        <input
          id="sim-rpm"
          v-model.number="rpm"
          type="range"
          :min="rpmMin"
          :max="rpmMax"
          step="50"
          :disabled="!connected || busy || active"
        />
        <input
          v-model.number="rpm"
          type="number"
          class="rpm-input"
          :min="rpmMin"
          :max="rpmMax"
          step="50"
          :disabled="!connected || busy || active"
        />
      </div>
      <p class="hint">Диапазон {{ rpmMin }}–{{ rpmMax }} RPM (поле triggerSimulatorRpm в INI).</p>
    </div>

    <div class="actions">
      <button type="button" class="btn primary" :disabled="!canStart" @click="start">
        {{ busy ? "…" : "Запустить стимуляцию" }}
      </button>
      <button type="button" class="btn secondary" :disabled="!canStop" @click="stop">
        Остановить
      </button>
    </div>

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
      <p class="status-value">{{ rpm }} RPM</p>
    </div>
  </div>
</template>

<style scoped>
.simulation-panel {
  display: grid;
  gap: 1rem;
  width: 100%;
  max-width: 36rem;
}

.field label {
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

.rpm-input {
  width: 6.5rem;
  padding: 0.5rem 0.65rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-text);
}

.hint {
  margin: 0.35rem 0 0;
  font-size: 0.82rem;
  color: var(--color-text-subtle);
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
</style>
