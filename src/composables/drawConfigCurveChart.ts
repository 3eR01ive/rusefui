export interface CurvePoint {
  x: number;
  y: number;
  /** Индекс строки в исходных bins (для hit-test при сортировке по X). */
  row?: number;
}

export interface ChartMargins {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

const FONT_AXIS = "10px Segoe UI, system-ui, sans-serif";
export const DEFAULT_CURVE_CHART_MARGINS: ChartMargins = {
  top: 16,
  right: 16,
  bottom: 32,
  left: 48,
};

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

export interface CurveChartLayout {
  margins: ChartMargins;
  plotW: number;
  plotH: number;
  sorted: CurvePoint[];
  xMin: number;
  xMax: number;
  yMin: number;
  yMax: number;
  toX: (x: number) => number;
  toY: (y: number) => number;
  fromY: (py: number) => number;
}

export function computeCurveChartLayout(
  width: number,
  height: number,
  points: CurvePoint[],
  margins: ChartMargins = DEFAULT_CURVE_CHART_MARGINS,
): CurveChartLayout | null {
  const plotW = width - margins.left - margins.right;
  const plotH = height - margins.top - margins.bottom;
  if (plotW <= 0 || plotH <= 0) return null;

  const valid = points.filter(
    (p) => Number.isFinite(p.x) && Number.isFinite(p.y),
  );
  if (valid.length === 0) return null;

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
  const fromY = (py: number) => {
    const frac = (py - margins.top) / plotH;
    return yMin + (1 - frac) * ySpan;
  };

  return {
    margins,
    plotW,
    plotH,
    sorted,
    xMin,
    xMax,
    yMin,
    yMax,
    toX,
    toY,
    fromY,
  };
}

/** Индекс строки (row) ближайшей точки или null. */
export function hitTestCurvePoint(
  layout: CurveChartLayout,
  px: number,
  py: number,
  radius = 12,
): number | null {
  let bestRow: number | null = null;
  let bestDist = radius * radius;
  for (const p of layout.sorted) {
    const cx = layout.toX(p.x);
    const cy = layout.toY(p.y);
    const dx = px - cx;
    const dy = py - cy;
    const d2 = dx * dx + dy * dy;
    if (d2 <= bestDist) {
      bestDist = d2;
      bestRow = p.row ?? null;
    }
  }
  return bestRow;
}

/** Статичная калибровочная кривая (X → Y). */
export function drawConfigCurveChart(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  points: CurvePoint[],
  margins: ChartMargins = DEFAULT_CURVE_CHART_MARGINS,
  options?: { activeRow?: number | null; hoverRow?: number | null },
): void {
  ctx.clearRect(0, 0, width, height);

  const layout = computeCurveChartLayout(width, height, points, margins);
  if (!layout) {
    ctx.font = FONT_AXIS;
    ctx.fillStyle = cssVar("--color-text-subtle", "#9c948a");
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText("Нет данных", width / 2, height / 2);
    return;
  }

  const { sorted, margins: m, plotW, plotH, xMin, xMax, yMin, yMax, toX, toY } =
    layout;
  const xSpan = Math.max(xMax - xMin, 1e-9);
  const ySpan = Math.max(yMax - yMin, 1e-9);

  ctx.fillStyle = cssVar("--color-bg-elevated", "#fff");
  ctx.fillRect(m.left, m.top, plotW, plotH);

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
    const y = m.top + frac * plotH;
    ctx.beginPath();
    ctx.moveTo(m.left, y);
    ctx.lineTo(m.left + plotW, y);
    ctx.stroke();
    ctx.fillText(formatTick(v), m.left - 6, y);
  }

  const xTicks = Math.min(6, Math.max(2, sorted.length - 1));
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  for (let i = 0; i <= xTicks; i++) {
    const frac = i / xTicks;
    const v = xMin + frac * xSpan;
    const x = m.left + frac * plotW;
    ctx.beginPath();
    ctx.moveTo(x, m.top);
    ctx.lineTo(x, m.top + plotH);
    ctx.stroke();
    ctx.fillText(formatTick(v), x, m.top + plotH + 6);
  }

  ctx.strokeStyle = cssVar("--color-border-strong", "#cfc6b8");
  ctx.strokeRect(m.left, m.top, plotW, plotH);

  const lineColor = cssVar("--color-accent", "#3d7ea6");
  const pointFill = cssVar("--color-bg", "#fff");
  const activeRow = options?.activeRow ?? null;
  const hoverRow = options?.hoverRow ?? null;

  ctx.save();
  ctx.beginPath();
  ctx.rect(m.left, m.top, plotW, plotH);
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

  for (const p of sorted) {
    const row = p.row;
    const isActive = row !== undefined && row === activeRow;
    const isHover = row !== undefined && row === hoverRow;
    const r = isActive ? 6 : isHover ? 5.5 : 4;
    const x = toX(p.x);
    const y = toY(p.y);
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.fillStyle = isActive ? lineColor : pointFill;
    ctx.fill();
    ctx.strokeStyle = lineColor;
    ctx.lineWidth = isActive || isHover ? 2 : 1.5;
    ctx.stroke();
  }

  ctx.restore();
}
