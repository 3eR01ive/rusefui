import type { TimeSeries } from "./useTimeSeriesBuffer";

const FONT_AXIS = "10px Segoe UI, system-ui, sans-serif";
const FONT_CORNER = "10px ui-monospace, SFMono-Regular, Menlo, monospace";

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

const PANEL_GAP = 2;
const CORNER_LINE_H = 10;
const OUTER_TOP = 1;
const OUTER_BOTTOM = 2;

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

function drawCrosshairMarker(
  ctx: CanvasRenderingContext2D,
  plotRight: number,
  x: number,
  y: number,
  color: string,
  label: string,
): void {
  ctx.save();
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.arc(x, y, 4, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = cssVar("--color-bg-elevated", "#fff");
  ctx.lineWidth = 1.5;
  ctx.stroke();

  ctx.font = "600 10px Segoe UI, system-ui, sans-serif";
  const tw = ctx.measureText(label).width;
  let lx = x + 8;
  if (lx + tw > plotRight - 4) lx = x - 8 - tw;
  const ly = y;
  const padX = 4;
  const padY = 2;
  const boxH = 14;
  ctx.fillStyle = cssVar("--color-bg-elevated", "#fff");
  ctx.globalAlpha = 0.92;
  ctx.fillRect(lx - padX, ly - boxH / 2 - padY, tw + padX * 2, boxH + padY * 2);
  ctx.globalAlpha = 1;
  ctx.strokeStyle = color;
  ctx.lineWidth = 1;
  ctx.strokeRect(lx - padX, ly - boxH / 2 - padY, tw + padX * 2, boxH + padY * 2);
  ctx.fillStyle = color;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  ctx.fillText(label, lx, ly);
  ctx.restore();
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

/** Панель log: несколько параметров, min/max в углах, без подписей оси X. */
export function drawLogGraphPanel(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  traces: LogTraceSpec[],
  tMin: number,
  tMax: number,
  crosshairT: number | null = null,
): void {
  const margins = logPanelMargins(traces.length);

  const plotW = width - margins.left - margins.right;
  const plotH = height - margins.top - margins.bottom;
  if (plotW <= 0 || plotH <= 0) return;

  const plotTMin = tMin;
  const plotTMax = tMax;
  const tSpan = Math.max(plotTMax - plotTMin, 0.001);
  const toX = (t: number) => margins.left + ((t - plotTMin) / tSpan) * plotW;

  ctx.fillStyle = cssVar("--color-bg-elevated", "#fff");
  ctx.fillRect(margins.left, margins.top, plotW, plotH);

  ctx.strokeStyle = cssVar("--color-border", "#e0d9ce");
  ctx.lineWidth = 1;
  const gridLines = 4;
  for (let i = 0; i <= gridLines; i++) {
    const y = margins.top + (i / gridLines) * plotH;
    ctx.beginPath();
    ctx.moveTo(margins.left, y);
    ctx.lineTo(margins.left + plotW, y);
    ctx.stroke();
  }

  ctx.strokeStyle = cssVar("--color-border-strong", "#cfc6b8");
  ctx.strokeRect(margins.left, margins.top, plotW, plotH);

  ctx.save();
  ctx.beginPath();
  ctx.rect(margins.left, margins.top, plotW, plotH);
  ctx.clip();

  for (const tr of traces) {
    const vSpan = Math.max(tr.vMax - tr.vMin, 1e-9);
    const toY = (v: number) =>
      margins.top + plotH - ((v - tr.vMin) / vSpan) * plotH;

    const pts = [...tr.series.points].sort((a, b) => a.t - b.t);
    if (pts.length < 1) continue;
    ctx.save();
    try {
      if (tr.preview) {
        ctx.globalAlpha = 0.72;
        ctx.setLineDash([5, 4]);
        ctx.lineWidth = 1.75;
      } else {
        ctx.setLineDash([]);
        ctx.lineWidth = 2;
      }
      ctx.strokeStyle = tr.color;
      ctx.lineJoin = "round";
      ctx.lineCap = "round";
      if (pts.length === 1) {
        const p = pts[0]!;
        const x = toX(p.t);
        const y = toY(p.v);
        if (!Number.isFinite(x) || !Number.isFinite(y)) continue;
        ctx.beginPath();
        ctx.arc(x, y, tr.preview ? 2 : 2.5, 0, Math.PI * 2);
        ctx.fillStyle = tr.color;
        ctx.globalAlpha = tr.preview ? 0.72 : 1;
        ctx.fill();
        continue;
      }
      ctx.beginPath();
      let started = false;
      for (const p of pts) {
        const x = toX(p.t);
        const y = toY(p.v);
        if (!Number.isFinite(x) || !Number.isFinite(y)) continue;
        if (!started) {
          ctx.moveTo(x, y);
          started = true;
        } else {
          ctx.lineTo(x, y);
        }
      }
      if (started) ctx.stroke();
    } finally {
      ctx.restore();
    }
  }

  if (crosshairT !== null && Number.isFinite(crosshairT)) {
    const cx = toX(crosshairT);
    if (cx >= margins.left && cx <= margins.left + plotW) {
      const plotRight = margins.left + plotW;
      for (const tr of traces) {
        const v = interpolateSeriesAtTime(tr.series.points, crosshairT);
        if (v === null) continue;
        const vSpan = Math.max(tr.vMax - tr.vMin, 1e-9);
        const cy = margins.top + plotH - ((v - tr.vMin) / vSpan) * plotH;
        if (!Number.isFinite(cy)) continue;
        const unit = tr.units ? ` ${tr.units}` : "";
        drawCrosshairMarker(
          ctx,
          plotRight,
          cx,
          cy,
          tr.color,
          `${tr.name} ${formatTick(v)}${unit}`,
        );
      }
    }
  }

  ctx.restore();

  ctx.font = FONT_CORNER;
  ctx.textAlign = "left";
  ctx.textBaseline = "top";

  let yMax = margins.top + 2;
  for (const tr of traces) {
    const unit = tr.units ? ` (${tr.units})` : "";
    ctx.fillStyle = tr.color;
    ctx.fillText(`Max = ${formatTick(tr.vMax)}${unit}`, margins.left + 4, yMax);
    yMax += CORNER_LINE_H;
  }

  let yMin = margins.top + plotH - 2 - traces.length * CORNER_LINE_H;
  for (const tr of traces) {
    const unit = tr.units ? ` (${tr.units})` : "";
    ctx.fillStyle = tr.color;
    ctx.textBaseline = "top";
    ctx.fillText(`Min = ${formatTick(tr.vMin)}${unit}`, margins.left + 4, yMin);
    yMin += CORNER_LINE_H;
  }

  // Текущие значения у правого края (последняя точка в окне)
  ctx.textAlign = "right";
  ctx.textBaseline = "middle";
  const tRight = tMax;
  let yCur = margins.top + plotH * 0.5 - ((traces.length - 1) * CORNER_LINE_H) / 2;
  for (const tr of traces) {
    const pts = tr.series.points;
    let val: number | null = null;
    for (let i = pts.length - 1; i >= 0; i--) {
      const p = pts[i]!;
      if (p.t <= tRight + 0.01) {
        val = p.v;
        break;
      }
    }
    if (val !== null) {
      const unit = tr.units ? ` ${tr.units}` : "";
      ctx.fillStyle = tr.color;
      ctx.font = "600 11px Segoe UI, system-ui, sans-serif";
      ctx.fillText(`${formatTick(val)}${unit}`, margins.left + plotW - 4, yCur);
      ctx.font = FONT_CORNER;
    }
    yCur += CORNER_LINE_H;
  }
}

/** Несколько независимых графиков (панелей), на каждой — несколько параметров. */
export function drawLogPanelsChart(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  panels: LogGraphPanelSpec[],
  tMin: number,
  tMax: number,
  crosshair: LogCrosshairSpec | null = null,
): void {
  ctx.clearRect(0, 0, width, height);
  if (panels.length === 0) return;

  const maxTraces = panels.reduce((m, p) => Math.max(m, p.traces.length), 0);
  const sharedMargins = logPanelMargins(maxTraces);
  const crosshairT =
    crosshair !== null
      ? plotXToTime(crosshair.x, width, sharedMargins, tMin, tMax)
      : null;

  const usable = height - OUTER_TOP - OUTER_BOTTOM;
  const panelH =
    (usable - PANEL_GAP * Math.max(0, panels.length - 1)) / panels.length;

  if (crosshairT !== null) {
    const cx = crosshair.x;
    const plotLeft = sharedMargins.left;
    const plotRight = width - sharedMargins.right;
    if (cx >= plotLeft && cx <= plotRight) {
      ctx.save();
      ctx.strokeStyle = cssVar("--color-accent", "#b45309");
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 3]);
      ctx.globalAlpha = 0.45;
      ctx.beginPath();
      ctx.moveTo(cx, OUTER_TOP);
      ctx.lineTo(cx, height - OUTER_BOTTOM);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.globalAlpha = 1;
      ctx.restore();
    }
  }

  panels.forEach((panel, i) => {
    const y0 = OUTER_TOP + i * (panelH + PANEL_GAP);
    ctx.save();
    ctx.translate(0, y0);

    drawLogGraphPanel(ctx, width, panelH, panel.traces, tMin, tMax, crosshairT);

    ctx.font = "600 9px Segoe UI, system-ui, sans-serif";
    ctx.fillStyle = cssVar("--color-text-subtle", "#9c948a");
    ctx.textAlign = "right";
    ctx.textBaseline = "top";
    ctx.fillText(panel.title, width - 10, 4);

    ctx.restore();
  });
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
