<script setup lang="ts">
import { computed, watch } from "vue";
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

const ports = computed(() => (state.value.ports as string[]) ?? []);
const selectedPort = computed({
  get: () => (state.value.selectedPort as string) ?? "",
  set: (port: string) => void dispatch("set_selected_port", { port }),
});
const baudRate = computed({
  get: () => (state.value.baudRate as number) ?? 115200,
  set: (baud_rate: number) => void dispatch("set_baud_rate", { baud_rate }),
});
const baudRates = computed(() => (state.value.baudRates as number[]) ?? [115200]);
const loadingPorts = computed(() => Boolean(state.value.loadingPorts));
const connecting = computed(() => Boolean(state.value.connecting));
const message = computed(() => (state.value.message as string) ?? null);
const messageIsError = computed(() => Boolean(state.value.messageIsError));
const isConnected = computed(() => Boolean(state.value.connected));

const canConnect = computed(
  () => !!selectedPort.value && !connecting.value && !isConnected.value,
);

watch(
  state,
  (s) => {
    dataCtx.connection.value = {
      connected: Boolean(s.connected),
      port_name: (s.portName as string) ?? null,
      baud_rate: (s.baudRateActive as number) ?? null,
      signature: (s.signature as string) ?? null,
      handshake_command: s.handshakeCommand != null ? String(s.handshakeCommand) : null,
      last_error: error.value,
    };
  },
  { deep: true },
);

function refreshPorts() {
  return dispatch("refresh_ports");
}

function connect() {
  return dispatch("connect");
}

function disconnect() {
  return dispatch("disconnect");
}
</script>

<template>
  <div class="connection-panel">
    <div class="field">
      <label for="port">Порт</label>
      <div class="row">
        <select
          id="port"
          :value="selectedPort"
          :disabled="isConnected || connecting"
          @change="selectedPort = ($event.target as HTMLSelectElement).value"
        >
          <option v-if="!ports.length" value="" disabled>— нет портов —</option>
          <option v-for="p in ports" :key="p" :value="p">{{ p }}</option>
        </select>
        <button
          type="button"
          class="btn secondary"
          :disabled="loadingPorts || isConnected"
          @click="refreshPorts"
        >
          {{ loadingPorts ? "…" : "Обновить" }}
        </button>
      </div>
    </div>

    <div class="field">
      <label for="baud">Скорость (baud)</label>
      <select
        id="baud"
        :value="baudRate"
        :disabled="isConnected || connecting"
        @change="baudRate = Number(($event.target as HTMLSelectElement).value)"
      >
        <option v-for="b in baudRates" :key="b" :value="b">{{ b }}</option>
      </select>
    </div>

    <div class="actions">
      <button type="button" class="btn primary" :disabled="!canConnect" @click="connect">
        {{ connecting ? "Подключение…" : "Подключить" }}
      </button>
      <button
        type="button"
        class="btn secondary"
        :disabled="!isConnected || connecting"
        @click="disconnect"
      >
        Отключить
      </button>
    </div>

    <p
      v-if="message || error"
      class="message"
      :class="{ error: messageIsError || !!error, success: isConnected && !messageIsError }"
    >
      {{ error ?? message }}
    </p>

    <div v-if="isConnected" class="status-box connected">
      <p class="status-label">Подключено</p>
      <dl class="status-dl">
        <dt>Порт</dt>
        <dd>{{ state.portName }}</dd>
        <dt>Baud</dt>
        <dd>{{ state.baudRateActive }}</dd>
        <dt>Handshake</dt>
        <dd>{{ state.handshakeCommand }}</dd>
        <dt>Signature</dt>
        <dd class="signature">{{ state.signature }}</dd>
      </dl>
    </div>
  </div>
</template>

<style scoped>
.connection-panel {
  display: grid;
  grid-template-columns: 1fr;
  gap: 1rem 1.5rem;
  width: 100%;
  align-items: start;
}

@media (min-width: 768px) {
  .connection-panel {
    grid-template-columns: 1fr 1fr;
  }

  .connection-panel .actions,
  .connection-panel .message,
  .connection-panel .status-box {
    grid-column: 1 / -1;
  }
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

.row {
  display: flex;
  gap: 0.5rem;
}

select {
  flex: 1;
  min-width: 0;
  padding: 0.55rem 0.7rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-text);
}

select:disabled {
  opacity: 0.6;
  background: var(--color-bg-muted);
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

.btn.primary:hover:not(:disabled) {
  background: var(--color-accent-hover);
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

.status-box.connected {
  padding: 1rem;
  border-radius: var(--radius-md);
  background: var(--color-bg-accent-soft);
  border: 1px solid var(--color-success-border);
}

.status-label {
  margin: 0 0 0.6rem;
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-accent-hover);
  font-weight: 600;
}

.status-dl {
  margin: 0;
  display: grid;
  grid-template-columns: auto 1fr;
  gap: 0.4rem 1rem;
}

.status-dl dt {
  color: var(--color-gray);
  font-weight: 500;
}

.status-dl dd {
  margin: 0;
  word-break: break-all;
}

.signature {
  font-family: ui-monospace, monospace;
  font-size: 0.82rem;
}
</style>
