import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, readonly, ref, shallowRef } from "vue";

/** Ключ в реестре `ui_persist` (Rust). */
export const PERSIST_KEY_OUTPUT_CHART = "output-chart";
export const PERSIST_KEY_COMPOSITE_CHART = "composite-chart";
export const PERSIST_KEY_DYNO = "dyno";

export type CrankEdgeMode = "both" | "rise" | "fall";

export interface CompositeChartUiSettings {
  alignTdc?: boolean;
  captureDurationMs?: number;
  crankEdgeMode?: CrankEdgeMode;
  /** @deprecated — kept for reading old project files */
  autostart?: boolean;
  /** @deprecated — kept for reading old project files */
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

export interface DynoUiSettings {
  ignoreTpsMin: boolean;
  minRpm: number;
  smoothStrength: number;
  chartHeight: number;
}

export interface ProjectLogRef {
  path: string;
  label?: string | null;
  addedAtMs: number;
  kind: string;
}

export interface RecentProjectEntry {
  path: string;
  label: string;
  exists: boolean;
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

/** `initProject()` завершён — можно показывать экран выбора проекта. */
export const projectInitialized = ref(false);

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
    projectInitialized.value = true;
  })();
  return initPromise;
}

export function useProject() {
  /** Проект привязан к файлу на диске — можно работать с ECU и настройками. */
  const hasOpenProject = computed(() => Boolean(info.value.path));
  const hasPath = hasOpenProject;

  /**
   * Новый проект: выбрать файл, создать и сбросить UI.
   * Проверку несохранённого проекта / burn выполняет вызывающий код.
   * @returns false если пользователь отменил диалог файла
   */
  async function createNewProject(): Promise<boolean> {
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

  async function openProjectAtPath(path: string): Promise<boolean> {
    await invoke("project_load", { path });
    return true;
  }

  /** Закрыть проект и вернуть экран Gate. */
  async function closeProject(): Promise<void> {
    await invoke("project_close");
  }

  async function listRecentProjects(): Promise<RecentProjectEntry[]> {
    return invoke<RecentProjectEntry[]>("recent_projects_list");
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
    hasOpenProject,
    hasPath,
    projectUiEpoch: readonly(projectUiEpoch),
    refreshInfo,
    createNewProject,
    openProject,
    openProjectAtPath,
    listRecentProjects,
    closeProject,
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
