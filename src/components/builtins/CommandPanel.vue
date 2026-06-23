<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useDataContext } from "../../core/data-context";
import { useRustComponent } from "../../composables/useRustComponent";
import { useComponentBinding } from "../../composables/useKeyboardRouter";
import { readProjectUiConfig } from "../../core/config-loader";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

async function loadQuickCommandsYaml(): Promise<Record<string, unknown>> {
  const rel = String(props.props.quickCommands ?? "console/quick-commands.yaml");
  try {
    return { quickCommandsYaml: await readProjectUiConfig(rel) };
  } catch (e) {
    console.warn("quick-commands.yaml load failed", e);
    return { quickCommandsYaml: "" };
  }
}

const { state, dispatch, error, ready, mounting } = useRustComponent(
  props.instance,
  props.path,
  loadQuickCommandsYaml,
);

const dataCtx = useDataContext();

interface QuickCommand {
  id: string;
  label: string;
  text: string;
  description?: string;
}

interface LogExchange {
  id: number;
  command: string;
  status: "pending" | "ok" | "err";
  lines?: string[];
  error?: string;
}

const inputText = ref("");
const historyIndex = ref(-1);
const logRef = ref<HTMLElement | null>(null);
const inputRef = ref<HTMLInputElement | null>(null);

const connected = computed(() => dataCtx.connection.value.connected);
const busy = computed(() => Boolean(state.value.busy));
const exchanges = computed(() => (state.value.exchanges as LogExchange[]) ?? []);
const history = computed(() => (state.value.history as string[]) ?? []);
const quickCommands = computed(() => (state.value.quickCommands as QuickCommand[]) ?? []);

const canSend = computed(
  () => connected.value && !busy.value && inputText.value.trim().length > 0,
);

watch(
  exchanges,
  async () => {
    await nextTick();
    const el = logRef.value;
    if (el) el.scrollTop = el.scrollHeight;
  },
  { deep: true },
);

function sendText(text: string): Promise<void> {
  const trimmed = text.trim();
  if (!trimmed || busy.value || !connected.value) return Promise.resolve();
  inputText.value = trimmed;
  historyIndex.value = -1;
  return dispatch("send", { text: trimmed }).then(() => {});
}

function send(): Promise<void> {
  return sendText(inputText.value);
}

function runQuick(cmd: QuickCommand): Promise<void> {
  return dispatch("run_quick", { id: cmd.id }).then(() => {});
}

function clearLog(): Promise<void> {
  return dispatch("clear_log").then(() => {});
}

function onHistoryKey(event: KeyboardEvent): void {
  if (event.key === "ArrowUp") {
    event.preventDefault();
    if (!history.value.length) return;
    const next =
      historyIndex.value < 0
        ? history.value.length - 1
        : Math.max(0, historyIndex.value - 1);
    historyIndex.value = next;
    inputText.value = history.value[next] ?? "";
    return;
  }
  if (event.key === "ArrowDown") {
    event.preventDefault();
    if (historyIndex.value < 0) return;
    const next = historyIndex.value + 1;
    if (next >= history.value.length) {
      historyIndex.value = -1;
      inputText.value = "";
    } else {
      historyIndex.value = next;
      inputText.value = history.value[next] ?? "";
    }
  }
}

function onComponentKeydown(event: KeyboardEvent): boolean {
  if (event.key === "Enter" && !event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey) {
    if (!canSend.value) return false;
    void send();
    return true;
  }
  if (event.key === "ArrowUp" || event.key === "ArrowDown") {
    onHistoryKey(event);
    return true;
  }
  return false;
}

useComponentBinding(props.path, onComponentKeydown);
</script>

<template>
  <div class="cmd-panel">
    <p v-if="mounting" class="cmd-hint">Загрузка…</p>

    <template v-else-if="ready">
      <header class="cmd-header">
        <div class="cmd-status" :data-mode="busy ? 'busy' : connected ? 'online' : 'offline'">
          <span class="cmd-status-dot" aria-hidden="true" />
          <span class="cmd-status-text">
            {{ busy ? "Отправка…" : connected ? "ECU online" : "Нет ECU" }}
          </span>
        </div>
        <button
          type="button"
          class="cmd-clear"
          :disabled="!exchanges.length || busy"
          title="Очистить лог"
          @click="clearLog"
        >
          Очистить
        </button>
      </header>

      <div v-if="quickCommands.length" class="cmd-quick">
        <p class="cmd-quick-label">Быстрые команды</p>
        <div class="cmd-chips">
          <button
            v-for="cmd in quickCommands"
            :key="cmd.id"
            type="button"
            class="cmd-chip"
            :title="cmd.description ?? cmd.text"
            :disabled="!connected || busy"
            @click="runQuick(cmd)"
          >
            {{ cmd.label }}
          </button>
        </div>
      </div>

      <div ref="logRef" class="cmd-log selectable" role="log" aria-live="polite" aria-relevant="additions">
        <p v-if="!exchanges.length" class="cmd-log-empty">
          Ответы ECU появятся здесь после отправки команды.
        </p>
        <article
          v-for="ex in exchanges"
          :key="ex.id"
          class="cmd-exchange"
          :data-status="ex.status"
        >
          <header class="cmd-exchange-head">
            <span class="cmd-exchange-badge">{{ ex.status === "pending" ? "…" : "›" }}</span>
            <code class="cmd-exchange-cmd">{{ ex.command }}</code>
            <span v-if="ex.status === 'pending'" class="cmd-exchange-meta">ожидание…</span>
          </header>
          <pre
            v-if="ex.status === 'ok' && (ex.lines?.length ?? 0) > 0"
            class="cmd-exchange-body"
          >{{ (ex.lines ?? []).join("\n") }}</pre>
          <p v-else-if="ex.status === 'err'" class="cmd-exchange-error">{{ ex.error ?? "ошибка" }}</p>
        </article>
      </div>

      <div class="cmd-input-row">
        <span class="cmd-prompt" aria-hidden="true">&gt;</span>
        <input
          ref="inputRef"
          v-model="inputText"
          type="text"
          class="cmd-input"
          data-nav-focus
          placeholder="help, sensorinfo, rpm 1500…"
          :disabled="!connected || busy"
          spellcheck="false"
          autocomplete="off"
        />
        <button type="button" class="cmd-send" :disabled="!canSend" @click="send">
          {{ busy ? "…" : "Send" }}
        </button>
      </div>

      <p v-if="error" class="cmd-error">{{ error }}</p>
      <p v-else class="cmd-footnote">Enter — отправить · ↑/↓ — история · ←/→ — между панелями</p>
    </template>
  </div>
</template>

<style scoped>
.cmd-panel {
  display: flex;
  flex-direction: column;
  gap: 0.65rem;
  width: 100%;
  min-width: 0;
  min-height: 0;
  height: 100%;
  box-sizing: border-box;
}

.cmd-header,
.cmd-quick,
.cmd-input-row,
.cmd-footnote,
.cmd-error {
  flex-shrink: 0;
}

.cmd-hint {
  margin: 0;
  color: var(--color-text-muted);
  font-size: 0.9rem;
}

.cmd-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
}

.cmd-status {
  display: inline-flex;
  align-items: center;
  gap: 0.45rem;
  font-size: 0.82rem;
  font-weight: 500;
  color: var(--color-text-muted);
}

.cmd-status-dot {
  width: 0.55rem;
  height: 0.55rem;
  border-radius: 50%;
  background: var(--color-text-muted);
}

.cmd-status[data-mode="online"] .cmd-status-dot {
  background: #2ecc71;
  box-shadow: 0 0 0 2px color-mix(in srgb, #2ecc71 25%, transparent);
}

.cmd-status[data-mode="busy"] .cmd-status-dot {
  background: #f39c12;
  animation: cmd-pulse 1s ease-in-out infinite;
}

.cmd-status[data-mode="offline"] .cmd-status-dot {
  background: var(--color-border);
}

@keyframes cmd-pulse {
  50% {
    opacity: 0.45;
  }
}

.cmd-clear {
  border: 1px solid var(--color-border);
  background: transparent;
  color: var(--color-text-muted);
  font-size: 0.78rem;
  padding: 0.25rem 0.6rem;
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.cmd-clear:hover:not(:disabled) {
  color: var(--color-text);
  border-color: var(--color-text-muted);
}

.cmd-clear:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.cmd-quick-label {
  margin: 0 0 0.45rem;
  font-size: 0.72rem;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-text-muted);
}

.cmd-chips {
  display: flex;
  flex-wrap: nowrap;
  gap: 0.35rem;
  overflow-x: auto;
  padding-bottom: 0.15rem;
  scrollbar-width: thin;
}

.cmd-chip {
  padding: 0.28rem 0.62rem;
  border: 1px solid var(--color-border);
  border-radius: 999px;
  background: color-mix(in srgb, var(--color-bg) 70%, var(--color-bg-elevated));
  color: var(--color-text);
  font-size: 0.76rem;
  font-weight: 500;
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s;
}

.cmd-chip:hover:not(:disabled) {
  border-color: var(--color-accent);
  background: color-mix(in srgb, var(--color-accent) 8%, var(--color-bg-elevated));
}

.cmd-chip:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.cmd-log {
  flex: 1 1 0;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior: contain;
  padding: 0.55rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: color-mix(in srgb, var(--color-bg) 50%, #0d1117);
  display: flex;
  flex-direction: column;
  gap: 0.45rem;
  scrollbar-width: thin;
}

.cmd-log-empty {
  margin: auto 0.5rem;
  color: var(--color-text-muted);
  font-style: italic;
  font-size: 0.82rem;
  text-align: center;
}

.cmd-exchange {
  flex: 0 0 auto;
  border-radius: var(--radius-sm);
  border: 1px solid color-mix(in srgb, var(--color-border) 80%, transparent);
  background: color-mix(in srgb, var(--color-bg-elevated) 40%, #161b22);
  overflow: hidden;
}

.cmd-exchange[data-status="pending"] {
  border-color: color-mix(in srgb, #f39c12 35%, var(--color-border));
}

.cmd-exchange[data-status="ok"] {
  border-color: color-mix(in srgb, #2ecc71 22%, var(--color-border));
}

.cmd-exchange[data-status="err"] {
  border-color: color-mix(in srgb, #ff7b72 35%, var(--color-border));
}

.cmd-exchange-head {
  display: flex;
  align-items: center;
  gap: 0.45rem;
  padding: 0.35rem 0.55rem;
  background: color-mix(in srgb, var(--color-accent) 6%, transparent);
}

.cmd-exchange:not(:has(.cmd-exchange-body)):not(:has(.cmd-exchange-error)) .cmd-exchange-head {
  border-bottom: none;
}

.cmd-exchange:has(.cmd-exchange-body) .cmd-exchange-head,
.cmd-exchange:has(.cmd-exchange-error) .cmd-exchange-head {
  border-bottom: 1px solid color-mix(in srgb, var(--color-border) 70%, transparent);
}

.cmd-exchange-meta {
  margin-left: auto;
  font-size: 0.72rem;
  color: var(--color-text-muted);
  font-style: italic;
}

.cmd-exchange-badge {
  flex-shrink: 0;
  width: 1.1rem;
  height: 1.1rem;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
  font-size: 0.72rem;
  font-weight: 700;
  color: var(--color-accent);
  background: color-mix(in srgb, var(--color-accent) 12%, transparent);
}

.cmd-exchange-cmd {
  font-family: var(--font-mono, ui-monospace, monospace);
  font-size: 0.8rem;
  font-weight: 600;
  color: #79c0ff;
}

.cmd-exchange-body {
  margin: 0;
  padding: 0.45rem 0.65rem;
  max-height: 9rem;
  overflow-x: hidden;
  overflow-y: auto;
  font-family: var(--font-mono, ui-monospace, "Cascadia Code", monospace);
  font-size: 0.76rem;
  line-height: 1.45;
  color: #c9d1d9;
  white-space: pre-wrap;
  word-break: break-word;
  tab-size: 2;
  scrollbar-width: thin;
}

.cmd-exchange-error {
  margin: 0;
  padding: 0.55rem 0.65rem;
  font-size: 0.78rem;
  color: #ff7b72;
}

.cmd-input-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.35rem 0.5rem 0.35rem 0.65rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
}

.cmd-prompt {
  color: var(--color-accent);
  font-family: var(--font-mono, ui-monospace, monospace);
  font-weight: 700;
  user-select: none;
}

.cmd-input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  color: var(--color-text);
  font-family: var(--font-mono, ui-monospace, monospace);
  font-size: 0.85rem;
  outline: none;
}

.cmd-input:disabled {
  opacity: 0.55;
}

.cmd-send {
  flex-shrink: 0;
  border: none;
  border-radius: var(--radius-sm);
  padding: 0.45rem 0.9rem;
  background: var(--color-accent);
  color: #fff;
  font-size: 0.82rem;
  font-weight: 600;
  cursor: pointer;
}

.cmd-send:hover:not(:disabled) {
  filter: brightness(1.08);
}

.cmd-send:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.cmd-error {
  margin: 0;
  font-size: 0.82rem;
  color: var(--color-error);
}

.cmd-footnote {
  margin: 0;
  font-size: 0.72rem;
  color: var(--color-text-muted);
}
</style>
