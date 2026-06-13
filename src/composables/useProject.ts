import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, readonly, ref, shallowRef } from "vue";

/** Ключ в реестре `ui_persist` (Rust). */
export const PERSIST_KEY_OUTPUT_CHART = "output-chart";
export const PERSIST_KEY_COMPOSITE_CHART = "composite-chart";
export const PERSIST_KEY_DYNO = "dyno";
export const PERSIST_KEY_KNOCK = "knock";
export const PERSIST_KEY_SIMULATION = "simulation";

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
  /** Directory path of the open git project; null when no project is open. */
  path: string | null;
  name: string;
  dirty: boolean;
  logCount: number;
  timelineClipCount: number;
  hasEcuConfig: boolean;
}

export interface ProjectScript {
  id: string;
  name: string;
  createdAtMs: number;
}

export interface CommitSummary {
  id: string;
  shortId: string;
  message: string;
  timestampMs: number;
}

export interface ProjectListEntry {
  dir: string;
  name: string;
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
  followLive?: boolean;
  /** Фактический zoom; 0 или отсутствует — `windowSeconds`. */
  spanSec?: number;
}

export interface DynoUiSettings {
  ignoreTpsMin: boolean;
  minRpm: number;
  smoothStrength: number;
  chartHeight: number;
  settingsOpen: boolean;
  chartRpmMin: number;
  chartRpmMax: number;
  chartNmMin: number;
  chartNmMax: number;
  chartHpMin: number;
  chartHpMax: number;
}

export interface KnockUiSettings {
  ignoreTpsMin: boolean;
  minRpm: number;
  cutoffRpm: number;
  thresholdGapDb: number;
  tempTargetLambda: number;
  tempIgnitionRetardDeg: number;
  momentumSafeRpmMin: number;
  momentumSafeRpmMax: number;
  momentumMinLoad: number;
  momentumAdvanceAddDeg: number;
  momentumDurationMs: number;
  spectrogramWindowMs: number;
  spectrogramAutocontrast: boolean;
  spectrogramGainPercent: number;
  chartHeight: number;
  settingsOpen: boolean;
}

export type SimulationRampCurve = "linear" | "smooth";

export interface SimulationUiSettings {
  targetRpm: number;
  idleRpm: number;
  peakRpm: number;
  rampUpSec: number;
  rampDownSec: number;
  rampCurve: SimulationRampCurve;
  settingsOpen: boolean;
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
  timelineClipCount: 0,
  hasEcuConfig: false,
});

/** `initProject()` завершён — можно показывать экран выбора проекта. */
export const projectInitialized = ref(false);

let initPromise: Promise<void> | null = null;
let unlisten: UnlistenFn | null = null;

const uiFlushHooks = new Set<() => void | Promise<void>>();

/** Сбросить debounced UI в `ProjectStore` перед `project_save`. */
export function registerProjectUiFlushHook(
  hook: () => void | Promise<void>,
): () => void {
  uiFlushHooks.add(hook);
  return () => uiFlushHooks.delete(hook);
}

export async function flushProjectUiToStore(): Promise<void> {
  const hooks = [...uiFlushHooks];
  await Promise.all(hooks.map((h) => Promise.resolve(h())));
}

function isProjectSaveOnlyChange(prev: ProjectInfo, next: ProjectInfo): boolean {
  return (
    prev.path === next.path &&
    prev.name === next.name &&
    prev.logCount === next.logCount &&
    prev.timelineClipCount === next.timelineClipCount &&
    prev.hasEcuConfig === next.hasEcuConfig &&
    next.dirty === false
  );
}

async function refreshInfo(): Promise<void> {
  info.value = await invoke<ProjectInfo>("project_get_info");
}

export async function initProject(): Promise<void> {
  if (initPromise) return initPromise;
  initPromise = (async () => {
    await refreshInfo();
    if (!unlisten) {
      unlisten = await listen<ProjectInfo>("project-changed", (ev) => {
        const prev = info.value;
        info.value = ev.payload;
        if (!isProjectSaveOnlyChange(prev, ev.payload)) {
          projectUiEpoch.value += 1;
        }
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
  /** Проект привязан к директории на диске — можно работать с ECU и настройками. */
  const hasOpenProject = computed(() => Boolean(info.value.path));
  const hasPath = hasOpenProject;

  /**
   * Новый проект: запросить имя, создать в ~/.rusefui/projects/.
   * @returns false если пользователь отменил
   */
  async function createNewProject(): Promise<boolean> {
    const name = window.prompt("Название проекта:", "Новый проект");
    if (name === null) return false;
    const trimmed = name.trim() || "Новый проект";
    await invoke("project_create_new", { name: trimmed });
    return true;
  }

  /** Список проектов в ~/.rusefui/projects/ */
  async function listProjects(): Promise<ProjectListEntry[]> {
    return invoke<ProjectListEntry[]>("project_list");
  }

  /** Открыть проект из конкретного пути (папка или legacy .rusefui файл). */
  async function openProjectAtPath(path: string): Promise<boolean> {
    await invoke("project_load", { path });
    return true;
  }

  /**
   * Показать системный диалог выбора папки (для проектов вне ~/.rusefui/projects/).
   */
  async function openProject(): Promise<boolean> {
    const path = await invoke<string | null>("pick_project_dir");
    if (!path) return false;
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

  async function saveProject(message?: string): Promise<string | null> {
    await flushProjectUiToStore();
    return invoke<string>("project_save", { message: message ?? null });
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

  async function clearTimeline(): Promise<boolean> {
    return invoke<boolean>("project_clear_timeline");
  }

  /** Форк без timeline. newName = "" → автоимя "... (копия)". */
  async function copyProjectWithoutTimeline(newName?: string): Promise<boolean> {
    const name = newName ?? "";
    await flushProjectUiToStore();
    await invoke("project_copy_without_timeline", { newName: name });
    return true;
  }

  // --- Скрипты ---

  async function listScripts(): Promise<ProjectScript[]> {
    return invoke<ProjectScript[]>("project_script_list");
  }

  async function createScript(name: string): Promise<ProjectScript> {
    return invoke<ProjectScript>("project_script_create", { name });
  }

  async function deleteScript(id: string): Promise<void> {
    await invoke("project_script_delete", { id });
  }

  async function getScriptContent(id: string): Promise<string> {
    return invoke<string>("project_script_get_content", { id });
  }

  async function setScriptContent(id: string, content: string): Promise<void> {
    await invoke("project_script_set_content", { id, content });
  }

  async function scriptEcuRead(scriptField: string): Promise<string> {
    return invoke<string>("project_script_ecu_read", { scriptField });
  }

  async function scriptEcuWrite(scriptField: string, content: string): Promise<void> {
    await invoke("project_script_ecu_write", { scriptField, content });
  }

  async function scriptEcuBurn(): Promise<void> {
    await invoke("project_script_ecu_burn");
  }

  async function importScript(path: string): Promise<ProjectScript> {
    return invoke<ProjectScript>("project_script_import", { path });
  }

  async function scriptHistory(id: string): Promise<CommitSummary[]> {
    return invoke<CommitSummary[]>("project_script_history", { id });
  }

  async function scriptDiff(id: string, fromId: string, toId?: string): Promise<string> {
    return invoke<string>("project_script_diff", {
      id,
      fromId,
      toId: toId ?? null,
    });
  }

  async function checkoutScriptVersion(id: string, commitId: string): Promise<string> {
    return invoke<string>("project_script_checkout_version", { id, commitId });
  }

  // --- История ---

  async function historyList(): Promise<CommitSummary[]> {
    return invoke<CommitSummary[]>("project_history_list");
  }

  async function diffCommits(
    fromId: string,
    toId?: string,
  ): Promise<string> {
    return invoke<string>("project_diff", {
      fromId,
      toId: toId ?? null,
    });
  }

  async function checkoutCommit(commitId: string): Promise<void> {
    await invoke("project_checkout", { commitId });
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
    listProjects,
    listRecentProjects,
    closeProject,
    saveProject,
    captureEcuConfig,
    addLog,
    removeLog,
    listLogs,
    getProjectUi,
    setProjectUi,
    listPersistKeys,
    clearTimeline,
    copyProjectWithoutTimeline,
    listScripts,
    createScript,
    deleteScript,
    getScriptContent,
    setScriptContent,
    scriptEcuRead,
    scriptEcuWrite,
    scriptEcuBurn,
    importScript,
    scriptHistory,
    scriptDiff,
    checkoutScriptVersion,
    historyList,
    diffCommits,
    checkoutCommit,
  };
}
