<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import {
  applyIniPath,
  initIniResolution,
  listIniCandidates,
  pickIniFile,
  retryOnlineIniDownload,
  useIniResolution,
  type IniCandidate,
} from "../composables/useIniResolution";
import { useEcuConnection } from "../composables/useEcuConnection";
import { useDataContext } from "../core/data-context";

const dataCtx = useDataContext();
const { setOfflineMode } = useEcuConnection(dataCtx);

const {
  info,
  ecuSignature,
  projectSignature,
  projectMismatch,
} = useIniResolution();

const candidates = ref<IniCandidate[]>([]);
const selectedPath = ref<string | null>(null);
const busy = ref(false);
const actionError = ref<string | null>(null);
const onlineBusy = ref(false);

const matchingCandidate = computed(() =>
  candidates.value.find((c) => c.matchesEcu) ?? null,
);
const selectedCandidate = computed(() =>
  candidates.value.find((c) => c.path === selectedPath.value) ?? null,
);

const recommendOnline = computed(() => {
  const o = info.value.online;
  if (!o) return false;
  return o.kind === "notAttempted" || o.kind === "failed";
});

const onlineCanRetry = computed(() => {
  const o = info.value.online;
  return !!o && o.kind !== "notApplicable";
});

const sourceLabel: Record<IniCandidate["source"], string> = {
  envOverride: "ENV",
  cache: "кэш",
  localDir: "локально",
};

async function refreshCandidates(): Promise<void> {
  candidates.value = await listIniCandidates();
  if (selectedPath.value) return;
  const suggested = info.value.suggestedIniPath;
  if (suggested && candidates.value.some((c) => c.path === suggested)) {
    selectedPath.value = suggested;
    return;
  }
  if (matchingCandidate.value) {
    selectedPath.value = matchingCandidate.value.path;
  }
}

onMounted(async () => {
  await initIniResolution();
  await refreshCandidates();
});

watch(
  () => info.value.ecuSignature,
  () => {
    selectedPath.value = null;
    void refreshCandidates();
  },
);

async function withBusy(fn: () => Promise<void>) {
  busy.value = true;
  actionError.value = null;
  try {
    await fn();
  } catch (e) {
    actionError.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = false;
  }
}

function onApplySelected() {
  if (!selectedCandidate.value) return;
  void withBusy(async () => {
    await applyIniPath(selectedCandidate.value!.path, false);
  });
}

function onApplyForced() {
  if (!selectedCandidate.value) return;
  void withBusy(async () => {
    await applyIniPath(selectedCandidate.value!.path, true);
  });
}

function onRetryOnline() {
  if (onlineBusy.value) return;
  onlineBusy.value = true;
  actionError.value = null;
  void (async () => {
    try {
      await retryOnlineIniDownload();
      await refreshCandidates();
    } catch (e) {
      actionError.value = e instanceof Error ? e.message : String(e);
    } finally {
      onlineBusy.value = false;
    }
  })();
}

function onPickFile() {
  void (async () => {
    const picked = await pickIniFile();
    if (!picked) return;
    selectedPath.value = picked;
    await withBusy(async () => {
      await applyIniPath(picked, false);
    });
  })();
}

function onGoOffline() {
  void setOfflineMode(true);
}

function fmtSize(bytes: number): string {
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(0)} KB`;
  return `${(kb / 1024).toFixed(1)} MB`;
}
</script>

<template>
  <div class="mismatch-screen">
    <div class="mismatch-card">
      <header class="mm-header">
        <div class="mm-title-row">
          <span class="mm-pill">ECU подключена</span>
          <h1 class="mm-title">Нужен подходящий INI</h1>
        </div>
        <p class="mm-subtitle">
          Signature ECU не совпадает с INI в проекте, либо подходящий файл ещё не выбран.
          Подтвердите INI для этой прошивки — только после этого откроется слияние config с ECU.
        </p>
        <p v-if="info.suggestedIniPath" class="mm-hint-suggested">
          Рекомендуемый INI уже найден автоматически — нажмите «Применить выбранный».
        </p>
      </header>

      <section class="mm-section">
        <h2 class="mm-section-title">Что мы знаем</h2>
        <div class="mm-grid">
          <div class="mm-field">
            <span class="mm-field-label">Signature ECU</span>
            <code class="mm-code">{{ ecuSignature ?? "—" }}</code>
            <span v-if="info.bundleTarget" class="mm-meta">target: {{ info.bundleTarget }}</span>
          </div>
          <div class="mm-field">
            <span class="mm-field-label">Порт</span>
            <code class="mm-code">{{ info.portName ?? "—" }}</code>
          </div>
          <div v-if="projectSignature" class="mm-field">
            <span class="mm-field-label">Signature из проекта</span>
            <code class="mm-code" :class="{ 'mm-code--warn': projectMismatch }">
              {{ projectSignature }}
            </code>
            <span v-if="projectMismatch" class="mm-meta mm-meta--warn">
              отличается от ECU
            </span>
          </div>
          <div v-if="info.lastError" class="mm-field mm-field--wide">
            <span class="mm-field-label">Причина</span>
            <code class="mm-code mm-code--muted">{{ info.lastError }}</code>
          </div>
        </div>
      </section>

      <section class="mm-section">
        <div class="mm-section-row">
          <h2 class="mm-section-title">Online-загрузка с rusefi.com</h2>
          <button
            type="button"
            class="mm-btn mm-btn-ghost"
            :disabled="onlineBusy || !onlineCanRetry"
            @click="onRetryOnline"
          >
            {{ onlineBusy ? "Загрузка…" : "Скачать снова" }}
          </button>
        </div>
        <div class="mm-online">
          <template v-if="info.online">
            <template v-if="info.online.kind === 'notApplicable'">
              <span class="mm-tag mm-tag--muted">недоступно</span>
              <span class="mm-online-text">Signature ECU не парсится — URL построить нельзя.</span>
            </template>
            <template v-else-if="info.online.kind === 'notAttempted'">
              <span class="mm-tag mm-tag--muted">не пробовали</span>
              <span class="mm-online-text">{{ info.online.reason }}</span>
            </template>
            <template v-else-if="info.online.kind === 'succeeded'">
              <span class="mm-tag mm-tag--ok">скачано</span>
              <code class="mm-code mm-code--inline">{{ info.online.path }}</code>
            </template>
            <template v-else-if="info.online.kind === 'failed'">
              <span class="mm-tag mm-tag--err">ошибка</span>
              <span class="mm-online-text">{{ info.online.error }}</span>
              <code class="mm-code mm-code--inline mm-code--muted">{{ info.online.url }}</code>
            </template>
          </template>
          <template v-else>
            <span class="mm-tag mm-tag--muted">нет данных</span>
          </template>
          <span v-if="recommendOnline" class="mm-online-hint">
            Если ECU прошита из нестабильной ветки, скачивание может вернуть ту же ошибку.
          </span>
        </div>
      </section>

      <section class="mm-section">
        <div class="mm-section-row">
          <h2 class="mm-section-title">Локальные INI ({{ candidates.length }})</h2>
          <button
            type="button"
            class="mm-btn mm-btn-ghost"
            :disabled="busy"
            @click="onPickFile"
          >
            Выбрать файл…
          </button>
        </div>
        <div v-if="candidates.length === 0" class="mm-empty">
          Ничего не найдено. Подключите каталог через
          <code class="mm-code mm-code--inline">RUSEFI_INI_DIR</code>
          или выберите файл вручную.
        </div>
        <ul v-else class="mm-cands">
          <li
            v-for="c in candidates"
            :key="c.path"
            class="mm-cand"
            :class="{
              'mm-cand--active': selectedPath === c.path,
              'mm-cand--match': c.matchesEcu,
            }"
            @click="selectedPath = c.path"
          >
            <div class="mm-cand-head">
              <span class="mm-cand-name">{{ c.fileName }}</span>
              <span class="mm-cand-tags">
                <span v-if="c.matchesEcu" class="mm-tag mm-tag--ok">match</span>
                <span class="mm-tag mm-tag--muted">{{ sourceLabel[c.source] }}</span>
                <span v-if="c.bundleTarget" class="mm-tag mm-tag--info">
                  {{ c.bundleTarget }}
                </span>
                <span class="mm-tag mm-tag--muted">{{ fmtSize(c.sizeBytes) }}</span>
              </span>
            </div>
            <code class="mm-cand-path">{{ c.path }}</code>
            <code v-if="c.signature" class="mm-cand-sig">{{ c.signature }}</code>
          </li>
        </ul>
      </section>

      <p v-if="actionError" class="mm-error">{{ actionError }}</p>

      <footer class="mm-actions">
        <button
          type="button"
          class="mm-btn mm-btn-secondary"
          :disabled="busy"
          @click="onGoOffline"
        >
          Перейти в offline
        </button>
        <span class="mm-spacer" />
        <button
          v-if="selectedCandidate && !selectedCandidate.matchesEcu"
          type="button"
          class="mm-btn mm-btn-warn"
          :disabled="busy"
          :title="`Применить ${selectedCandidate.fileName} без проверки signature`"
          @click="onApplyForced"
        >
          Использовать всё равно
        </button>
        <button
          type="button"
          class="mm-btn mm-btn-primary"
          :disabled="busy || !selectedCandidate"
          :title="
            selectedCandidate
              ? `Применить ${selectedCandidate.fileName}`
              : 'Выберите кандидат сверху'
          "
          @click="onApplySelected"
        >
          {{ busy ? "Применяем…" : "Применить выбранный" }}
        </button>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.mismatch-screen {
  position: fixed;
  inset: 0;
  z-index: 9500;
  background: var(--color-bg);
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding: 2.5rem 1.25rem;
  overflow-y: auto;
  box-sizing: border-box;
}

.mismatch-card {
  width: min(56rem, 100%);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card);
  padding: 1.75rem 2rem;
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}

.mm-header {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}

.mm-title-row {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  flex-wrap: wrap;
}

.mm-pill {
  font-size: 0.7rem;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  padding: 0.2rem 0.55rem;
  border-radius: 999px;
  background: var(--color-bg-accent-soft);
  color: var(--color-accent-hover);
  border: 1px solid var(--color-success-border);
}

.mm-title {
  margin: 0;
  font-size: 1.5rem;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--color-text);
}

.mm-subtitle {
  margin: 0;
  font-size: 0.92rem;
  color: var(--color-text-muted);
  line-height: 1.45;
  max-width: 50rem;
}

.mm-hint-suggested {
  margin: 0.35rem 0 0;
  font-size: 0.85rem;
  color: var(--color-accent-hover);
}

.mm-section {
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
  border-top: 1px solid var(--color-border);
  padding-top: 1rem;
}

.mm-section-title {
  margin: 0;
  font-size: 0.78rem;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--color-text-subtle);
}

.mm-section-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
}

.mm-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(15rem, 1fr));
  gap: 0.75rem 1.25rem;
}

.mm-field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  min-width: 0;
}

.mm-field--wide {
  grid-column: 1 / -1;
}

.mm-field-label {
  font-size: 0.7rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-text-subtle);
}

.mm-code {
  font-family: var(--font-mono, ui-monospace, SFMono-Regular, monospace);
  font-size: 0.84rem;
  word-break: break-all;
  color: var(--color-text);
  background: var(--color-bg-muted);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  padding: 0.3rem 0.5rem;
}

.mm-code--inline {
  display: inline-block;
  padding: 0.15rem 0.4rem;
  font-size: 0.78rem;
}

.mm-code--warn {
  border-color: var(--color-warning, #c97a18);
  color: var(--color-warning, #c97a18);
  background: color-mix(in srgb, var(--color-warning, #c97a18) 12%, transparent);
}

.mm-code--muted {
  color: var(--color-text-muted);
}

.mm-meta {
  font-size: 0.7rem;
  color: var(--color-text-subtle);
}

.mm-meta--warn {
  color: var(--color-warning, #c97a18);
  font-weight: 600;
}

.mm-online {
  display: flex;
  align-items: center;
  gap: 0.55rem;
  flex-wrap: wrap;
}

.mm-online-text {
  font-size: 0.85rem;
  color: var(--color-text-muted);
}

.mm-online-hint {
  flex-basis: 100%;
  font-size: 0.78rem;
  color: var(--color-text-subtle);
}

.mm-tag {
  display: inline-flex;
  align-items: center;
  font-size: 0.68rem;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  padding: 0.15rem 0.45rem;
  border-radius: 999px;
  border: 1px solid var(--color-border);
}

.mm-tag--muted {
  background: var(--color-bg-muted);
  color: var(--color-text-muted);
}

.mm-tag--ok {
  background: color-mix(in srgb, var(--color-accent) 16%, transparent);
  color: var(--color-accent-hover);
  border-color: var(--color-success-border);
}

.mm-tag--err {
  background: color-mix(in srgb, #c0392b 18%, transparent);
  color: #c0392b;
  border-color: color-mix(in srgb, #c0392b 35%, transparent);
}

.mm-tag--info {
  background: color-mix(in srgb, #2563eb 14%, transparent);
  color: #1d4ed8;
  border-color: color-mix(in srgb, #2563eb 30%, transparent);
}

.mm-cands {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.45rem;
  max-height: 22rem;
  overflow-y: auto;
}

.mm-cand {
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
  padding: 0.65rem 0.85rem;
  background: var(--color-bg-muted);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s;
}

.mm-cand:hover {
  border-color: var(--color-border-strong);
}

.mm-cand--active {
  border-color: var(--color-accent);
  background: var(--color-bg-accent-soft);
}

.mm-cand--match {
  border-left: 3px solid var(--color-accent);
}

.mm-cand-head {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  justify-content: space-between;
  flex-wrap: wrap;
}

.mm-cand-name {
  font-weight: 600;
  color: var(--color-text);
}

.mm-cand-tags {
  display: inline-flex;
  align-items: center;
  gap: 0.3rem;
  flex-wrap: wrap;
}

.mm-cand-path {
  font-family: var(--font-mono, ui-monospace, SFMono-Regular, monospace);
  font-size: 0.74rem;
  color: var(--color-text-subtle);
  word-break: break-all;
}

.mm-cand-sig {
  font-family: var(--font-mono, ui-monospace, SFMono-Regular, monospace);
  font-size: 0.74rem;
  color: var(--color-text-muted);
  word-break: break-all;
}

.mm-empty {
  font-size: 0.85rem;
  color: var(--color-text-muted);
  padding: 0.5rem 0;
}

.mm-error {
  margin: 0;
  padding: 0.55rem 0.85rem;
  font-size: 0.85rem;
  color: #c0392b;
  background: color-mix(in srgb, #c0392b 10%, transparent);
  border: 1px solid color-mix(in srgb, #c0392b 30%, transparent);
  border-radius: var(--radius-sm);
}

.mm-actions {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  border-top: 1px solid var(--color-border);
  padding-top: 1rem;
  flex-wrap: wrap;
}

.mm-spacer {
  flex: 1 1 0;
}

.mm-btn {
  display: inline-flex;
  align-items: center;
  gap: 0.4rem;
  padding: 0.5rem 1rem;
  font-size: 0.82rem;
  font-weight: 600;
  letter-spacing: 0.03em;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border);
  background: var(--color-bg-muted);
  color: var(--color-text);
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s, color 0.15s;
}

.mm-btn:hover:not(:disabled) {
  border-color: var(--color-border-strong);
}

.mm-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.mm-btn-primary {
  background: var(--color-accent);
  border-color: var(--color-accent);
  color: #fff;
}

.mm-btn-primary:hover:not(:disabled) {
  background: var(--color-accent-hover);
  border-color: var(--color-accent-hover);
}

.mm-btn-secondary {
  background: transparent;
}

.mm-btn-ghost {
  background: transparent;
  font-weight: 500;
}

.mm-btn-warn {
  background: transparent;
  border-color: var(--color-warning, #c97a18);
  color: var(--color-warning, #c97a18);
}

.mm-btn-warn:hover:not(:disabled) {
  background: color-mix(in srgb, var(--color-warning, #c97a18) 12%, transparent);
}
</style>
