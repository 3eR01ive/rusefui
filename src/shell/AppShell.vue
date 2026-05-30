<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watchEffect, useTemplateRef } from "vue";
import TabWorkspace from "./TabWorkspace.vue";
import ProtocolLogSheet from "./ProtocolLogSheet.vue";
import ConfigLoadOverlay from "./ConfigLoadOverlay.vue";
import ConfigDiffModal from "./ConfigDiffModal.vue";
import { createDataContext, provideDataContext } from "../core/data-context";
import { initOutputChannels } from "../composables/useOutputChannels";
import { initOutputTimeline } from "../composables/useOutputTimeline";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { initConfig, useConfig, patchConfigSnapshot } from "../composables/useConfig";
import { initChecklist, useChecklistFooter, useChecklistTabAlert } from "../composables/useChecklist";
import { tabAlertClasses } from "../composables/useTabAlerts";
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
import EcuIniMismatchScreen from "./EcuIniMismatchScreen.vue";
import { initIniResolution } from "../composables/useIniResolution";
import { initIniPanels } from "../composables/useIniPanels";
import ProjectMenu from "./ProjectMenu.vue";
import { useAppFooter, setFooterLed, footerToggleProtocol } from "../composables/useAppFooter";
import { useTabState } from "../composables/useTabState";
import { useKeyboardRouter } from "../composables/useKeyboardRouter";
import { saveProjectCallback, openProjectCallback, burnCallback, undoCallback, redoCallback } from "../composables/useHotkeys";
import { undoConfigChange, redoConfigChange } from "../composables/configCommands";
import UnsavedChangesDialog from "../components/UnsavedChangesDialog.vue";
import { useUnsavedChangesGuard } from "../composables/useUnsavedChangesGuard";

const dataCtx = createDataContext();
provideDataContext(dataCtx);

const appTitle = ref("rusefui");
const { offlineMode, scanning, busyPorts } = useEcuConnection(dataCtx);
const { togglePanel } = useProtocolLog();
footerToggleProtocol.value = togglePanel;
const { burn: burnConfig } = useConfig();
const {
  info: projectInfo,
  hasOpenProject,
  createNewProject,
  changeProjectIni,
  openProject,
  closeProject,
  saveProject,
  saveProjectAs,
  captureEcuConfig,
  clearTimeline,
  copyProjectWithoutTimeline,
} = useProject();
const { showMainUi, canBurn: workspaceCanBurn, snapshot: workspaceSnap } =
  useWorkspaceState();
const iniMismatchActive = computed(
  () => workspaceSnap.value.phase === "ecuIniMismatch",
);

const { activeTabId, setTab } = useTabState();
const tabWorkspaceRef = useTemplateRef<{ tabs: { id: string; title: string }[] }>("tabWorkspace");
useKeyboardRouter();
saveProjectCallback.value = () => onSaveProject();
openProjectCallback.value = () => onOpenProject();
burnCallback.value = () => { if (canBurn.value) void onBurn(); };
undoCallback.value = () => { void undoConfigChange(patchConfigSnapshot); };
redoCallback.value = () => { void redoConfigChange(patchConfigSnapshot); };

const burning = ref(false);
const burnError = ref<string | null>(null);
const projectError = ref<string | null>(null);
const projectBusy = ref(false);
const ramDirty = computed(() => workspaceSnap.value.burnPending);

const {
  dialogState: unsavedDialogState,
  onDialogPrimary: onUnsavedDialogPrimary,
  onDialogSkip: onUnsavedDialogSkip,
  onDialogCancel: onUnsavedDialogCancel,
  confirmUnsavedChanges,
} = useUnsavedChangesGuard();

const { setFooterStatus } = useAppFooter();
useChecklistFooter();
useChecklistTabAlert();
const appShellRef = ref<HTMLElement | null>(null);
const appHeaderRef = ref<HTMLElement | null>(null);
let headerResizeObserver: ResizeObserver | null = null;

function syncHeaderHeight() {
  const shell = appShellRef.value;
  const header = appHeaderRef.value;
  if (!shell || !header) return;
  const rect = header.getBoundingClientRect();
  shell.style.setProperty("--app-header-h", `${Math.ceil(rect.height)}px`);
}

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

let unlistenCloseReq: (() => void) | null = null;

function unsavedCheck(context: "quit" | "switch") {
  return {
    context,
    projectDirty: projectInfo.value.dirty,
    projectPath: projectInfo.value.path,
    burnPending: ramDirty.value,
    ecuConnected: dataCtx.connection.value.connected,
    canBurn: canBurn.value,
    saveProject,
    saveProjectAs,
    burnConfig: onBurn,
  };
}

onMounted(async () => {
  await initProject();
  await initWorkspaceState();
  await initIniResolution();
  await initIniPanels();
  await initConfig();
  await initChecklist();
  void initOutputChannels();
  void initOutputTimeline();

  unlistenCloseReq = await listen("app-close-requested", () => {
    void (async () => {
      const proceed = await confirmUnsavedChanges(unsavedCheck("quit"));
      if (proceed) void invoke("app_force_quit");
    })();
  });
  await nextTick();
  syncHeaderHeight();
  window.addEventListener("resize", syncHeaderHeight);
  if (typeof ResizeObserver !== "undefined" && appHeaderRef.value) {
    headerResizeObserver = new ResizeObserver(() => syncHeaderHeight());
    headerResizeObserver.observe(appHeaderRef.value);
  }
});

onUnmounted(() => {
  unlistenCloseReq?.();
  window.removeEventListener("resize", syncHeaderHeight);
  headerResizeObserver?.disconnect();
  headerResizeObserver = null;
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

function onChangeProjectIni(): void {
  void runProjectAction(async () => {
    if (projectInfo.value.hasEcuConfig) {
      const ok = window.confirm(
        "Смена INI удалит сохранённый снимок config в проекте (ecuConfig). Продолжить?",
      );
      if (!ok) return;
    }
    try {
      await changeProjectIni(false);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      if (msg.includes("signature не совпадает")) {
        const force = window.confirm(
          `${msg}\n\nПрименить INI принудительно (force)?`,
        );
        if (force) await changeProjectIni(true);
      } else {
        throw e;
      }
    }
  });
}

function onNewProject(): void {
  void runProjectAction(async () => {
    const proceed = await confirmUnsavedChanges(unsavedCheck("switch"));
    if (!proceed) return;
    await createNewProject();
  });
}

function onOpenProject(): void {
  void runProjectAction(async () => {
    const proceed = await confirmUnsavedChanges(unsavedCheck("switch"));
    if (!proceed) return;
    await openProject();
  });
}

function onCloseProject(): void {
  void runProjectAction(async () => {
    const proceed = await confirmUnsavedChanges(unsavedCheck("switch"));
    if (!proceed) return;
    await closeProject();
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

function onCopyProjectWithoutTimeline(): void {
  void runProjectAction(async () => {
    const proceed = await confirmUnsavedChanges(unsavedCheck("switch"));
    if (!proceed) return;
    await copyProjectWithoutTimeline();
  });
}

function onClearTimeline(): void {
  void runProjectAction(async () => {
    await clearTimeline();
  });
}

function workspacePhaseLabel(phase: WorkspacePhase): string {
  switch (phase) {
    case "gate":
      return "";
    case "projectOnly":
      return "проект";
    case "ecuScanning":
      return "поиск ECU";
    case "ecuIniMismatch":
      return "INI mismatch";
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

function afterPaint(): Promise<void> {
  return new Promise((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
}

async function onBurn() {
  if (!canBurn.value) return;
  burning.value = true;
  burnError.value = null;
  await nextTick();
  await afterPaint();
  try {
    await burnConfig();
    burning.value = false;
  } catch (e) {
    burnError.value = e instanceof Error ? e.message : String(e);
    burning.value = false;
  }
}
</script>

<template>
  <ProjectGate v-if="!showMainUi && !iniMismatchActive" />
  <EcuIniMismatchScreen v-if="iniMismatchActive" />

  <div v-if="showMainUi" ref="appShellRef" class="app-shell">
    <header ref="appHeaderRef" class="app-header">
      <ProjectMenu
        :project-name="projectInfo.name"
        :project-path="projectInfo.path"
        :project-dirty="projectInfo.dirty"
        :project-busy="projectBusy"
        :can-capture-config="dataCtx.connection.value.connected"
        :has-open-project="hasOpenProject"
        :timeline-clip-count="projectInfo.timelineClipCount"
        :ini-signature="projectInfo.iniSignature"
        @new-project="onNewProject"
        @open-project="onOpenProject"
        @close-project="onCloseProject"
        @save-project="onSaveProject"
        @save-project-as="onSaveProjectAs"
        @change-ini="onChangeProjectIni"
        @capture-config="onCaptureConfigToProject"
        @copy-project-without-timeline="onCopyProjectWithoutTimeline"
        @clear-timeline="onClearTimeline"
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
        <div
          v-for="tab in (tabWorkspaceRef?.tabs ?? [])"
          :key="tab.id"
          class="header-tab-slot"
          :class="tabAlertClasses(tab.id)"
        >
          <button
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
          <!-- Timeline: horizontal axis + markers -->
          <svg v-else-if="tab.id === 'timeline'" class="tab-icon" viewBox="0 0 22 20" fill="none" aria-hidden="true">
            <line x1="2" y1="14" x2="20" y2="14" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" opacity=".35"/>
            <circle cx="6" cy="14" r="2.2" fill="currentColor"/>
            <circle cx="11" cy="14" r="2.2" fill="currentColor" opacity=".65"/>
            <circle cx="16.5" cy="14" r="2.2" fill="currentColor" opacity=".45"/>
            <path d="M4 4.5h14M4 8h10" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" opacity=".25"/>
          </svg>
          <!-- Knock: waveform bars -->
          <svg v-else-if="tab.id === 'knock'" class="tab-icon" viewBox="0 0 22 20" fill="none" aria-hidden="true">
            <rect x="2" y="2" width="18" height="16" rx="2.5" fill="currentColor" opacity=".12" stroke="currentColor" stroke-width="1.3"/>
            <path d="M5 14V10M8.5 14V6M12 14V8M15.5 14V4M19 14V11" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
          </svg>
          <!-- Control: spark plug -->
          <svg v-else-if="tab.id === 'simulation'" class="tab-icon" viewBox="0 0 22 22" fill="none" aria-hidden="true">
            <rect x="7.5" y="1.5" width="7" height="5" rx="1.5" fill="currentColor" opacity=".45" stroke="currentColor" stroke-width="1.3"/>
            <path d="M7.5 6.5h7v1.5a3.5 3.5 0 0 1-7 0V6.5Z" fill="currentColor" opacity=".2" stroke="currentColor" stroke-width="1.3"/>
            <path d="M11 10v4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
            <path d="M8.5 14h5l-1.8 3h1.5L11 21l.6-4H8.5l2-3Z" fill="currentColor"/>
          </svg>
          <!-- CHKLST: clipboard + checks -->
          <svg v-else-if="tab.id === 'checklist'" class="tab-icon" viewBox="0 0 22 20" fill="none" aria-hidden="true">
            <rect x="4" y="1.5" width="14" height="17" rx="2.5" fill="currentColor" opacity=".12" stroke="currentColor" stroke-width="1.4"/>
            <path d="M7.5 6.5h7M7.5 10h7M7.5 13.5h5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" opacity=".35"/>
            <path d="M5.8 6.3l1 1 1.8-2M5.8 9.8l1 1 1.8-2M5.8 13.3l1 1 1.8-2" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>
          </svg>
          <!-- Run: tachometer -->
          <svg v-else-if="tab.id === 'run'" class="tab-icon" viewBox="0 0 22 22" fill="none" aria-hidden="true">
            <path d="M11 3.5a8.5 8.5 0 1 1 0 17 8.5 8.5 0 0 1 0-17Z" fill="currentColor" opacity=".12" stroke="currentColor" stroke-width="1.4"/>
            <path d="M11 11V7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
            <path d="M11 11l3.5 2" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
            <circle cx="11" cy="11" r="1.3" fill="currentColor"/>
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
        </div>
      </nav>

      <div class="header-actions">
        <div
          v-if="dataCtx.connection.value.connected"
          class="burn-wrap"
          :class="{ 'burn-wrap--dirty': ramDirty && canBurn }"
        >
          <button
            type="button"
            class="burn-btn"
            :class="{ 'burn-btn--dirty': ramDirty && canBurn, 'burn-btn--burning': burning }"
            :disabled="!canBurn"
            :title="burnError ?? 'Записать конфигурацию во flash (Ctrl+Enter, команда B)'"
            @click="onBurn"
          >
            <!-- Flame icon — classic emoji-style with fill-rule evenodd cutout -->
            <svg class="burn-icon" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
              <defs>
                <linearGradient id="fg" x1="12" y1="3" x2="12" y2="21" gradientUnits="userSpaceOnUse">
                  <stop offset="0%"   stop-color="#fef08a"/>
                  <stop offset="40%"  stop-color="#fb923c"/>
                  <stop offset="100%" stop-color="#dc2626"/>
                </linearGradient>
              </defs>
              <path fill-rule="evenodd" clip-rule="evenodd"
                d="M12 3
                   C11 5.5 9 8 8.5 10.5
                   C8.2 9.8 8 9 8 8
                   C6.5 9.5 6 11.5 6 13.5
                   C6 17.09 8.69 20 12 20
                   C15.31 20 18 17.09 18 13.5
                   C18 11 16.5 8.5 15 7
                   C15.1 8.2 14.7 9.3 14 10
                   C13.9 7.5 13 5 12 3Z
                   M12 12
                   C11.3 12.8 11 13.8 11.2 14.8
                   C11.5 14.3 12 13.9 12.5 13.7
                   C12.6 14.2 12.6 14.7 12.3 15.2
                   C13.1 14.7 13.5 13.7 13.3 12.7
                   C13.1 12.2 12.6 11.8 12 12Z"
                fill="url(#fg)"/>
            </svg>
            <span class="burn-label">{{ burning ? "Burn…" : "Burn" }}</span>
          </button>
        </div>
      </div>
    </header>

    <!-- Блюр + спиннер во время записи во flash -->
    <div v-if="burning" class="burn-overlay" aria-live="assertive" aria-label="Запись во flash">
      <div class="burn-overlay-card">
        <div class="burn-spinner-wrap" aria-hidden="true">
          <svg class="burn-spinner-ring" viewBox="0 0 100 100" fill="none" xmlns="http://www.w3.org/2000/svg">
            <defs>
              <linearGradient id="spin-grad" x1="0" y1="50" x2="100" y2="50" gradientUnits="userSpaceOnUse">
                <stop offset="0%" stop-color="#fbbf24" stop-opacity="0"/>
                <stop offset="55%" stop-color="#fb923c"/>
                <stop offset="100%" stop-color="#ef4444"/>
              </linearGradient>
            </defs>
            <circle cx="50" cy="50" r="42" stroke="rgba(255,255,255,0.12)" stroke-width="7" fill="none"/>
            <circle cx="50" cy="50" r="42"
              stroke="url(#spin-grad)" stroke-width="7" fill="none"
              stroke-dasharray="180 84" stroke-linecap="round"/>
          </svg>
          <div class="burn-flame-pulse">
            <svg class="burn-flame-center" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
              <defs>
                <linearGradient id="fc" x1="12" y1="3" x2="12" y2="21" gradientUnits="userSpaceOnUse">
                  <stop offset="0%" stop-color="#fef08a"/>
                  <stop offset="40%" stop-color="#fb923c"/>
                  <stop offset="100%" stop-color="#dc2626"/>
                </linearGradient>
              </defs>
              <path fill-rule="evenodd" clip-rule="evenodd"
                d="M12 3C11 5.5 9 8 8.5 10.5C8.2 9.8 8 9 8 8C6.5 9.5 6 11.5 6 13.5C6 17.09 8.69 20 12 20C15.31 20 18 17.09 18 13.5C18 11 16.5 8.5 15 7C15.1 8.2 14.7 9.3 14 10C13.9 7.5 13 5 12 3ZM12 12C11.3 12.8 11 13.8 11.2 14.8C11.5 14.3 12 13.9 12.5 13.7C12.6 14.2 12.6 14.7 12.3 15.2C13.1 14.7 13.5 13.7 13.3 12.7C13.1 12.2 12.6 11.8 12 12Z"
                fill="url(#fc)"/>
            </svg>
          </div>
        </div>
        <p class="burn-overlay-label">Запись во flash…</p>
      </div>
    </div>

    <!-- Диалог: несохранённые изменения при закрытии -->
    <UnsavedChangesDialog
      :state="unsavedDialogState"
      @primary="onUnsavedDialogPrimary"
      @skip="onUnsavedDialogSkip"
      @cancel="onUnsavedDialogCancel"
    />

    <main class="app-main">
      <TabWorkspace ref="tabWorkspace" />
    </main>
    <ProtocolLogSheet />
    <ConfigLoadOverlay />
    <ConfigDiffModal />
    <EcuConnectionModal />
  </div>
</template>

<style scoped>
.app-shell {
  width: 100%;
  max-width: none;
  margin: 0;
  padding: 0;
  height: 100vh;
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  overflow: hidden;
  padding-top: calc(var(--app-header-h, 5.5rem) + 0.75rem);
}

.app-header {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  z-index: 400;
  display: flex;
  flex-wrap: nowrap;
  align-items: center;
  gap: 0;
  padding: var(--app-padding-y) var(--app-padding-x) 1rem;
  background: var(--color-bg);
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.app-main {
  position: relative;
  height: calc(100vh - var(--app-header-h, 5.5rem) - var(--footer-height) - 0.75rem);
  overflow: hidden;
  padding: 0 var(--app-padding-x);
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

.header-tab-slot {
  position: relative;
  display: inline-flex;
  border-radius: calc(var(--radius-md) + 2px);
  padding: 2px;
  flex-shrink: 0;
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

/* CSS custom property для анимации вращения границы */
@property --burn-angle {
  syntax: "<angle>";
  inherits: false;
  initial-value: 0deg;
}

/* Обёртка — создаёт вращающийся conic-gradient бордер */
.burn-wrap {
  position: relative;
  border-radius: calc(var(--radius-sm) + 3px);
  padding: 2px; /* толщина «бордера» */
  background: transparent;
  display: inline-flex;
}

/* Вращающийся бордер — виден при ховере или при dirty */
.burn-wrap::before {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: inherit;
  background: conic-gradient(
    from var(--burn-angle),
    transparent 0deg,
    #fbbf24 40deg,
    #f97316 90deg,
    #ea580c 120deg,
    transparent 160deg
  );
  opacity: 0;
  transition: opacity 0.2s;
}

.burn-wrap:hover::before {
  opacity: 1;
  animation: burn-spin 1.5s linear infinite;
}

.burn-wrap--dirty::before {
  opacity: 1;
  animation: burn-spin 1.2s linear infinite;
}

@keyframes burn-spin {
  to { --burn-angle: 360deg; }
}

/* Сама кнопка — по умолчанию только оранжевая обводка, без заливки */
.burn-btn {
  position: relative;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0.5rem 1.1rem;
  font-size: 0.82rem;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: #fb923c;
  background: transparent;
  border: 2px solid #ea580c;
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: background 0.18s, border-color 0.18s, color 0.18s;
  z-index: 0;
  white-space: nowrap;
}

/* Есть несохранённое в RAM — чуть заметнее, но без сплошной заливки */
.burn-btn--dirty {
  background: rgba(249, 115, 22, 0.1);
  border-color: #f97316;
  color: #fdba74;
}

.burn-btn--burning {
  background: transparent;
  border-color: #f97316;
  color: #fb923c;
  opacity: 0.65;
  cursor: wait;
}

.burn-btn:disabled {
  opacity: 0.35;
  border-color: var(--color-border-strong);
  color: var(--color-gray);
  cursor: not-allowed;
}

.burn-icon {
  width: 26px;
  height: 26px;
  flex-shrink: 0;
  display: block;
}

.burn-label {
  line-height: 1;
}

/* Блюр поверх интерфейса во время Burn */
.burn-overlay {
  position: fixed;
  inset: 0;
  z-index: 9000;
  background: rgba(0, 0, 0, 0.5);
  backdrop-filter: blur(6px);
  display: flex;
  align-items: center;
  justify-content: center;
}

.burn-overlay-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 24px;
}

/* Спиннер: кольцо + иконка строго по центру */
.burn-spinner-wrap {
  --burn-spinner-size: 140px;
  position: relative;
  width: var(--burn-spinner-size);
  height: var(--burn-spinner-size);
  display: grid;
  place-items: center;
}

.burn-spinner-ring {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  animation: burn-ring-spin 1s linear infinite;
}

@keyframes burn-ring-spin {
  to { transform: rotate(360deg); }
}

.burn-flame-pulse {
  position: relative;
  z-index: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  animation: flame-flicker 0.85s ease-in-out infinite alternate;
}

.burn-flame-center {
  width: 56px;
  height: 56px;
  display: block;
}

@keyframes flame-flicker {
  from { transform: scale(1); }
  to { transform: scale(1.08); }
}

.burn-overlay-label {
  color: #fff;
  font-size: 1.15rem;
  font-weight: 600;
  letter-spacing: 0.04em;
  margin: 0;
  text-shadow: 0 1px 8px rgba(0, 0, 0, 0.5);
}


</style>
