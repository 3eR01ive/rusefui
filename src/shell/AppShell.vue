<script setup lang="ts">
import { computed, onMounted, ref, watchEffect, useTemplateRef } from "vue";
import TabWorkspace from "./TabWorkspace.vue";
import ProtocolLogSheet from "./ProtocolLogSheet.vue";
import ConfigLoadOverlay from "./ConfigLoadOverlay.vue";
import ConfigDiffModal from "./ConfigDiffModal.vue";
import { createDataContext, provideDataContext } from "../core/data-context";
import { initOutputChannels } from "../composables/useOutputChannels";
import { initOutputTimeline } from "../composables/useOutputTimeline";
import { initConfig, useConfig } from "../composables/useConfig";
import { useProtocolLog, useProtocolLogLifecycle } from "../composables/useProtocolLog";
import { useEcuConnection } from "../composables/useEcuConnection";
import { initProject, useProject } from "../composables/useProject";
import {
  initWorkspaceState,
  useWorkspaceState,
  type WorkspacePhase,
} from "../composables/useWorkspaceState";
import ProjectGate from "./ProjectGate.vue";
import EcuConnectionModal from "./EcuConnectionModal.vue";
import ProjectMenu from "./ProjectMenu.vue";
import { useAppFooter, setFooterLed, footerToggleProtocol } from "../composables/useAppFooter";
import { useTabState } from "../composables/useTabState";
import { useGlobalHotkeys, saveProjectCallback, openProjectCallback } from "../composables/useHotkeys";

const dataCtx = createDataContext();
provideDataContext(dataCtx);

const appTitle = ref("rusefui");
const { offlineMode, scanning, busyPorts } = useEcuConnection(dataCtx);
const { togglePanel } = useProtocolLog();
footerToggleProtocol.value = togglePanel;
const { burn: burnConfig } = useConfig();
const {
  info: projectInfo,
  createNewProject,
  openProject,
  saveProject,
  saveProjectAs,
  captureEcuConfig,
} = useProject();
const { showMainUi, canBurn: workspaceCanBurn, snapshot: workspaceSnap } =
  useWorkspaceState();

const { activeTabId, setTab } = useTabState();
const tabWorkspaceRef = useTemplateRef<{ tabs: { id: string; title: string }[] }>("tabWorkspace");
useGlobalHotkeys();
saveProjectCallback.value = () => onSaveProject();
openProjectCallback.value = () => onOpenProject();

const burning = ref(false);
const burnError = ref<string | null>(null);
const projectError = ref<string | null>(null);
const projectBusy = ref(false);

const { setFooterStatus } = useAppFooter();

watchEffect(() => {
  if (!showMainUi.value) {
    setFooterStatus("app:project", null);
    setFooterStatus("app:ecu", null);
    setFooterStatus("app:burn", null);
    setFooterStatus("app:log", null);
    setFooterStatus("app:project-error", null);
    setFooterStatus("app:burning", null);
    setFooterStatus("app:loading-log", null);
    setFooterLed("off");
    return;
  }

  const projectParts: string[] = [projectInfo.value.name];
  if (projectInfo.value.dirty && projectInfo.value.path) projectParts.push("несохранён");
  const phase = workspacePhaseLabel(workspaceSnap.value.phase);
  if (phase) projectParts.push(phase);
  setFooterStatus("app:project", projectParts.join(" · "), { priority: 10 });

  if (scanning.value && !offlineMode.value) {
    setFooterLed("scanning", "Поиск ECU…");
    setFooterStatus("app:ecu", null);
  } else if (busyPorts.value.length && !offlineMode.value) {
    setFooterLed("error", `Порт занят: ${busyPorts.value.join(", ")}`);
    setFooterStatus("app:ecu", `Порт занят (${busyPorts.value.join(", ")}) — отключите TunerStudio`, {
      error: true,
      priority: 30,
    });
  } else if (dataCtx.connection.value.connected) {
    setFooterLed("connected", "ECU подключена");
    setFooterStatus("app:ecu", null);
  } else if (offlineMode.value) {
    setFooterLed("off", "Offline");
    setFooterStatus("app:ecu", null);
  } else {
    setFooterLed("off", "нет ECU");
    setFooterStatus("app:ecu", null);
  }

  setFooterStatus("app:burn", burnError.value, { error: true, priority: 100 });
  setFooterStatus("app:project-error", projectError.value, { error: true, priority: 100 });
  setFooterStatus("app:burning", burning.value ? "Burn…" : null, { priority: 15 });
});


const canBurn = computed(
  () => workspaceCanBurn.value && !burning.value,
);

useProtocolLogLifecycle();

onMounted(async () => {
  await initProject();
  await initWorkspaceState();
  await initConfig();
  void initOutputChannels();
  void initOutputTimeline();
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

function workspacePhaseLabel(phase: WorkspacePhase): string {
  switch (phase) {
    case "gate":
      return "";
    case "projectOnly":
      return "проект";
    case "ecuScanning":
      return "поиск ECU";
    case "ecuConnectedIdle":
      return "ECU";
    case "configFromProject":
      return "config⊂проект";
    case "configLoadingFromEcu":
      return "загрузка…";
    case "configFromEcu":
      return "config⊂ECU";
    default:
      return "";
  }
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
  <ProjectGate v-if="!showMainUi" />

  <div v-if="showMainUi" class="app-shell">
    <header class="app-header">
      <ProjectMenu
        :project-name="projectInfo.name"
        :project-path="projectInfo.path"
        :project-dirty="projectInfo.dirty"
        :project-busy="projectBusy"
        :can-capture-config="dataCtx.connection.value.connected"
        @new-project="onNewProject"
        @open-project="onOpenProject"
        @save-project="onSaveProject"
        @save-project-as="onSaveProjectAs"
        @capture-config="onCaptureConfigToProject"
      />
      <div class="brand-block">
        <div class="brand-mark" aria-hidden="true" />
        <div>
          <h1 class="app-title">{{ appTitle }}</h1>
          <span class="app-subtitle">rusEFI · декларативный UI</span>
        </div>
      </div>
      <div class="header-tabs-sep" aria-hidden="true" />
      <nav class="header-tabs" role="tablist" aria-label="Разделы">
        <button
          v-for="tab in (tabWorkspaceRef?.tabs ?? [])"
          :key="tab.id"
          type="button"
          role="tab"
          class="header-tab-btn"
          :class="{ active: tab.id === activeTabId }"
          :aria-selected="tab.id === activeTabId"
          :title="tab.title"
          @click="setTab(tab.id)"
        >
          <!-- Monitor: screen + ECG waveform -->
          <svg v-if="tab.id === 'monitor'" class="tab-icon" viewBox="0 0 22 20" fill="none" aria-hidden="true">
            <rect x="1.5" y="1.5" width="19" height="13" rx="2.5" fill="currentColor" opacity=".12" stroke="currentColor" stroke-width="1.5"/>
            <path d="M4 9.5L6.5 6.5l2.2 3.8 2.8-6.3L14 9.5l2-2.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
            <path d="M8.5 14.5v2.5M13.5 14.5v2.5M6 17h10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
          </svg>
          <!-- Control: spark plug -->
          <svg v-else-if="tab.id === 'simulation'" class="tab-icon" viewBox="0 0 22 22" fill="none" aria-hidden="true">
            <rect x="7.5" y="1.5" width="7" height="5" rx="1.5" fill="currentColor" opacity=".45" stroke="currentColor" stroke-width="1.3"/>
            <path d="M7.5 6.5h7v1.5a3.5 3.5 0 0 1-7 0V6.5Z" fill="currentColor" opacity=".2" stroke="currentColor" stroke-width="1.3"/>
            <path d="M11 10v4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
            <path d="M8.5 14h5l-1.8 3h1.5L11 21l.6-4H8.5l2-3Z" fill="currentColor"/>
          </svg>
          <!-- Config: horizontal sliders -->
          <svg v-else-if="tab.id === 'ini-preview'" class="tab-icon" viewBox="0 0 22 20" fill="none" aria-hidden="true">
            <line x1="2" y1="4.5" x2="20" y2="4.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" opacity=".35"/>
            <line x1="2" y1="10" x2="20" y2="10" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" opacity=".35"/>
            <line x1="2" y1="15.5" x2="20" y2="15.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" opacity=".35"/>
            <circle cx="7" cy="4.5" r="2.8" fill="currentColor" stroke="var(--color-bg-elevated)" stroke-width="1.2"/>
            <circle cx="14" cy="10" r="2.8" fill="currentColor" stroke="var(--color-bg-elevated)" stroke-width="1.2"/>
            <circle cx="9" cy="15.5" r="2.8" fill="currentColor" stroke="var(--color-bg-elevated)" stroke-width="1.2"/>
          </svg>
          <!-- fallback -->
          <svg v-else class="tab-icon" viewBox="0 0 22 22" fill="none" aria-hidden="true">
            <rect x="3" y="3" width="16" height="16" rx="2.5" fill="currentColor" opacity=".25" stroke="currentColor" stroke-width="1.3"/>
          </svg>
          <span class="header-tab-label">{{ tab.title }}</span>
        </button>
      </nav>

      <div class="header-actions">
        <button
          v-if="dataCtx.connection.value.connected"
          type="button"
          class="burn-btn"
          :disabled="!canBurn"
          :title="burnError ?? 'Записать конфигурацию во flash (команда B, как Burn в TunerStudio)'"
          @click="onBurn"
        >
          {{ burning ? "Burn…" : "Burn" }}
        </button>
      </div>
    </header>
    <TabWorkspace ref="tabWorkspace" />
    <ProtocolLogSheet />
    <ConfigLoadOverlay />
    <ConfigDiffModal />
    <EcuConnectionModal />
  </div>
</template>

<style scoped>
.app-shell {
  width: 100%;
  max-width: var(--content-max);
  margin: 0;
  padding: var(--app-padding-y) var(--app-padding-x) 0.5rem;
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
}

.app-header {
  display: flex;
  flex-wrap: nowrap;
  align-items: center;
  gap: 0;
  margin-bottom: 0.5rem;
  padding-bottom: 1rem;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

/* gap after ProjectMenu via its container */
.app-header > :first-child {
  margin-right: 0.75rem;
}

.brand-block {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  flex-shrink: 0;
}

/* ---- vertical separator between brand and tabs ---- */
.header-tabs-sep {
  width: 1px;
  height: 2rem;
  background: var(--color-border);
  flex-shrink: 0;
  margin: 0 0.75rem;
}

/* ---- tab buttons in header ---- */
.header-tabs {
  display: flex;
  align-items: center;
  gap: 0.3rem;
}

.header-tab-btn {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.18rem;
  width: 4rem;
  height: 3.2rem;
  background: none;
  border: 1px solid transparent;
  border-radius: var(--radius-md);
  cursor: pointer;
  color: var(--color-text-subtle);
  transition: background 0.15s, border-color 0.15s, color 0.15s;
  padding: 0 0.25rem;
  flex-shrink: 0;
}

.header-tab-btn:hover {
  background: var(--color-bg-muted);
  color: var(--color-text-muted);
  border-color: var(--color-border);
}

.header-tab-btn.active {
  background: var(--color-bg-accent-soft);
  border-color: var(--color-success-border);
  color: var(--color-accent-hover);
}

.tab-icon {
  width: 1.4rem;
  height: 1.4rem;
}

.header-tab-label {
  font-size: 0.58rem;
  font-weight: 600;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  line-height: 1;
  white-space: nowrap;
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


</style>
