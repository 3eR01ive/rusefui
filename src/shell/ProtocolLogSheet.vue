<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import { sourceLabel, useProtocolLog } from "../composables/useProtocolLog";

const { entries, logPath, filters, open, clear, formatTime, setFilters } =
  useProtocolLog();
const listEl = ref<HTMLElement | null>(null);

watch(
  () => entries.value.length,
  async () => {
    if (!open.value) return;
    await nextTick();
    if (listEl.value) {
      listEl.value.scrollTop = listEl.value.scrollHeight;
    }
  },
);

function close() {
  open.value = false;
}

function dirLabel(direction: string): string {
  switch (direction) {
    case "tx":
      return "TX";
    case "rx":
      return "RX";
    case "err":
      return "ERR";
    case "link":
      return "LINK";
    default:
      return direction.toUpperCase();
  }
}

type LevelFilterKey = "error" | "warn" | "info" | "debug" | "trace";

type SourceFilterKey =
  | "commands"
  | "output"
  | "trigger"
  | "spectrogram"
  | "config";

const levelFilterOptions: { key: LevelFilterKey; label: string }[] = [
  { key: "error", label: "Error" },
  { key: "warn", label: "Warn" },
  { key: "info", label: "Info" },
  { key: "debug", label: "Debug" },
  { key: "trace", label: "Trace" },
];

const sourceFilterOptions: {
  key: SourceFilterKey;
  label: string;
  hint?: string;
}[] = [
  { key: "commands", label: "Команды", hint: "по умолчанию" },
  { key: "output", label: "Output (O)" },
  { key: "trigger", label: "Trigger / composite" },
  { key: "spectrogram", label: "Spectrogram (knock)" },
  { key: "config", label: "Config (R)" },
];

function onLevelToggle(key: LevelFilterKey, checked: boolean) {
  void setFilters({ ...filters.value, [key]: checked });
}

function onSourceToggle(key: SourceFilterKey, checked: boolean) {
  void setFilters({ ...filters.value, [key]: checked });
}
</script>

<template>
  <Teleport to="body">
    <Transition name="protocol-sheet">
      <div v-if="open" class="protocol-overlay" @click.self="close">
        <section class="protocol-sheet" role="dialog" aria-label="Протокол ECU">
          <header class="sheet-header">
            <div>
              <h2 class="sheet-title">Протокол ECU</h2>
              <p class="sheet-path">{{ logPath }}</p>
            </div>
            <div class="sheet-actions">
              <button type="button" class="btn ghost" @click="clear">Очистить UI</button>
              <button type="button" class="btn ghost" @click="close">Закрыть</button>
            </div>
          </header>

          <div class="sheet-filters" aria-label="Фильтры лога">
            <div class="filter-group">
              <span class="filter-group-label">Уровни</span>
              <label
                v-for="opt in levelFilterOptions"
                :key="opt.key"
                class="filter-chip"
              >
                <input
                  type="checkbox"
                  :checked="filters[opt.key]"
                  @change="
                    onLevelToggle(
                      opt.key,
                      ($event.target as HTMLInputElement).checked,
                    )
                  "
                />
                <span>{{ opt.label }}</span>
              </label>
            </div>
            <div class="filter-group">
              <span class="filter-group-label">Источники</span>
              <label
                v-for="opt in sourceFilterOptions"
                :key="opt.key"
                class="filter-chip"
                :title="opt.hint"
              >
                <input
                  type="checkbox"
                  :checked="filters[opt.key]"
                  @change="
                    onSourceToggle(
                      opt.key,
                      ($event.target as HTMLInputElement).checked,
                    )
                  "
                />
                <span>{{ opt.label }}</span>
                <span v-if="opt.hint" class="filter-hint">{{ opt.hint }}</span>
              </label>
            </div>
          </div>

          <div ref="listEl" class="sheet-list">
            <p v-if="!entries.length" class="empty">
              Пока нет записей. Включите источники (Spectrogram, Output, …) или выполните команды.
            </p>
            <article
              v-for="entry in entries"
              :key="entry.id"
              class="log-row"
              :class="[entry.direction, entry.level, `src-${entry.source}`]"
            >
              <div class="log-meta">
                <span class="log-source">{{ sourceLabel(entry.source) }}</span>
                <span class="log-level">{{ entry.level }}</span>
                <span class="log-dir">{{ dirLabel(entry.direction) }}</span>
                <span class="log-time">{{ formatTime(entry.timestampMs) }}</span>
                <span v-if="entry.command" class="log-cmd">{{ entry.command }}</span>
                <span v-if="entry.responseCode != null" class="log-code">
                  0x{{ entry.responseCode.toString(16).padStart(2, "0") }}
                </span>
              </div>
              <p class="log-summary">{{ entry.summary }}</p>
              <p v-if="entry.payloadHex" class="log-hex">payload: {{ entry.payloadHex }}</p>
              <p v-if="entry.frameHex" class="log-hex">frame: {{ entry.frameHex }}</p>
            </article>
          </div>
        </section>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.protocol-overlay {
  position: fixed;
  inset: 0;
  z-index: 1000;
  background: rgba(58, 53, 48, 0.28);
  display: flex;
  align-items: flex-end;
  justify-content: center;
}

.protocol-sheet {
  width: min(960px, 100%);
  max-height: min(70vh, 640px);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-strong);
  border-bottom: none;
  border-radius: var(--radius-lg) var(--radius-lg) 0 0;
  box-shadow: 0 -8px 32px rgba(58, 53, 48, 0.12);
  display: flex;
  flex-direction: column;
}

.sheet-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem;
  padding: 1rem 1.25rem 0.65rem;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.sheet-title {
  margin: 0;
  font-size: 1rem;
  font-weight: 600;
}

.sheet-path {
  margin: 0.25rem 0 0;
  font-size: 0.75rem;
  font-family: ui-monospace, monospace;
  color: var(--color-text-subtle);
  word-break: break-all;
}

.sheet-actions {
  display: flex;
  gap: 0.5rem;
  flex-shrink: 0;
}

.sheet-filters {
  display: flex;
  flex-direction: column;
  gap: 0.55rem;
  padding: 0.65rem 1.25rem;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.filter-group {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.45rem 0.75rem;
}

.filter-group-label {
  font-size: 0.72rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-text-subtle);
  min-width: 4.5rem;
}

.filter-chip {
  display: inline-flex;
  align-items: center;
  gap: 0.3rem;
  font-size: 0.78rem;
  color: var(--color-text-muted);
  cursor: pointer;
  user-select: none;
}

.filter-chip input {
  accent-color: var(--color-accent);
}

.filter-hint {
  font-size: 0.68rem;
  color: var(--color-text-subtle);
}

.btn.ghost {
  padding: 0.35rem 0.65rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border-strong);
  background: transparent;
  color: var(--color-text-muted);
  font-size: 0.82rem;
}

.sheet-list {
  overflow: auto;
  padding: 0.75rem 1.25rem 1.25rem;
  flex: 1;
}

.empty {
  margin: 0;
  color: var(--color-text-subtle);
  font-size: 0.9rem;
}

.log-row {
  padding: 0.55rem 0.65rem;
  margin-bottom: 0.45rem;
  border-radius: var(--radius-sm);
  border-left: 3px solid var(--color-border-strong);
  background: var(--color-bg-muted);
  font-size: 0.82rem;
}

.log-row.tx {
  border-left-color: var(--color-accent);
}

.log-row.rx {
  border-left-color: #5a9a6e;
}

.log-row.err,
.log-row.error {
  border-left-color: var(--color-error);
  background: var(--color-error-bg);
}

.log-row.info,
.log-row.link {
  border-left-color: #6b8cae;
}

.log-row.link {
  background: var(--color-bg-accent-soft);
}

.log-row.src-spectrogram {
  border-left-color: #9b6bb8;
}

.log-row.src-trigger {
  border-left-color: #b89a4a;
}

.log-row.src-output {
  border-left-color: #6b8cae;
}

.log-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 0.45rem 0.75rem;
  align-items: center;
  margin-bottom: 0.25rem;
}

.log-source {
  font-size: 0.68rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  color: var(--color-accent);
}

.log-level {
  font-size: 0.68rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-text-subtle);
}

.log-dir {
  font-weight: 700;
  font-size: 0.72rem;
  letter-spacing: 0.04em;
}

.log-time {
  font-family: ui-monospace, monospace;
  color: var(--color-text-subtle);
}

.log-cmd,
.log-code {
  font-family: ui-monospace, monospace;
  color: var(--color-text-muted);
}

.log-summary {
  margin: 0;
  color: var(--color-text);
}

.log-hex {
  margin: 0.2rem 0 0;
  font-family: ui-monospace, monospace;
  font-size: 0.75rem;
  color: var(--color-text-subtle);
  word-break: break-all;
}

.protocol-sheet-enter-active,
.protocol-sheet-leave-active {
  transition: opacity 0.2s ease;
}

.protocol-sheet-enter-active .protocol-sheet,
.protocol-sheet-leave-active .protocol-sheet {
  transition: transform 0.28s ease;
}

.protocol-sheet-enter-from,
.protocol-sheet-leave-to {
  opacity: 0;
}

.protocol-sheet-enter-from .protocol-sheet,
.protocol-sheet-leave-to .protocol-sheet {
  transform: translateY(100%);
}
</style>
