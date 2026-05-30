import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { ref } from "vue";
import type { ProjectInfo } from "./useProject";
import {
  timelineRenderer,
  type ProjectTimelineClip,
} from "./timelineRenderer";
import { activeTabId } from "./useTabState";

export {
  TIMELINE_CHANNEL_LABELS,
  timelineRenderer,
} from "./timelineRenderer";
export type { TimelineFrame, ProjectTimelineClip } from "./timelineFrame";

const spanLabel = ref("…");
const loading = ref(false);
const error = ref<string | null>(null);

let listenersReady: Promise<void> | null = null;
let unlistenProject: UnlistenFn | null = null;
let _unlistenReset: UnlistenFn | null = null;
let timelineKey = "";
let clipsLoaded = false;

function timelineDataKey(info: ProjectInfo): string {
  return `${info.path ?? ""}:${info.logCount}:${info.timelineClipCount}`;
}

function timelineTabVisible(): boolean {
  return activeTabId.value === "timeline";
}

/** Один IPC — список клипов; viewport полностью на клиенте. */
export async function reloadTimelineClips(paint = true): Promise<void> {
  loading.value = true;
  error.value = null;
  const doPaint = paint && timelineTabVisible();
  try {
    const clips = await invoke<ProjectTimelineClip[]>("project_timeline_list");
    timelineRenderer.setClips(clips);
    if (paint) timelineRenderer.rebuild(doPaint);
    else spanLabel.value = timelineRenderer.spanLabel();
    clipsLoaded = true;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
    timelineRenderer.setClips([]);
    if (paint) timelineRenderer.rebuild(doPaint);
  } finally {
    loading.value = false;
  }
}

async function setupListeners(): Promise<void> {
  if (unlistenProject) return;
  unlistenProject = await listen<ProjectInfo>("project-changed", (ev) => {
    const key = timelineDataKey(ev.payload);
    if (key === timelineKey) return;
    timelineKey = key;
    clipsLoaded = false;
    void reloadTimelineClips(true);
  });
  _unlistenReset = await listen("workspace-reset", () => {
    timelineKey = "";
    clipsLoaded = false;
    const paint = timelineTabVisible();
    timelineRenderer.reset(paint);
    spanLabel.value = timelineRenderer.spanLabel();
    void reloadTimelineClips(paint);
  });
}

export async function ensureTimelineListeners(): Promise<void> {
  if (!listenersReady) {
    listenersReady = setupListeners();
  }
  await listenersReady;
}

export async function ensureTimelineClipsLoaded(): Promise<void> {
  await ensureTimelineListeners();
  if (clipsLoaded) return;
  try {
    const info = await invoke<ProjectInfo>("project_get_info");
    timelineKey = timelineDataKey(info);
  } catch {
    timelineKey = "";
  }
  await reloadTimelineClips(true);
}

export function useProjectTimeline() {
  return {
    spanLabel,
    loading,
    error,
  };
}
