import type { TimeSeries } from "./useTimeSeriesBuffer";

const FONT_AXIS = "10px Segoe UI, system-ui, sans-serif";

export interface ChartMargins {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

const DEFAULT_MARGINS: ChartMargins = { top: 12, right: 12, bottom: 28, left: 52 };

/** Одна кривая на панели log (свой диапазон Y, общая ось времени). */
export interface LogTraceSpec {
  series: TimeSeries;
  vMin: number;
  vMax: number;
  name: string;
  units: string;
  color: string;
  /** Временная кривая при наведении/фокусе списка каналов (ещё не выбрана в граф). */
  preview?: boolean;
}

export interface LogGraphPanelSpec {
  traces: LogTraceSpec[];
  /** Подпись в углу (Граф 1, …). */
  title: string;
}

const CORNER_LINE_H = 10;

export interface LogCrosshairSpec {
  /** X в координатах canvas (CSS px, как `width` графика). */
  x: number;
}

export function logPanelMargins(traceCount: number): ChartMargins {
  const topPad = Math.max(12, 4 + traceCount * CORNER_LINE_H);
  const bottomPad = Math.max(12, 4 + traceCount * CORNER_LINE_H);
  return { top: topPad, right: 12, bottom: bottomPad, left: 8 };
}

/** Значение кривой в момент t (линейная интерполяция между точками). */
export function interpolateSeriesAtTime(
  points: { t: number; v: number }[],
  t: number,
): number | null {
  const pts = points
    .filter((p) => Number.isFinite(p.t) && Number.isFinite(p.v))
    .sort((a, b) => a.t - b.t);
  if (pts.length === 0) return null;
  if (t <= pts[0]!.t) return pts[0]!.v;
  const last = pts[pts.length - 1]!;
  if (t >= last.t) return last.v;
  for (let i = 1; i < pts.length; i++) {
    const p1 = pts[i]!;
    if (p1.t >= t) {
      const p0 = pts[i - 1]!;
      const dt = p1.t - p0.t;
      if (dt < 1e-12) return p1.v;
      const f = (t - p0.t) / dt;
      return p0.v + (p1.v - p0.v) * f;
    }
  }
  return last.v;
}

export function plotXToTime(
  x: number,
  width: number,
  margins: ChartMargins,
  tMin: number,
  tMax: number,
): number | null {
  const plotW = width - margins.left - margins.right;
  if (plotW <= 0) return null;
  if (x < margins.left || x > margins.left + plotW) return null;
  const tSpan = Math.max(tMax - tMin, 0.001);
  return tMin + ((x - margins.left) / plotW) * tSpan;
}

function cssVar(name: string, fallback: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

export function formatTick(v: number): string {
  const abs = Math.abs(v);
  if (abs >= 10000) return v.toFixed(0);
  if (abs >= 100) return v.toFixed(0);
  if (abs >= 10) return v.toFixed(1);
  if (abs >= 1) return v.toFixed(2);
  return v.toFixed(3);
}

/** Классический одиночный график (с осями) — для прочих виджетов. */
export function drawTimeSeriesChart(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  series: TimeSeries[],
  tMin: number,
  tMax: number,
  vMin: number,
  vMax: number,
  margins: ChartMargins = DEFAULT_MARGINS,
): void {
  ctx.clearRect(0, 0, width, height);

  const plotW = width - margins.left - margins.right;
  const plotH = height - margins.top - margins.bottom;
  if (plotW <= 0 || plotH <= 0) return;

  const tSpan = Math.max(tMax - tMin, 0.001);
  const vSpan = Math.max(vMax - vMin, 0.001);

  const toX = (t: number) => margins.left + ((t - tMin) / tSpan) * plotW;
  const toY = (v: number) => margins.top + plotH - ((v - vMin) / vSpan) * plotH;

  ctx.fillStyle = cssVar("--color-bg-elevated", "#fff");
  ctx.fillRect(margins.left, margins.top, plotW, plotH);

  ctx.strokeStyle = cssVar("--color-border", "#e0d9ce");
  ctx.lineWidth = 1;

  const yTicks = 5;
  ctx.font = FONT_AXIS;
  ctx.fillStyle = cssVar("--color-text-subtle", "#9c948a");
  ctx.textAlign = "right";
  ctx.textBaseline = "middle";

  for (let i = 0; i <= yTicks; i++) {
    const frac = i / yTicks;
    const v = vMin + (1 - frac) * vSpan;
    const y = margins.top + frac * plotH;
    ctx.beginPath();
    ctx.moveTo(margins.left, y);
    ctx.lineTo(margins.left + plotW, y);
    ctx.stroke();
    ctx.fillText(formatTick(v), margins.left - 6, y);
  }

  const xTicks = 6;
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  for (let i = 0; i <= xTicks; i++) {
    const frac = i / xTicks;
    const t = tMin + frac * tSpan;
    const x = margins.left + frac * plotW;
    ctx.beginPath();
    ctx.moveTo(x, margins.top);
    ctx.lineTo(x, margins.top + plotH);
    ctx.stroke();
    const rel = tMax - t;
    const label = rel <= 0.5 ? "now" : `−${rel.toFixed(0)}s`;
    ctx.fillText(label, x, margins.top + plotH + 6);
  }

  ctx.strokeStyle = cssVar("--color-border-strong", "#cfc6b8");
  ctx.strokeRect(margins.left, margins.top, plotW, plotH);

  ctx.save();
  ctx.beginPath();
  ctx.rect(margins.left, margins.top, plotW, plotH);
  ctx.clip();

  for (const s of series) {
    if (s.points.length < 2) continue;
    ctx.strokeStyle = s.color;
    ctx.lineWidth = 1.75;
    ctx.lineJoin = "round";
    ctx.beginPath();
    let started = false;
    for (const p of s.points) {
      if (p.t < tMin - 0.01) continue;
      const x = toX(p.t);
      const y = toY(p.v);
      if (!started) {
        ctx.moveTo(x, y);
        started = true;
      } else {
        ctx.lineTo(x, y);
      }
    }
    ctx.stroke();
  }

  ctx.restore();
}
