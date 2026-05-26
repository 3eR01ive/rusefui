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

function padValueRange(vMin: number, vMax: number): { vMin: number; vMax: number } {
  if (vMin === vMax) {
    const pad = Math.abs(vMin) * 0.1 + 1;
    return { vMin: vMin - pad, vMax: vMax + pad };
  }
  const pad = (vMax - vMin) * 0.08;
  return { vMin: vMin - pad, vMax: vMax + pad };
}

export function createTimeSeriesStore(initialWindowSeconds: number) {
  const seriesMap = reactive(new Map<string, TimeSeries>());
  let windowSeconds = initialWindowSeconds;
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
    const maxPoints = Math.ceil(windowSeconds * 250);
    while (s.points.length > maxPoints) {
      s.points.shift();
    }
  }

  function setWindowSeconds(sec: number): void {
    if (sec > 0) windowSeconds = sec;
  }

  function getSeries(field: string): TimeSeries | undefined {
    return seriesMap.get(field);
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
    const span = windowSeconds;
    if (tMax <= 0) return { tMin: 0, tMax: span };
    // Пока данных меньше окна — ось 0..span, иначе кривая прилипает к правому краю.
    if (tMax < span) return { tMin: 0, tMax: span };
    return { tMin: tMax - span, tMax };
  }

  function valueRangeForSeries(
    s: TimeSeries,
    tMin: number,
    tMax: number,
    yMin: number | null,
    yMax: number | null,
  ): { vMin: number; vMax: number } {
    let dataMin = Infinity;
    let dataMax = -Infinity;
    for (const p of s.points) {
      if (p.t < tMin || p.t > tMax) continue;
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
      return padValueRange(dataMin, dataMax);
    }
    return { vMin, vMax };
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
    valueRangeForSeries,
    setWindowSeconds,
    getSeries,
  };
}

export type TimeSeriesStore = ReturnType<typeof createTimeSeriesStore>;
