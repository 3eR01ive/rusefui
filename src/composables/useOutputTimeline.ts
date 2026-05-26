import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, readonly, ref, shallowRef } from "vue";

export type TimelineMode = "empty" | "live" | "file" | "liveAndFile";

export interface OutputTimelineStatus {
  mode: TimelineMode;
  connected: boolean;
  followLive: boolean;
  liveSec: number;
  dataMinSec: number;
  dataMaxSec: number;
  viewEndSec: number;
  spanSec: number;
  sessionLogPath?: string | null;
  fieldCount: number;
}

export interface TimelinePoint {
  t: number;
  v: number;
}

export interface TimelineFieldView {
  field: string;
  points: TimelinePoint[];
}

export interface OutputTimelineView {
  tMin: number;
  tMax: number;
  liveSec: number;
  followLive: boolean;
  series: TimelineFieldView[];
}

export interface TimelineViewControl {
  followLive?: boolean;
  viewEndSec?: number;
  spanSec?: number;
  panSec?: number;
  zoomFactor?: number;
}

const SERIES_COLORS = [
  "#e07020",
  "#2a7de1",
  "#3a9e52",
  "#c43d7a",
  "#7a52c9",
  "#c9a227",
  "#1a9e9e",
  "#8b5a2b",
];

const status = shallowRef<OutputTimelineStatus>({
  mode: "empty",
  connected: false,
  followLive: true,
  liveSec: 0,
  dataMinSec: 0,
  dataMaxSec: 0,
  viewEndSec: 0,
  spanSec: 30,
  fieldCount: 0,
});

const fieldColors = ref<Record<string, string>>({});
let colorIdx = 0;
let initPromise: Promise<void> | null = null;
let unlistenEcu: UnlistenFn | null = null;

function pickColor(field: string): string {
  const existing = fieldColors.value[field];
  if (existing) return existing;
  const c = SERIES_COLORS[colorIdx % SERIES_COLORS.length]!;
  colorIdx += 1;
  fieldColors.value = { ...fieldColors.value, [field]: c };
  return c;
}

function ensureFieldColors(fields: string[]): void {
  for (const f of fields) pickColor(f);
}

export async function initOutputTimeline(): Promise<void> {
  if (initPromise) return initPromise;
  initPromise = (async () => {
    try {
      status.value = await invoke<OutputTimelineStatus>("output_timeline_status");
    } catch {
      /* browser dev */
    }
    if (!unlistenEcu) {
      unlistenEcu = await listen("ecu-connection", async () => {
        await refreshTimelineStatus();
      });
    }
  })();
  return initPromise;
}

export async function refreshTimelineStatus(): Promise<OutputTimelineStatus> {
  try {
    status.value = await invoke<OutputTimelineStatus>("output_timeline_status");
  } catch {
    /* ignore */
  }
  return status.value;
}

export async function queryTimelineView(
  fields: string[],
  pixelWidth: number,
): Promise<OutputTimelineView> {
  ensureFieldColors(fields);
  return invoke<OutputTimelineView>("output_timeline_query_view", {
    params: {
      fields,
      pixelWidth: Math.max(64, Math.floor(pixelWidth)),
    },
  });
}

export async function controlTimelineView(
  ctrl: TimelineViewControl,
): Promise<OutputTimelineStatus> {
  status.value = await invoke<OutputTimelineStatus>("output_timeline_set_view", {
    params: { ctrl },
  });
  return status.value;
}

export async function loadTimelineFile(path: string): Promise<OutputTimelineStatus> {
  status.value = await invoke<OutputTimelineStatus>("output_timeline_load_file", { path });
  return status.value;
}

export function timelineFieldColor(field: string): string {
  return pickColor(field);
}

export function valueRangeForPoints(
  points: TimelinePoint[],
  yMin: number | null,
  yMax: number | null,
): { vMin: number; vMax: number } {
  let dataMin = Infinity;
  let dataMax = -Infinity;
  for (const p of points) {
    if (p.v < dataMin) dataMin = p.v;
    if (p.v > dataMax) dataMax = p.v;
  }
  if (yMin !== null && yMax !== null && yMin < yMax) {
    return { vMin: yMin, vMax: yMax };
  }
  if (!Number.isFinite(dataMin) || !Number.isFinite(dataMax)) {
    if (yMin !== null && yMax !== null) return { vMin: yMin, vMax: yMax };
    if (yMin !== null) return { vMin: yMin, vMax: yMin + 1 };
    if (yMax !== null) return { vMin: yMax - 1, vMax: yMax };
    return { vMin: 0, vMax: 1 };
  }
  const vMin = yMin !== null ? yMin : dataMin;
  const vMax = yMax !== null ? yMax : dataMax;
  if (vMin >= vMax) {
    const pad = Math.abs(dataMin) * 0.1 + 1;
    return { vMin: dataMin - pad, vMax: dataMax + pad };
  }
  return { vMin, vMax };
}

export function useOutputTimeline() {
  const hasHistory = computed(
    () =>
      status.value.dataMaxSec > 0 ||
      status.value.mode === "file" ||
      status.value.mode === "liveAndFile",
  );

  return {
    status: readonly(status),
    hasHistory,
    spanSec: computed(() => status.value.spanSec),
    followLive: computed(() => status.value.followLive),
    fieldColor: timelineFieldColor,
    queryView: queryTimelineView,
    controlView: controlTimelineView,
    loadFile: loadTimelineFile,
    refreshStatus: refreshTimelineStatus,
    valueRangeForPoints,
  };
}
