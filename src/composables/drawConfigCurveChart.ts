export interface CurvePoint {
  x: number;
  y: number;
}

export interface ChartMargins {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

const FONT_AXIS = "10px Segoe UI, system-ui, sans-serif";
const DEFAULT_MARGINS: ChartMargins = { top: 16, right: 16, bottom: 32, left: 48 };

function cssVar(name: string, fallback: string): string {
  return (
    getComputedStyle(document.documentElement).getPropertyValue(name).trim() ||
    fallback
  );
}

function formatTick(v: number): string {
  const abs = Math.abs(v);
  if (abs >= 10000) return v.toFixed(0);
  if (abs >= 100) return v.toFixed(0);
  if (abs >= 10) return v.toFixed(1);
  if (abs >= 1) return v.toFixed(2);
  return v.toFixed(3);
}

function boundsWithPadding(
  min: number,
  max: number,
  ratio = 0.08,
): { min: number; max: number } {
  if (!Number.isFinite(min) || !Number.isFinite(max)) {
    return { min: 0, max: 1 };
  }
  if (Math.abs(max - min) < 1e-9) {
    const pad = Math.max(Math.abs(min), 1) * 0.1;
    return { min: min - pad, max: max + pad };
  }
  const pad = (max - min) * ratio;
  return { min: min - pad, max: max + pad };
}

/** Статичная калибровочная кривая (X → Y). */
export function drawConfigCurveChart(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  points: CurvePoint[],
  margins: ChartMargins = DEFAULT_MARGINS,
): void {
  ctx.clearRect(0, 0, width, height);

  const plotW = width - margins.left - margins.right;
  const plotH = height - margins.top - margins.bottom;
  if (plotW <= 0 || plotH <= 0) return;

  const valid = points.filter(
    (p) => Number.isFinite(p.x) && Number.isFinite(p.y),
  );
  if (valid.length === 0) {
    ctx.font = FONT_AXIS;
    ctx.fillStyle = cssVar("--color-text-subtle", "#9c948a");
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText("Нет данных", width / 2, height / 2);
    return;
  }

  const sorted = [...valid].sort((a, b) => a.x - b.x);
  const xMinRaw = sorted[0]!.x;
  const xMaxRaw = sorted[sorted.length - 1]!.x;
  let yMinRaw = sorted[0]!.y;
  let yMaxRaw = sorted[0]!.y;
  for (const p of sorted) {
    yMinRaw = Math.min(yMinRaw, p.y);
    yMaxRaw = Math.max(yMaxRaw, p.y);
  }

  const { min: xMin, max: xMax } = boundsWithPadding(xMinRaw, xMaxRaw);
  const { min: yMin, max: yMax } = boundsWithPadding(yMinRaw, yMaxRaw);
  const xSpan = Math.max(xMax - xMin, 1e-9);
  const ySpan = Math.max(yMax - yMin, 1e-9);

  const toX = (x: number) => margins.left + ((x - xMin) / xSpan) * plotW;
  const toY = (y: number) => margins.top + plotH - ((y - yMin) / ySpan) * plotH;

  ctx.fillStyle = cssVar("--color-bg-elevated", "#fff");
  ctx.fillRect(margins.left, margins.top, plotW, plotH);

  ctx.strokeStyle = cssVar("--color-border", "#e0d9ce");
  ctx.lineWidth = 1;
  ctx.font = FONT_AXIS;
  ctx.fillStyle = cssVar("--color-text-subtle", "#9c948a");

  const yTicks = 5;
  ctx.textAlign = "right";
  ctx.textBaseline = "middle";
  for (let i = 0; i <= yTicks; i++) {
    const frac = i / yTicks;
    const v = yMin + (1 - frac) * ySpan;
    const y = margins.top + frac * plotH;
    ctx.beginPath();
    ctx.moveTo(margins.left, y);
    ctx.lineTo(margins.left + plotW, y);
    ctx.stroke();
    ctx.fillText(formatTick(v), margins.left - 6, y);
  }

  const xTicks = Math.min(6, Math.max(2, sorted.length - 1));
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  for (let i = 0; i <= xTicks; i++) {
    const frac = i / xTicks;
    const v = xMin + frac * xSpan;
    const x = margins.left + frac * plotW;
    ctx.beginPath();
    ctx.moveTo(x, margins.top);
    ctx.lineTo(x, margins.top + plotH);
    ctx.stroke();
    ctx.fillText(formatTick(v), x, margins.top + plotH + 6);
  }

  ctx.strokeStyle = cssVar("--color-border-strong", "#cfc6b8");
  ctx.strokeRect(margins.left, margins.top, plotW, plotH);

  const lineColor = cssVar("--color-accent", "#3d7ea6");
  const pointFill = cssVar("--color-bg", "#fff");

  ctx.save();
  ctx.beginPath();
  ctx.rect(margins.left, margins.top, plotW, plotH);
  ctx.clip();

  if (sorted.length >= 2) {
    ctx.strokeStyle = lineColor;
    ctx.lineWidth = 2;
    ctx.lineJoin = "round";
    ctx.beginPath();
    ctx.moveTo(toX(sorted[0]!.x), toY(sorted[0]!.y));
    for (let i = 1; i < sorted.length; i++) {
      ctx.lineTo(toX(sorted[i]!.x), toY(sorted[i]!.y));
    }
    ctx.stroke();
  }

  const r = 4;
  for (const p of sorted) {
    const x = toX(p.x);
    const y = toY(p.y);
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.fillStyle = pointFill;
    ctx.fill();
    ctx.strokeStyle = lineColor;
    ctx.lineWidth = 1.5;
    ctx.stroke();
  }

  ctx.restore();
}
