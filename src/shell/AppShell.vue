<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import TabWorkspace from "./TabWorkspace.vue";
import ProtocolLogSheet from "./ProtocolLogSheet.vue";
import ConfigLoadOverlay from "./ConfigLoadOverlay.vue";
import ConfigDiffModal from "./ConfigDiffModal.vue";
import { createDataContext, provideDataContext } from "../core/data-context";
import { initOutputChannels } from "../composables/useOutputChannels";
import {
  initOutputTimeline,
  pickAndLoadTimelineFile,
} from "../composables/useOutputTimeline";
import { initConfig, useConfig } from "../composables/useConfig";
import { useProtocolLog, useProtocolLogLifecycle } from "../composables/useProtocolLog";
import { useEcuConnection } from "../composables/useEcuConnection";
import { initProject, useProject } from "../composables/useProject";

const dataCtx = createDataContext();
provideDataContext(dataCtx);

const appTitle = ref("rusefui");
const { offlineMode, scanning, busyPorts, setOfflineMode } = useEcuConnection(dataCtx);
const { togglePanel } = useProtocolLog();
const { snapshot: configSnap, burn: burnConfig } = useConfig();
const {
  info: projectInfo,
  createNewProject,
  openProject,
  saveProject,
  saveProjectAs,
  captureEcuConfig,
} = useProject();

const burning = ref(false);
const burnError = ref<string | null>(null);
const loadingLog = ref(false);
const logLoadError = ref<string | null>(null);
const projectError = ref<string | null>(null);
const projectBusy = ref(false);

async function onOpenOutputLog(): Promise<void> {
  logLoadError.value = null;
  loadingLog.value = true;
  try {
    const st = await pickAndLoadTimelineFile();
    if (!st) return;
  } catch (e) {
    logLoadError.value = e instanceof Error ? e.message : String(e);
  } finally {
    loadingLog.value = false;
  }
}

const canBurn = computed(
  () =>
    dataCtx.connection.value.connected &&
    configSnap.value.loaded &&
    !configSnap.value.loading &&
    !configSnap.value.readOnly &&
    !burning.value,
);

useProtocolLogLifecycle();

onMounted(() => {
  void initProject();
  void initOutputChannels();
  void initOutputTimeline();
  void initConfig();
});

async function runProjectAction(fn: () => Promise<void>): Promise<void> {
  projectError.value = null;
  projectBusy.value = true;
  try {
    await fn();
  } catch (e) {
    projectError.value = e instanceof Error ? e.message : String(e);
  } finally {
    projectBusy.value = false;
  }
}

function onNewProject(): void {
  void runProjectAction(async () => {
    await createNewProject();
  });
}

function onOpenProject(): void {
  void runProjectAction(async () => {
    await openProject();
  });
}

function onSaveProject(): void {
  void runProjectAction(async () => {
    await saveProject();
  });
}

function onSaveProjectAs(): void {
  void runProjectAction(async () => {
    await saveProjectAs();
  });
}

function onCaptureConfigToProject(): void {
  void runProjectAction(() => captureEcuConfig());
}

async function onBurn() {
  if (!canBurn.value) return;
  burning.value = true;
  burnError.value = null;
  try {
    await burnConfig();
  } catch (e) {
    burnError.value = e instanceof Error ? e.message : String(e);
  } finally {
    burning.value = false;
  }
}
</script>

<template>
  <div class="app-shell">
    <header class="app-header">
      <div class="brand-mark" aria-hidden="true" />
      <div>
        <h1 class="app-title">{{ appTitle }}</h1>
        <span class="app-subtitle">rusEFI · декларативный UI</span>
      </div>
      <div class="header-actions">
        <div class="project-bar" :title="projectInfo.path ?? 'Файл не сохранён'">
          <span class="project-name">
            {{ projectInfo.name }}
            <span v-if="projectInfo.dirty" class="project-dirty">●</span>
          </span>
          <div class="project-btns">
            <button
              type="button"
              class="log-btn"
              :disabled="projectBusy"
              title="Новый проект"
              @click="onNewProject"
            >
              Новый
            </button>
            <button
              type="button"
              class="log-btn"
              :disabled="projectBusy"
              title="Открыть проект (.json)"
              @click="onOpenProject"
            >
              Открыть
            </button>
            <button
              type="button"
              class="log-btn"
              :disabled="projectBusy"
              title="Сохранить проект"
              @click="onSaveProject"
            >
              Сохранить
            </button>
            <button
              type="button"
              class="log-btn"
              :disabled="projectBusy"
              title="Сохранить проект как…"
              @click="onSaveProjectAs"
            >
              Сохр. как
            </button>
            <button
              type="button"
              class="log-btn"
              :disabled="projectBusy || !dataCtx.connection.value.connected"
              title="Скопировать текущий config page 0 с ECU в проект"
              @click="onCaptureConfigToProject"
            >
              Config→проект
            </button>
          </div>
        </div>
        <label class="offline-toggle" title="Не подключаться к ECU автоматически">
          <input
            type="checkbox"
            :checked="offlineMode"
            @change="setOfflineMode(($event.target as HTMLInputElement).checked)"
          />
          <span>Offline mode</span>
        </label>
        <span v-if="scanning && !offlineMode" class="scan-hint" aria-live="polite">
          Поиск ECU…
        </span>
        <span v-else-if="busyPorts.length && !offlineMode" class="scan-hint busy" aria-live="polite">
          Порт занят ({{ busyPorts.join(", ") }}) — отключите TunerStudio
        </span>
        <button
          type="button"
          class="log-btn"
          :disabled="loadingLog"
          title="Открыть CSV-лог output channels (офлайн просмотр)"
          @click="onOpenOutputLog"
        >
          {{ loadingLog ? "Лог…" : "Открыть лог" }}
        </button>
        <button
          type="button"
          class="log-btn"
          title="Лог USB, подключения и протокола ECU"
          @click="togglePanel"
        >
          Протокол
        </button>
        <template v-if="dataCtx.connection.value.connected">
          <button
            type="button"
            class="burn-btn"
            :disabled="!canBurn"
            :title="
              burnError ??
              'Записать конфигурацию во flash (команда B, как Burn в TunerStudio)'
            "
            @click="onBurn"
          >
            {{ burning ? "Burn…" : "Burn" }}
          </button>
          <button
            type="button"
            class="conn-badge"
            title="Протокол ECU — команды и ответы"
            @click="togglePanel"
          >
            ECU
          </button>
        </template>
      </div>
      <p v-if="burnError" class="burn-error" role="alert">{{ burnError }}</p>
      <p v-if="logLoadError" class="burn-error" role="alert">{{ logLoadError }}</p>
    </header>
    <TabWorkspace />
    <p v-if="projectError" class="project-error">{{ projectError }}</p>
    <ProtocolLogSheet />
    <ConfigLoadOverlay />
    <ConfigDiffModal />
  </div>
</template>

<style scoped>
.app-shell {
  width: 100%;
  max-width: var(--content-max);
  margin: 0;
  padding: var(--app-padding-y) var(--app-padding-x) calc(var(--app-padding-y) + 0.5rem);
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
}

.app-header {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.85rem;
  margin-bottom: 0.5rem;
  padding-bottom: 1rem;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.brand-mark {
  width: 4px;
  height: 2.5rem;
  border-radius: 2px;
  background: linear-gradient(
    180deg,
    var(--color-accent) 0%,
    var(--color-accent-muted) 100%
  );
  flex-shrink: 0;
}

.app-title {
  margin: 0;
  font-size: 1.5rem;
  font-weight: 600;
  letter-spacing: -0.02em;
  color: var(--color-text);
}

.app-subtitle {
  display: block;
  margin-top: 0.2rem;
  font-size: 0.85rem;
  color: var(--color-text-muted);
}

.header-actions {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-left: auto;
  flex-wrap: wrap;
}

.project-bar {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 0.2rem;
  max-width: min(100%, 28rem);
}

.project-name {
  font-size: 0.78rem;
  color: var(--color-text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 100%;
}

.project-dirty {
  color: var(--color-accent);
}

.project-btns {
  display: flex;
  flex-wrap: wrap;
  gap: 0.25rem;
  justify-content: flex-end;
}

.project-error {
  margin: 0 0.5rem;
  padding: 0.35rem 0.6rem;
  font-size: 0.82rem;
  color: var(--color-error);
  background: color-mix(in srgb, var(--color-error) 12%, transparent);
  border-radius: var(--radius-sm);
}

.offline-toggle {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  font-size: 0.78rem;
  color: var(--color-text-muted);
  cursor: pointer;
  user-select: none;
}

.offline-toggle input {
  accent-color: var(--color-accent);
}

.scan-hint {
  font-size: 0.72rem;
  color: var(--color-text-muted);
  font-style: italic;
}

.scan-hint.busy {
  color: var(--color-error);
  font-style: normal;
}

.log-btn {
  padding: 0.25rem 0.55rem;
  font-size: 0.72rem;
  font-weight: 600;
  letter-spacing: 0.04em;
  color: var(--color-text-muted);
  background: var(--color-bg-muted);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.log-btn:hover {
  color: var(--color-text);
  border-color: var(--color-border-strong);
}

.burn-btn {
  padding: 0.35rem 0.75rem;
  font-size: 0.72rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-on-accent);
  background: var(--color-accent);
  border: 1px solid var(--color-accent);
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.burn-btn:hover:not(:disabled) {
  filter: brightness(1.05);
}

.burn-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.burn-error {
  flex: 1 1 100%;
  margin: 0.35rem 0 0;
  font-size: 0.82rem;
  color: var(--color-error);
}

.conn-badge {
  padding: 0.25rem 0.55rem;
  font-size: 0.72rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-accent-hover);
  background: var(--color-bg-accent-soft);
  border: 1px solid var(--color-success-border);
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.conn-badge:hover {
  background: #fce8d8;
}
</style>
