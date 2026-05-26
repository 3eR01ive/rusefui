import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, readonly, shallowRef } from "vue";

/** Синхронно с `WorkspacePhase` в Rust. */
export type WorkspacePhase =
  | "gate"
  | "projectOnly"
  | "ecuScanning"
  | "ecuConnectedIdle"
  | "configFromProject"
  | "configLoadingFromEcu"
  | "configFromEcu";

export type ConfigSource = "none" | "projectFile" | "ecuLive";

export interface WorkspaceCapabilities {
  showMainUi: boolean;
  editProjectConfig: boolean;
  writeConfigToEcu: boolean;
  burnToFlash: boolean;
  pollOutputChannels: boolean;
  startCompositeLogger: boolean;
}

export interface WorkspaceSnapshot {
  phase: WorkspacePhase;
  projectPath: string | null;
  projectName: string;
  projectDirty: boolean;
  hasEcuConfigInProject: boolean;
  offlineMode: boolean;
  ecuConnected: boolean;
  ecuScanning: boolean;
  configSource: ConfigSource;
  configLoaded: boolean;
  configLoading: boolean;
  capabilities: WorkspaceCapabilities;
}

const defaultCapabilities: WorkspaceCapabilities = {
  showMainUi: false,
  editProjectConfig: false,
  writeConfigToEcu: false,
  burnToFlash: false,
  pollOutputChannels: false,
  startCompositeLogger: false,
};

const snapshot = shallowRef<WorkspaceSnapshot>({
  phase: "gate",
  projectPath: null,
  projectName: "Новый проект",
  projectDirty: false,
  hasEcuConfigInProject: false,
  offlineMode: false,
  ecuConnected: false,
  ecuScanning: false,
  configSource: "none",
  configLoaded: false,
  configLoading: false,
  capabilities: defaultCapabilities,
});

/** Текущий снимок FSM (для `configCanEdit` и др.). */
export { snapshot as workspaceSnapshot };

let initPromise: Promise<void> | null = null;
let unlisten: UnlistenFn | null = null;

export async function initWorkspaceState(): Promise<void> {
  if (initPromise) return initPromise;
  initPromise = (async () => {
    try {
      snapshot.value = await invoke<WorkspaceSnapshot>("workspace_get_state");
    } catch {
      /* не Tauri */
    }
    if (!unlisten) {
      unlisten = await listen<WorkspaceSnapshot>("workspace-state", (ev) => {
        snapshot.value = ev.payload;
      });
      await listen("workspace-reset", async () => {
        try {
          snapshot.value = await invoke<WorkspaceSnapshot>("workspace_get_state");
        } catch {
          /* ignore */
        }
      });
    }
  })();
  return initPromise;
}

export async function refreshWorkspaceState(): Promise<WorkspaceSnapshot> {
  try {
    snapshot.value = await invoke<WorkspaceSnapshot>("workspace_get_state");
  } catch {
    /* ignore */
  }
  return snapshot.value;
}

export function useWorkspaceState() {
  const phase = computed(() => snapshot.value.phase);
  const hasOpenProject = computed(() => snapshot.value.projectPath != null);
  const showMainUi = computed(() => snapshot.value.capabilities.showMainUi);
  const canEditProjectConfig = computed(
    () => snapshot.value.capabilities.editProjectConfig,
  );
  const canWriteConfigToEcu = computed(
    () => snapshot.value.capabilities.writeConfigToEcu,
  );
  const canBurn = computed(() => snapshot.value.capabilities.burnToFlash);

  return {
    snapshot: readonly(snapshot),
    phase,
    hasOpenProject,
    showMainUi,
    canEditProjectConfig,
    canWriteConfigToEcu,
    canBurn,
    refreshWorkspaceState,
  };
}
