<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { ConnectionStatus, ConnectParams } from "../types/connection";

const BAUD_RATES = [115200, 230400, 460800, 921600] as const;

const ports = ref<string[]>([]);
const selectedPort = ref("");
const baudRate = ref<number>(115200);
const loadingPorts = ref(false);
const connecting = ref(false);
const status = ref<ConnectionStatus>({ connected: false });
const message = ref<string | null>(null);

const isConnected = computed(() => status.value.connected);
const canConnect = computed(
  () => !!selectedPort.value && !connecting.value && !isConnected.value,
);
const isErrorMessage = computed(
  () =>
  message.value != null &&
  !isConnected.value &&
  (message.value.toLowerCase().includes("error") ||
    message.value.toLowerCase().includes("ошиб") ||
    message.value.toLowerCase().includes("failed") ||
    message.value.toLowerCase().includes("timeout")),
);

async function refreshPorts() {
  loadingPorts.value = true;
  message.value = null;
  try {
    ports.value = await invoke<string[]>("list_serial_ports");
    if (ports.value.length && !selectedPort.value) {
      selectedPort.value = ports.value[0];
    }
    if (!ports.value.length) {
      message.value = "Последовательные порты не найдены.";
    }
  } catch (e) {
    message.value = String(e);
  } finally {
    loadingPorts.value = false;
  }
}

async function loadStatus() {
  try {
    status.value = await invoke<ConnectionStatus>("connection_status");
  } catch (e) {
    message.value = String(e);
  }
}

async function connect() {
  if (!selectedPort.value) return;
  connecting.value = true;
  message.value = null;
  const params: ConnectParams = {
    port: selectedPort.value,
    baud_rate: baudRate.value,
  };
  try {
    status.value = await invoke<ConnectionStatus>("connect_ecu", { params });
    message.value = "Подключено.";
  } catch (e) {
    message.value = String(e);
    await loadStatus();
  } finally {
    connecting.value = false;
  }
}

async function disconnect() {
  connecting.value = true;
  message.value = null;
  try {
    status.value = await invoke<ConnectionStatus>("disconnect_ecu");
    message.value = "Отключено.";
  } catch (e) {
    message.value = String(e);
  } finally {
    connecting.value = false;
  }
}

onMounted(async () => {
  await refreshPorts();
  await loadStatus();
});
</script>

<template>
  <section class="card">
    <h2 class="card-title">Подключение к ECU</h2>

    <div class="field">
      <label for="port">Порт</label>
      <div class="row">
        <select id="port" v-model="selectedPort" :disabled="isConnected || connecting">
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
      <select id="baud" v-model.number="baudRate" :disabled="isConnected || connecting">
        <option v-for="b in BAUD_RATES" :key="b" :value="b">{{ b }}</option>
      </select>
    </div>

    <div class="actions">
      <button
        type="button"
        class="btn primary"
        :disabled="!canConnect"
        @click="connect"
      >
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
      v-if="message"
      class="message"
      :class="{ error: isErrorMessage, success: isConnected && !isErrorMessage }"
    >
      {{ message }}
    </p>

    <div v-if="isConnected" class="status-box connected">
      <p class="status-label">Подключено</p>
      <dl class="status-dl">
        <dt>Порт</dt>
        <dd>{{ status.port_name }}</dd>
        <dt>Baud</dt>
        <dd>{{ status.baud_rate }}</dd>
        <dt>Handshake</dt>
        <dd>{{ status.handshake_command }}</dd>
        <dt>Signature</dt>
        <dd class="signature">{{ status.signature }}</dd>
      </dl>
    </div>

    <div v-else class="status-box idle">
      <p>ECU не подключена. Выберите порт и нажмите «Подключить».</p>
      <p class="hint">
        Отправляется команда <code>S</code> (CRC), при необходимости — plain <code>Q</code>.
      </p>
    </div>
  </section>
</template>

<style scoped>
.card {
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  padding: 1.35rem 1.4rem;
  box-shadow: var(--shadow-card);
}

.card-title {
  margin: 0 0 1.25rem;
  font-size: 1.1rem;
  font-weight: 600;
  color: var(--color-text);
}

.field {
  margin-bottom: 1rem;
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
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}

select:hover:not(:disabled) {
  border-color: var(--color-accent-muted);
}

select:focus {
  outline: none;
  border-color: var(--color-accent);
  box-shadow: 0 0 0 3px var(--color-bg-accent-soft);
}

select:disabled {
  opacity: 0.6;
  background: var(--color-bg-muted);
}

.actions {
  display: flex;
  gap: 0.5rem;
  margin-top: 1.25rem;
  flex-wrap: wrap;
}

.btn {
  padding: 0.55rem 1.1rem;
  border-radius: var(--radius-md);
  border: 1px solid transparent;
  font-weight: 500;
  transition:
    background-color 0.15s ease,
    border-color 0.15s ease,
    transform 0.1s ease;
}

.btn:active:not(:disabled) {
  transform: translateY(1px);
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

.btn.secondary:hover:not(:disabled) {
  background: var(--color-gray-hover);
}

.btn:disabled {
  opacity: 0.42;
  cursor: not-allowed;
}

.message {
  margin: 1rem 0 0;
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

.status-box {
  margin-top: 1.25rem;
  padding: 1rem 1.05rem;
  border-radius: var(--radius-md);
  font-size: 0.9rem;
}

.status-box.connected {
  background: var(--color-bg-accent-soft);
  border: 1px solid var(--color-success-border);
}

.status-box.idle {
  background: var(--color-bg-muted);
  border: 1px dashed var(--color-border-strong);
  color: var(--color-text-muted);
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
  color: var(--color-text);
}

.signature {
  font-family: ui-monospace, "Cascadia Mono", monospace;
  font-size: 0.82rem;
  color: var(--color-text);
}

.hint {
  margin: 0.75rem 0 0;
  font-size: 0.8rem;
  color: var(--color-text-subtle);
}

code {
  font-family: ui-monospace, monospace;
  background: var(--color-bg-elevated);
  color: var(--color-accent-hover);
  padding: 0.12em 0.4em;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border);
  font-size: 0.92em;
}
</style>
