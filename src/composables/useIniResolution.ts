import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, readonly, shallowRef } from "vue";

/** Зеркалит `IniCandidateSource` в Rust. */
export type IniCandidateSource = "envOverride" | "cache" | "localDir";

/** Зеркалит `IniCandidate` в Rust. */
export interface IniCandidate {
  path: string;
  fileName: string;
  source: IniCandidateSource;
  signature: string | null;
  matchesEcu: boolean;
  bundleTarget: string | null;
  sizeBytes: number;
}

/** Зеркалит `OnlineDownloadStatus` в Rust (`tag = "kind"`). */
export type OnlineDownloadStatus =
  | { kind: "notApplicable" }
  | { kind: "notAttempted"; reason: string }
  | { kind: "succeeded"; path: string; url: string }
  | { kind: "failed"; url: string; error: string };

/** Зеркалит `IniResolutionInfo` в Rust. */
export interface IniResolutionInfo {
  pending: boolean;
  ecuSignature: string | null;
  portName: string | null;
  projectSignature: string | null;
  bundleTarget: string | null;
  lastError: string | null;
  online: OnlineDownloadStatus | null;
  suggestedIniPath: string | null;
}

const defaultInfo: IniResolutionInfo = {
  pending: false,
  ecuSignature: null,
  portName: null,
  projectSignature: null,
  bundleTarget: null,
  lastError: null,
  online: null,
  suggestedIniPath: null,
};

const info = shallowRef<IniResolutionInfo>(defaultInfo);
let initPromise: Promise<void> | null = null;
let unlisten: UnlistenFn | null = null;

export async function initIniResolution(): Promise<void> {
  if (initPromise) return initPromise;
  initPromise = (async () => {
    try {
      info.value = await invoke<IniResolutionInfo>("ini_get_resolution");
    } catch {
      /* не Tauri */
    }
    if (!unlisten) {
      unlisten = await listen<IniResolutionInfo>("ini-resolution", (ev) => {
        info.value = ev.payload;
      });
    }
  })();
  return initPromise;
}

/** Свежие данные resolution (без ожидания event'а). */
export async function refreshIniResolution(): Promise<IniResolutionInfo> {
  try {
    info.value = await invoke<IniResolutionInfo>("ini_get_resolution");
  } catch {
    /* ignore */
  }
  return info.value;
}

export async function listIniCandidates(): Promise<IniCandidate[]> {
  try {
    return await invoke<IniCandidate[]>("ini_list_candidates");
  } catch {
    return [];
  }
}

export async function applyIniPath(
  path: string,
  force: boolean,
  updateProjectRef: boolean = true,
): Promise<void> {
  await invoke("ini_apply_path", {
    path,
    force,
    updateProjectRef,
  });
}

export async function retryOnlineIniDownload(): Promise<string> {
  return await invoke<string>("ini_retry_online_download");
}

export async function pickIniFile(): Promise<string | null> {
  try {
    const res = await invoke<string | null>("ini_pick_file");
    return res ?? null;
  } catch {
    return null;
  }
}

export function useIniResolution() {
  const pending = computed(() => info.value.pending);
  const ecuSignature = computed(() => info.value.ecuSignature);
  const projectSignature = computed(() => info.value.projectSignature);
  const projectMismatch = computed(
    () =>
      !!info.value.projectSignature &&
      !!info.value.ecuSignature &&
      info.value.projectSignature !== info.value.ecuSignature,
  );
  return {
    info: readonly(info),
    pending,
    ecuSignature,
    projectSignature,
    projectMismatch,
    refreshIniResolution,
    listIniCandidates,
    applyIniPath,
    retryOnlineIniDownload,
    pickIniFile,
  };
}
