import type { SnifferChannel, SnifferEvent } from "../../composables/useEngineSniffer";

/**
 * Геометрия engine sniffer: динамические каналы (по одному lane на имя),
 * ось X — время кадра (µs). Кадр самодостаточен (время с нуля), поэтому
 * по умолчанию показываем весь кадр; t0/span оставлены для будущего зума.
 */

export interface SnifferTimeRange {
  t0: number;
  spanUs: number;
}

export interface SnifferView {
  t0: number;
  tEnd: number;
  /** Ширина окна (µs). */
  span: number;
  plotLeft: number;
  plotW: number;
  cssW: number;
  cssH: number;
  laneH: number;
  channels: readonly SnifferChannel[];
  events: readonly SnifferEvent[];
  /** Времена TDC-маркеров в окне. */
  tdcTimes: readonly number[];
}

export function buildSnifferView(
  channels: readonly SnifferChannel[],
  events: readonly SnifferEvent[],
  frameSpanUs: number,
  cssW: number,
  cssH: number,
  labelW: number,
  timeRange?: SnifferTimeRange | null,
): SnifferView | null {
  if (cssW <= 0 || cssH <= 0) return null;
  if (channels.length === 0) return null;

  const t0 = timeRange?.t0 ?? 0;
  const span = Math.max(1, timeRange?.spanUs ?? frameSpanUs);
  const tEnd = t0 + span;
  const plotLeft = labelW;
  const plotW = Math.max(1, cssW - labelW - 8);

  const tdcTimes: number[] = [];
  for (const e of events) {
    if (e.tdc && e.tUs >= t0 && e.tUs <= tEnd) tdcTimes.push(e.tUs);
  }

  return {
    t0,
    tEnd,
    span,
    plotLeft,
    plotW,
    cssW,
    cssH,
    laneH: (cssH - 8) / channels.length,
    channels,
    events,
    tdcTimes,
  };
}

export function xAtTime(tUs: number, view: SnifferView): number {
  return view.plotLeft + ((tUs - view.t0) / view.span) * view.plotW;
}

export function timeAtX(x: number, view: SnifferView): number {
  const frac = (x - view.plotLeft) / view.plotW;
  return view.t0 + frac * view.span;
}

/** Y верхнего (high) и нижнего (low) уровня lane по индексу канала. */
export function laneY(
  laneIndex: number,
  view: SnifferView,
): { yHigh: number; yLow: number; yMid: number } {
  const y0 = laneIndex * view.laneH + 4;
  const yHigh = y0 + view.laneH * 0.22;
  const yLow = y0 + view.laneH * 0.78;
  return { yHigh, yLow, yMid: (yHigh + yLow) / 2 };
}

/**
 * Уровень канала на старте окна. Кадр начинается с t0=0, событий левее нет,
 * поэтому до первого фронта держим уровень, обратный первому фронту
 * (чтобы первый фронт был виден как переход).
 */
export function startLevel(channelEvents: readonly SnifferEvent[]): boolean {
  if (channelEvents.length === 0) return false;
  return !channelEvents[0]!.up;
}
