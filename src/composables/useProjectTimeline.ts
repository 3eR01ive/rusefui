import { invoke } from "@tauri-apps/api/core";
import { computed, ref, shallowRef, watch } from "vue";
import { projectUiEpoch, workspaceResetEpoch } from "./useProject";

export type TimelineChannel = "logs" | "trigger" | "spectrogram" | "runs";

export interface ProjectTimelineRecordRef {
  path: string;
  kind?: string | null;
}

export interface ProjectTimelineClip {
  id: string;
  channel: TimelineChannel;
  startMs: number;
  endMs?: number | null;
  record: ProjectTimelineRecordRef;
  label?: string | null;
}

export const TIMELINE_CHANNELS: ReadonlyArray<{
  id: TimelineChannel;
  title: string;
}> = [
  { id: "logs", title: "Логи" },
  { id: "trigger", title: "Триггер" },
  { id: "spectrogram", title: "Спектрограмма" },
  { id: "runs", title: "Прогоны" },
];

const clips = shallowRef<ProjectTimelineClip[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);
let initPromise: Promise<void> | null = null;

async function refresh(): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    clips.value = await invoke<ProjectTimelineClip[]>("project_timeline_list");
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
    clips.value = [];
  } finally {
    loading.value = false;
  }
}

export async function initProjectTimeline(): Promise<void> {
  if (initPromise) return initPromise;
  initPromise = refresh();
  return initPromise;
}

watch([projectUiEpoch, workspaceResetEpoch], () => {
  void refresh();
});

export function useProjectTimeline() {
  const sortedClips = computed(() =>
    [...clips.value].sort((a, b) => a.startMs - b.startMs),
  );

  return {
    clips: sortedClips,
    loading,
    error,
    refresh,
  };
}

export function formatTimelineMs(ms: number): string {
  const d = new Date(ms);
  return d.toLocaleString(undefined, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function formatTimelineMsFull(ms: number): string {
  const d = new Date(ms);
  return d.toLocaleString(undefined, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function channelTitle(channel: string): string {
  return TIMELINE_CHANNELS.find((c) => c.id === channel)?.title ?? channel;
}

export function basename(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || path;
}

export function clipEndMs(clip: ProjectTimelineClip, nowMs: number): number {
  return clip.endMs ?? nowMs;
}

export function formatSpanMs(spanMs: number): string {
  const sec = spanMs / 1000;
  if (sec < 120) return `${Math.round(sec)} с`;
  if (sec < 7200) return `${(sec / 60).toFixed(0)} мин`;
  if (sec < 172800) return `${(sec / 3600).toFixed(1)} ч`;
  return `${(sec / 86400).toFixed(1)} д`;
}
