import type { TimeSeries } from "./useTimeSeriesBuffer";

const FONT_AXIS = "10px Segoe UI, system-ui, sans-serif";

export interface ChartMargins {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

const DEFAULT_MARGINS: ChartMargins = { top: 12, right: 12, bottom: 28, left: 52 };

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

  // фон области графика
  ctx.fillStyle = getComputedStyle(document.documentElement)
    .getPropertyValue("--color-bg-elevated")
    .trim() || "#fff";
  ctx.fillRect(margins.left, margins.top, plotW, plotH);

  // сетка
  ctx.strokeStyle = getComputedStyle(document.documentElement)
    .getPropertyValue("--color-border")
    .trim() || "#e0d9ce";
  ctx.lineWidth = 1;

  const yTicks = 5;
  ctx.font = FONT_AXIS;
  ctx.fillStyle = getComputedStyle(document.documentElement)
    .getPropertyValue("--color-text-subtle")
    .trim() || "#9c948a";
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
    ctx.fillText(formatTime(t, tMax), x, margins.top + plotH + 6);
  }

  // рамка
  ctx.strokeStyle = getComputedStyle(document.documentElement)
    .getPropertyValue("--color-border-strong")
    .trim() || "#cfc6b8";
  ctx.strokeRect(margins.left, margins.top, plotW, plotH);

  // кривые
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

function formatTick(v: number): string {
  const abs = Math.abs(v);
  if (abs >= 10000) return v.toFixed(0);
  if (abs >= 100) return v.toFixed(0);
  if (abs >= 10) return v.toFixed(1);
  if (abs >= 1) return v.toFixed(2);
  return v.toFixed(3);
}

function formatTime(t: number, tMax: number): string {
  const rel = tMax - t;
  if (rel <= 0.5) return "now";
  return `−${rel.toFixed(0)}s`;
}
