import { reactive } from "vue";

export interface TimePoint {
  t: number;
  v: number;
}

export interface TimeSeries {
  field: string;
  color: string;
  points: TimePoint[];
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

export function createTimeSeriesStore(windowSeconds: number) {
  const seriesMap = reactive(new Map<string, TimeSeries>());
  let tOrigin: number | null = null;
  let colorIdx = 0;

  function nowSec(): number {
    const ms = performance.now();
    if (tOrigin === null) tOrigin = ms;
    return (ms - tOrigin) / 1000;
  }

  function resetTimeOrigin(): void {
    tOrigin = null;
    for (const s of seriesMap.values()) {
      s.points.length = 0;
    }
  }

  function pickColor(): string {
    const c = SERIES_COLORS[colorIdx % SERIES_COLORS.length]!;
    colorIdx += 1;
    return c;
  }

  function ensureSeries(field: string): TimeSeries {
    let s = seriesMap.get(field);
    if (!s) {
      s = { field, color: pickColor(), points: [] };
      seriesMap.set(field, s);
    }
    return s;
  }

  function addSample(field: string, value: number): void {
    const s = ensureSeries(field);
    const t = nowSec();
    s.points.push({ t, v: value });
    trimSeries(s, t);
  }

  function trimSeries(s: TimeSeries, tNow: number): void {
    const minT = tNow - windowSeconds;
    while (s.points.length > 0 && s.points[0]!.t < minT) {
      s.points.shift();
    }
    const maxPoints = Math.ceil(windowSeconds * 15);
    while (s.points.length > maxPoints) {
      s.points.shift();
    }
  }

  function removeSeries(field: string): void {
    seriesMap.delete(field);
  }

  function setFields(fields: string[]): void {
    for (const key of [...seriesMap.keys()]) {
      if (!fields.includes(key)) removeSeries(key);
    }
    for (const f of fields) ensureSeries(f);
  }

  function visibleRange(): { tMin: number; tMax: number } {
    let tMax = 0;
    for (const s of seriesMap.values()) {
      const last = s.points[s.points.length - 1];
      if (last && last.t > tMax) tMax = last.t;
    }
    if (tMax <= 0) return { tMin: 0, tMax: windowSeconds };
    return { tMin: Math.max(0, tMax - windowSeconds), tMax };
  }

  function valueRange(tMin: number, tMax: number): { vMin: number; vMax: number } {
    let vMin = Infinity;
    let vMax = -Infinity;
    for (const s of seriesMap.values()) {
      for (const p of s.points) {
        if (p.t < tMin || p.t > tMax) continue;
        if (p.v < vMin) vMin = p.v;
        if (p.v > vMax) vMax = p.v;
      }
    }
    if (!Number.isFinite(vMin) || !Number.isFinite(vMax)) {
      return { vMin: 0, vMax: 1 };
    }
    if (vMin === vMax) {
      const pad = Math.abs(vMin) * 0.1 + 1;
      return { vMin: vMin - pad, vMax: vMax + pad };
    }
    const pad = (vMax - vMin) * 0.08;
    return { vMin: vMin - pad, vMax: vMax + pad };
  }

  return {
    seriesMap,
    addSample,
    setFields,
    removeSeries,
    resetTimeOrigin,
    visibleRange,
    valueRange,
  };
}

export type TimeSeriesStore = ReturnType<typeof createTimeSeriesStore>;
