import type { CompositeEvent } from "../../composables/useCompositeLogger";

/** Полный цикл четырёхтактного ДВС (градусы коленвала). */
export const CRANK_CYCLE_DEG = 720;

export type ChannelKey = "pri" | "sec" | "trg" | "sync" | "coil" | "inj";

export interface ChartView {
  t0: number;
  tEnd: number;
  span: number;
  plotW: number;
  plotLeft: number;
  cssW: number;
  cssH: number;
  laneH: number;
  visible: readonly CompositeEvent[];
  /** Времена импульсов TDC (переход trg → true). */
  tdcTimes: number[];
}

export interface ChartTimeRange {
  t0: number;
  tEnd: number;
}

export function channelValue(ev: CompositeEvent, key: ChannelKey): boolean {
  return ev[key];
}

/** Импульсы TDC: фронт 0→1 по каналу `trg`. */
export function findTdcTimes(events: readonly CompositeEvent[]): number[] {
  const out: number[] = [];
  if (events.length === 0) return out;

  let prev = events[0]!.trg;
  if (prev) out.push(events[0]!.tUs);

  for (let i = 1; i < events.length; i++) {
    const cur = events[i]!;
    if (cur.trg && !prev) {
      out.push(cur.tUs);
    }
    prev = cur.trg;
  }
  return out;
}

/** Длительность данных в текущем буфере (мс). */
export function bufferSpanMs(events: readonly CompositeEvent[]): number {
  if (events.length < 2) return 0;
  return (events[events.length - 1]!.tUs - events[0]!.tUs) / 1000;
}

/** Макс. окно: не шире props и не шире буфера логгера. */
export function maxViewSpanMs(
  events: readonly CompositeEvent[],
  configMaxMs: number,
  minMs: number,
): number {
  const bufMs = bufferSpanMs(events);
  const cap = bufMs > 0 ? Math.min(configMaxMs, bufMs) : configMaxMs;
  return Math.max(minMs, cap);
}

/** Срез для ступенчатого графика: точка до t0 + все в [t0, tEnd]. */
export function sliceEventsForRange(
  events: readonly CompositeEvent[],
  t0: number,
  tEnd: number,
): CompositeEvent[] {
  if (events.length === 0) return [];

  let hi = 0;
  while (hi < events.length && events[hi]!.tUs < t0) hi++;
  const start = hi > 0 ? hi - 1 : hi;

  const out: CompositeEvent[] = [];
  for (let i = start; i < events.length && events[i]!.tUs <= tEnd; i++) {
    out.push(events[i]!);
  }
  if (out.length === 0) {
    out.push(events[events.length - 1]!);
  }
  return out;
}

export function buildChartView(
  events: readonly CompositeEvent[],
  _windowMs: number,
  cssW: number,
  cssH: number,
  labelW: number,
  channelCount: number,
  timeRange: ChartTimeRange | null,
): ChartView | null {
  if (events.length < 2 || cssW <= 0 || cssH <= 0 || !timeRange) return null;

  const { t0, tEnd } = timeRange;
  const span = Math.max(1, tEnd - t0);
  const plotLeft = labelW;
  const plotW = cssW - labelW - 8;
  const visible = sliceEventsForRange(events, t0, tEnd);
  if (visible.length < 1) return null;

  const tdcTimes = findTdcTimes(visible);

  return {
    t0,
    tEnd,
    span,
    plotW,
    plotLeft,
    cssW,
    cssH,
    laneH: (cssH - 8) / channelCount,
    visible,
    tdcTimes,
  };
}

export function xAtTime(tUs: number, view: ChartView): number {
  return view.plotLeft + ((tUs - view.t0) / view.span) * view.plotW;
}

export function timeAtX(x: number, view: ChartView): number {
  const frac = (x - view.plotLeft) / view.plotW;
  return view.t0 + frac * view.span;
}

/** Ступенчатое значение канала в момент времени. */
export function valueAtTime(
  tUs: number,
  events: readonly CompositeEvent[],
  key: ChannelKey,
): boolean {
  if (events.length === 0) return false;
  let val = channelValue(events[0]!, key);
  for (const ev of events) {
    if (ev.tUs > tUs) break;
    val = channelValue(ev, key);
  }
  return val;
}

function cyclePeriodUs(tdcTimes: number[], tUs: number): number | null {
  if (tdcTimes.length >= 2) {
    const idx = tdcTimes.findIndex((t) => t > tUs);
    if (idx > 0) return tdcTimes[idx]! - tdcTimes[idx - 1]!;
    if (idx === -1) {
      const last = tdcTimes[tdcTimes.length - 1]!;
      const prev = tdcTimes[tdcTimes.length - 2]!;
      return last - prev;
    }
    if (idx === 0 && tdcTimes.length >= 2) {
      return tdcTimes[1]! - tdcTimes[0]!;
    }
  }
  return null;
}

/**
 * Угол коленвала [0, 720) внутри цикла между соседними TDC.
 * Без TDC — оценка по RPM (об/мин → °/µs).
 */
export function crankAngleDeg(
  tUs: number,
  view: ChartView,
  rpm: number | null | undefined,
): number {
  const { tdcTimes } = view;
  if (tdcTimes.length > 0) {
    let i = 0;
    while (i < tdcTimes.length && tdcTimes[i]! <= tUs) i++;
    const tNext = i < tdcTimes.length ? tdcTimes[i]! : null;
    const tPrev =
      i > 0
        ? tdcTimes[i - 1]!
        : tdcTimes[0]! - (cyclePeriodUs(tdcTimes, tUs) ?? estimatePeriodFromRpm(rpm));

    const tHi = tNext ?? tPrev + (cyclePeriodUs(tdcTimes, tUs) ?? estimatePeriodFromRpm(rpm));
    const period = Math.max(1, tHi - tPrev);
    let deg = ((tUs - tPrev) / period) * CRANK_CYCLE_DEG;
    deg = ((deg % CRANK_CYCLE_DEG) + CRANK_CYCLE_DEG) % CRANK_CYCLE_DEG;
    return deg;
  }

  if (rpm != null && rpm > 0) {
    const degPerUs = (rpm * 360) / 60 / 1_000_000;
    const tRel = tUs - view.t0;
    return (tRel * degPerUs) % CRANK_CYCLE_DEG;
  }

  const frac = (tUs - view.t0) / view.span;
  return frac * CRANK_CYCLE_DEG;
}

function estimatePeriodFromRpm(rpm: number | null | undefined): number {
  if (rpm == null || rpm <= 0) return 100_000;
  return Math.round((2 * 60 * 1_000_000) / rpm);
}

export function laneY(
  laneIndex: number,
  view: ChartView,
  high: boolean,
): { yHigh: number; yLow: number; y: number } {
  const y0 = laneIndex * view.laneH + 4;
  const yHigh = y0 + view.laneH * 0.22;
  const yLow = y0 + view.laneH * 0.78;
  return { yHigh, yLow, y: high ? yHigh : yLow };
}
