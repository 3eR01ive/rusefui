export interface KnockRpmValuePoint {
  rpm: number;
  value: number;
}

/** @deprecated используйте KnockRpmValuePoint */
export interface KnockRunPoint {
  rpm: number;
  knockLevel: number;
  threshold: number;
}

export interface ChartMargins {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

const FONT_AXIS = "10px Segoe UI, system-ui, sans-serif";
const DEFAULT_MARGINS: ChartMargins = { top: 24, right: 20, bottom: 36, left: 52 };

function cssVar(name: string, fallback: string): string {
  return (
    getComputedStyle(document.documentElement).getPropertyValue(name).trim() ||
    fallback
  );
}

function formatTick(v: number): string {
  const abs = Math.abs(v);
  if (abs >= 100) return v.toFixed(0);
  if (abs >= 10) return v.toFixed(1);
  return v.toFixed(2);
}

function boundsWithPadding(
  min: number,
  max: number,
  ratio = 0.1,
): { min: number; max: number } {
  if (!Number.isFinite(min) || !Number.isFinite(max)) {
    return { min: -60, max: 10 };
  }
  if (Math.abs(max - min) < 1e-9) {
    const pad = Math.max(Math.abs(min), 1) * 0.15;
    return { min: min - pad, max: max + pad };
  }
  const pad = (max - min) * ratio;
  return { min: min - pad, max: max + pad };
}

function validRpmPoints(points: KnockRpmValuePoint[]): KnockRpmValuePoint[] {
  return points.filter(
    (p) => Number.isFinite(p.rpm) && Number.isFinite(p.value) && p.rpm >= 0,
  );
}

function drawRpmSeries(
  ctx: CanvasRenderingContext2D,
  points: KnockRpmValuePoint[],
  toX: (rpm: number) => number,
  toY: (v: number) => number,
  color: string,
  style: { dashed?: boolean; opacity?: number; lineWidth?: number } = {},
): void {
  const sorted = [...validRpmPoints(points)].sort((a, b) => a.rpm - b.rpm);
  if (sorted.length === 0) return;

  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = style.lineWidth ?? 2;
  ctx.lineJoin = "round";
  ctx.globalAlpha = style.opacity ?? 1;
  if (style.dashed) ctx.setLineDash([6, 5]);

  if (sorted.length === 1) {
    const p = sorted[0]!;
    ctx.beginPath();
    ctx.arc(toX(p.rpm), toY(p.value), 3.5, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
    return;
  }

  ctx.beginPath();
  ctx.moveTo(toX(sorted[0]!.rpm), toY(sorted[0]!.value));
  for (let i = 1; i < sorted.length; i += 1) {
    ctx.lineTo(toX(sorted[i]!.rpm), toY(sorted[i]!.value));
  }
  ctx.stroke();
  ctx.restore();
}

/**
 * Шаг 1: RPM × dB.
 * — пунктир: knockBaseNoise из config
 * — сплошная: knock level с прогона (output)
 */
export function drawKnockThresholdChart(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  configThreshold: KnockRpmValuePoint[],
  runLevels: KnockRpmValuePoint[],
  margins: ChartMargins = DEFAULT_MARGINS,
  previousRunLevels: KnockRpmValuePoint[] = [],
  liveRpm: number | null = null,
  liveLevel: number | null = null,
  baselineThreshold: KnockRpmValuePoint[] = [],
): void {
  ctx.clearRect(0, 0, width, height);

  const plotW = width - margins.left - margins.right;
  const plotH = height - margins.top - margins.bottom;
  if (plotW <= 0 || plotH <= 0) return;

  const cfgPts = validRpmPoints(configThreshold);
  const basePts = validRpmPoints(baselineThreshold);
  const runPts = validRpmPoints(runLevels);
  const prevPts = validRpmPoints(previousRunLevels);
  const all = [...cfgPts, ...basePts, ...runPts, ...prevPts];

  let yMin = -60;
  let yMax = 10;
  if (all.length > 0) {
    let min = Infinity;
    let max = -Infinity;
    for (const p of all) {
      min = Math.min(min, p.value);
      max = Math.max(max, p.value);
    }
    if (liveLevel != null && Number.isFinite(liveLevel)) {
      min = Math.min(min, liveLevel);
      max = Math.max(max, liveLevel);
    }
    ({ min: yMin, max: yMax } = boundsWithPadding(min, max));
  }

  let xMin = 0;
  let xMax = 7000;
  if (all.length > 0) {
    xMin = Math.min(...all.map((p) => p.rpm));
    xMax = Math.max(...all.map((p) => p.rpm));
  }
  if (liveRpm != null && Number.isFinite(liveRpm)) {
    xMin = Math.min(xMin, liveRpm);
    xMax = Math.max(xMax, liveRpm);
  }
  if (xMax - xMin < 200) xMax = xMin + 200;

  const toX = (rpm: number) => margins.left + ((rpm - xMin) / (xMax - xMin)) * plotW;
  const toY = (v: number) => margins.top + plotH - ((v - yMin) / (yMax - yMin)) * plotH;

  const grid = cssVar("--color-border-subtle", "rgba(255,255,255,0.08)");
  const text = cssVar("--color-text-muted", "#888");
  const levelColor = cssVar("--color-accent", "#5b9cf5");
  const thrColor = cssVar("--color-warning", "#e6a23c");
  const prevColor = cssVar("--color-text-muted", "#666");

  ctx.fillStyle = cssVar("--color-surface-2", "#1a1d24");
  ctx.fillRect(margins.left, margins.top, plotW, plotH);

  ctx.strokeStyle = grid;
  ctx.lineWidth = 1;
  for (let i = 0; i <= 4; i += 1) {
    const y = margins.top + (plotH * i) / 4;
    ctx.beginPath();
    ctx.moveTo(margins.left, y);
    ctx.lineTo(margins.left + plotW, y);
    ctx.stroke();
  }

  drawRpmSeries(ctx, basePts, toX, toY, prevColor, { dashed: true, opacity: 0.35, lineWidth: 1.5 });
  drawRpmSeries(ctx, cfgPts, toX, toY, thrColor, { dashed: true, lineWidth: 2.5 });
  drawRpmSeries(ctx, prevPts, toX, toY, prevColor, { dashed: true, opacity: 0.55 });
  drawRpmSeries(ctx, runPts, toX, toY, levelColor);

  if (liveRpm != null && liveLevel != null && Number.isFinite(liveRpm) && Number.isFinite(liveLevel)) {
    ctx.save();
    ctx.fillStyle = levelColor;
    ctx.beginPath();
    ctx.arc(toX(liveRpm), toY(liveLevel), 4, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  ctx.fillStyle = text;
  ctx.font = FONT_AXIS;
  ctx.textAlign = "right";
  ctx.textBaseline = "middle";
  for (let i = 0; i <= 4; i += 1) {
    const v = yMin + ((yMax - yMin) * (4 - i)) / 4;
    const y = margins.top + (plotH * i) / 4;
    ctx.fillText(formatTick(v), margins.left - 6, y);
  }

  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  for (let i = 0; i <= 4; i += 1) {
    const rpm = xMin + ((xMax - xMin) * i) / 4;
    const x = margins.left + (plotW * i) / 4;
    ctx.fillText(String(Math.round(rpm)), x, margins.top + plotH + 8);
  }

  ctx.textAlign = "left";
  ctx.fillText("RPM", margins.left + plotW / 2 - 12, height - 6);
  ctx.save();
  ctx.translate(12, margins.top + plotH / 2);
  ctx.rotate(-Math.PI / 2);
  ctx.fillText("dB", 0, 0);
  ctx.restore();

  ctx.font = "11px Segoe UI, system-ui, sans-serif";
  ctx.textAlign = "left";
  ctx.fillStyle = thrColor;
  ctx.fillText("- - threshold (autotune)", margins.left + 8, margins.top + 14);
  if (basePts.length > 0) {
    ctx.fillStyle = prevColor;
    ctx.fillText("- - knockBaseNoise (было)", margins.left + 168, margins.top + 14);
    ctx.fillStyle = levelColor;
    ctx.fillText("● knock level", margins.left + 320, margins.top + 14);
  } else {
    ctx.fillStyle = levelColor;
    ctx.fillText("● knock level (прогон)", margins.left + 168, margins.top + 14);
  }

  if (cfgPts.length === 0 && runPts.length === 0 && prevPts.length === 0) {
    ctx.fillStyle = text;
    ctx.textAlign = "center";
    ctx.font = "13px Segoe UI, system-ui, sans-serif";
    ctx.fillText("Загрузите config и запустите прогон", width / 2, height / 2);
  } else if (cfgPts.length === 0) {
    ctx.fillStyle = text;
    ctx.textAlign = "center";
    ctx.font = "12px Segoe UI, system-ui, sans-serif";
    ctx.fillText("Кривая knockBaseNoise — загрузите config", width / 2, margins.top + 36);
  }
}
