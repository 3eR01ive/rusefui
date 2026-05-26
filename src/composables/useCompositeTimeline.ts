import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, readonly, ref, shallowRef } from "vue";
import type { CompositeEvent } from "./useCompositeLogger";
import type { TimelineViewControl } from "./useOutputTimeline";

export type CompositeTimelineMode = "empty" | "live" | "file";

export interface CompositeTimelineStatus {
  mode: CompositeTimelineMode;
  followLive: boolean;
  dataMinSec: number;
  dataMaxSec: number;
  viewEndSec: number;
  spanSec: number;
  sessionLogPath?: string | null;
  eventCount: number;
  sessionStartMs?: number | null;
}

export interface CompositeTimelineView {
  tMin: number;
  tMax: number;
  followLive: boolean;
  events: CompositeEvent[];
}

export const compositeTimelineLoadEpoch = ref(0);

const status = shallowRef<CompositeTimelineStatus>({
  mode: "empty",
  followLive: false,
  dataMinSec: 0,
  dataMaxSec: 0,
  viewEndSec: 0,
  spanSec: 0.5,
  eventCount: 0,
});

let initPromise: Promise<void> | null = null;
let unlisten: UnlistenFn | null = null;

export async function initCompositeTimeline(): Promise<void> {
  if (initPromise) return initPromise;
  initPromise = (async () => {
    try {
      status.value = await invoke<CompositeTimelineStatus>("composite_timeline_status");
    } catch {
      /* not in tauri */
    }
    if (!unlisten) {
      unlisten = await listen<CompositeTimelineStatus>("composite-timeline-status", (ev) => {
        status.value = ev.payload;
      });
      await listen("workspace-reset", async () => {
        compositeTimelineLoadEpoch.value += 1;
        await refreshCompositeTimelineStatus();
      });
    }
  })();
  return initPromise;
}

export async function refreshCompositeTimelineStatus(): Promise<CompositeTimelineStatus> {
  try {
    status.value = await invoke<CompositeTimelineStatus>("composite_timeline_status");
  } catch {
    /* ignore */
  }
  return status.value;
}

export type CompositeTimelineViewport = {
  viewEndSec: number;
  spanSec: number;
};

export async function queryCompositeTimelineView(
  pixelWidth: number,
  viewport?: CompositeTimelineViewport,
): Promise<CompositeTimelineView> {
  return invoke<CompositeTimelineView>("composite_timeline_query_view", {
    params: {
      pixelWidth: Math.max(64, Math.floor(pixelWidth)),
      viewEndSec: viewport?.viewEndSec,
      spanSec: viewport?.spanSec,
    },
  });
}

export async function controlCompositeTimelineView(
  ctrl: TimelineViewControl,
): Promise<CompositeTimelineStatus> {
  status.value = await invoke<CompositeTimelineStatus>("composite_timeline_set_view", {
    params: { ctrl },
  });
  return status.value;
}

export async function loadCompositeTimelineFile(
  path: string,
): Promise<CompositeTimelineStatus> {
  status.value = await invoke<CompositeTimelineStatus>("composite_timeline_load_file", {
    path,
  });
  compositeTimelineLoadEpoch.value += 1;
  return status.value;
}

export async function pickAndLoadCompositeLogFile(): Promise<CompositeTimelineStatus | null> {
  const path = await invoke<string | null>("pick_composite_log_path");
  if (!path) return null;
  const st = await loadCompositeTimelineFile(path);
  try {
    await invoke("project_add_log", { path, label: null, kind: "composite_csv" });
  } catch {
    /* not in tauri */
  }
  return st;
}

export function useCompositeTimeline() {
  const hasFile = computed(() => status.value.mode === "file" && status.value.eventCount > 0);

  return {
    status: readonly(status),
    hasFile,
    refreshStatus: refreshCompositeTimelineStatus,
    queryView: queryCompositeTimelineView,
    controlView: controlCompositeTimelineView,
    loadFile: loadCompositeTimelineFile,
    pickAndLoadFile: pickAndLoadCompositeLogFile,
  };
}
