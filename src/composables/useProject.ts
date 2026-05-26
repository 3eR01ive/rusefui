import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, readonly, ref, shallowRef } from "vue";

/** Ключ в реестре `ui_persist` (Rust). */
export const PERSIST_KEY_OUTPUT_CHART = "output-chart";
export const PERSIST_KEY_COMPOSITE_CHART = "composite-chart";

export interface CompositeChartUiSettings {
  autostart: boolean;
  alignTdc?: boolean;
  autoStopSec?: number;
}

export interface ProjectInfo {
  path: string | null;
  name: string;
  dirty: boolean;
  logCount: number;
  hasEcuConfig: boolean;
}

export interface LogGraphGroupJson {
  id: string;
  fieldNames: string[];
}

export interface LogRangeInputJson {
  min: string;
  max: string;
}

export interface LogUiSettings {
  windowSeconds: number;
  chartHeight: number;
  zoomStepPct: number;
  settingsExpanded: boolean;
  graphGroups: LogGraphGroupJson[];
  activeGraphId: string;
  rangeInputs: Record<string, LogRangeInputJson>;
}

export interface ProjectLogRef {
  path: string;
  label?: string | null;
  addedAtMs: number;
  kind: string;
}

/** Сигнал для компонентов: перечитать UI из проекта. */
export const projectUiEpoch = ref(0);

/** Сброс config/timeline/кэша графика после смены проекта. */
export const workspaceResetEpoch = ref(0);

const info = shallowRef<ProjectInfo>({
  path: null,
  name: "Новый проект",
  dirty: false,
  logCount: 0,
  hasEcuConfig: false,
});

let initPromise: Promise<void> | null = null;
let unlisten: UnlistenFn | null = null;

async function refreshInfo(): Promise<void> {
  info.value = await invoke<ProjectInfo>("project_get_info");
}

export async function initProject(): Promise<void> {
  if (initPromise) return initPromise;
  initPromise = (async () => {
    await refreshInfo();
    if (!unlisten) {
      unlisten = await listen<ProjectInfo>("project-changed", (ev) => {
        info.value = ev.payload;
        projectUiEpoch.value += 1;
      });
      await listen("workspace-reset", () => {
        workspaceResetEpoch.value += 1;
      });
    }
  })();
  return initPromise;
}

export function useProject() {
  const hasPath = computed(() => Boolean(info.value.path));

  /**
   * Новый проект: при необходимости сохранить текущий, выбрать файл, создать и сбросить UI.
   * @returns false если пользователь отменил диалог
   */
  async function createNewProject(): Promise<boolean> {
    if (info.value.dirty) {
      const save = window.confirm(
        "Сохранить текущий проект перед созданием нового?",
      );
      if (save) {
        if (info.value.path) {
          await invoke("project_save");
        } else {
          const saved = await saveProjectAs();
          if (!saved) return false;
        }
      }
    }

    const path = await invoke<string | null>("pick_project_save_path", {
      defaultName: "Новый проект",
    });
    if (!path) return false;

    await invoke("project_create_new", { path, name: null });
    return true;
  }

  async function openProject(): Promise<boolean> {
    const path = await invoke<string | null>("pick_project_open_path");
    if (!path) return false;
    await invoke("project_load", { path });
    return true;
  }

  async function saveProject(): Promise<string | null> {
    try {
      return await invoke<string>("project_save");
    } catch {
      return saveProjectAs();
    }
  }

  async function saveProjectAs(): Promise<string | null> {
    const path = await invoke<string | null>("pick_project_save_path", {
      defaultName: info.value.name,
    });
    if (!path) return null;
    await invoke("project_save_path", { path });
    return path;
  }

  async function captureEcuConfig(): Promise<void> {
    await invoke("project_capture_ecu_config");
  }

  async function addLog(
    path: string,
    label?: string,
    kind?: "output_csv" | "composite_csv",
  ): Promise<void> {
    await invoke("project_add_log", {
      path,
      label: label ?? null,
      kind: kind ?? null,
    });
  }

  async function removeLog(path: string): Promise<void> {
    await invoke("project_remove_log", { path });
  }

  async function listLogs(): Promise<ProjectLogRef[]> {
    return invoke<ProjectLogRef[]>("project_list_logs");
  }

  async function getProjectUi<T>(key: string): Promise<T> {
    return invoke<T>("project_ui_get", { key });
  }

  async function setProjectUi(key: string, value: unknown): Promise<void> {
    await invoke("project_ui_set", { key, value });
  }

  async function listPersistKeys(): Promise<string[]> {
    return invoke<string[]>("project_ui_persist_keys");
  }

  return {
    info: readonly(info),
    hasPath,
    projectUiEpoch: readonly(projectUiEpoch),
    refreshInfo,
    createNewProject,
    openProject,
    saveProject,
    saveProjectAs,
    captureEcuConfig,
    addLog,
    removeLog,
    listLogs,
    getProjectUi,
    setProjectUi,
    listPersistKeys,
  };
}
