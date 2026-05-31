import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { ref } from "vue";
import type { ProjectInfo } from "./useProject";
import {
  TIMELINE_CHANNEL_LABELS,
  timelineRenderer,
  type ProjectTimelineClip,
} from "./timelineRenderer";
import {
  buildMockTimelineClips,
  USE_MOCK_TIMELINE_CLIPS,
} from "./timelineMockClips";
import { activeTabId } from "./useTabState";

export {
  TIMELINE_CHANNEL_LABELS,
  timelineRenderer,
} from "./timelineRenderer";
export type { TimelineFrame, ProjectTimelineClip, TimelineTickDraw } from "./timelineFrame";

const spanLabel = ref("…");
const loading = ref(false);
const error = ref<string | null>(null);
const channelClipCounts = ref<Record<string, number>>({});

function countClipsByChannel(clips: ProjectTimelineClip[]): Record<string, number> {
  const counts: Record<string, number> = Object.fromEntries(
    TIMELINE_CHANNEL_LABELS.map((ch) => [ch.id, 0]),
  );
  for (const clip of clips) {
    counts[clip.channel] = (counts[clip.channel] ?? 0) + 1;
  }
  return counts;
}

let listenersReady: Promise<void> | null = null;
let unlistenProject: UnlistenFn | null = null;
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
    const clips = USE_MOCK_TIMELINE_CLIPS
      ? buildMockTimelineClips()
      : await invoke<ProjectTimelineClip[]>("project_timeline_list");
    timelineRenderer.setClips(clips);
    channelClipCounts.value = countClipsByChannel(clips);
    if (USE_MOCK_TIMELINE_CLIPS) {
      timelineRenderer.resetView(doPaint);
    } else if (paint) {
      timelineRenderer.rebuild(doPaint);
    } else {
      spanLabel.value = timelineRenderer.spanLabel();
    }
    clipsLoaded = true;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
    timelineRenderer.setClips([]);
    channelClipCounts.value = countClipsByChannel([]);
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
  void listen("workspace-reset", () => {
    timelineKey = "";
    clipsLoaded = false;
    const paint = timelineTabVisible();
    timelineRenderer.reset(paint);
    spanLabel.value = timelineRenderer.spanLabel();
    channelClipCounts.value = countClipsByChannel([]);
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
    channelClipCounts,
  };
}
