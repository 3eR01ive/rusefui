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

export interface KnockThresholdChartOptions {
  margins?: ChartMargins;
  previousRunLevels?: KnockRpmValuePoint[];
  liveRpm?: number | null;
  liveLevel?: number | null;
  /** knockBaseNoise до прогона (пунктир «было»). */
  baselineThreshold?: KnockRpmValuePoint[];
  thresholdGapDb?: number;
  recording?: boolean;
}

const FONT_AXIS = "10px Segoe UI, system-ui, sans-serif";
const FONT_LABEL = "11px Segoe UI, system-ui, sans-serif";
const FONT_BADGE = "10px ui-monospace, SFMono-Regular, Menlo, monospace";
const DEFAULT_MARGINS: ChartMargins = { top: 40, right: 18, bottom: 42, left: 56 };

interface ChartTheme {
  plotBg: string;
  grid: string;
  gridMajor: string;
  text: string;
  textSubtle: string;
  level: string;
  levelFill: string;
  threshold: string;
  thresholdGlow: string;
  baseline: string;
  prev: string;
  gapFill: string;
  live: string;
  legendBg: string;
  legendBorder: string;
  accentWarn: string;
}

function themeColors(): ChartTheme {
  const accent = cssVar("--color-accent", "#5b9cf5");
  const success = cssVar("--color-success", "#3ecf8e");
  return {
    plotBg: cssVar("--color-surface-2", "#1a1d24"),
    grid: cssVar("--color-border-subtle", "rgba(255,255,255,0.06)"),
    gridMajor: cssVar("--color-border", "rgba(255,255,255,0.12)"),
    text: cssVar("--color-text-muted", "#888"),
    textSubtle: cssVar("--color-text-subtle", "#666"),
    level: accent,
    levelFill: withAlpha(accent, 0.14),
    threshold: success,
    thresholdGlow: withAlpha(success, 0.35),
    baseline: cssVar("--color-text-muted", "#666"),
    prev: cssVar("--color-text-subtle", "#555"),
    gapFill: withAlpha(success, 0.22),
    live: cssVar("--color-accent", "#5b9cf5"),
    legendBg: withAlpha(cssVar("--color-bg-elevated", "#12151a"), 0.88),
    legendBorder: cssVar("--color-border", "rgba(255,255,255,0.1)"),
    accentWarn: cssVar("--color-warning", "#e6a23c"),
  };
}

function cssVar(name: string, fallback: string): string {
  return (
    getComputedStyle(document.documentElement).getPropertyValue(name).trim() ||
    fallback
  );
}

function withAlpha(color: string, alpha: number): string {
  if (color.startsWith("#") && color.length === 7) {
    const r = parseInt(color.slice(1, 3), 16);
    const g = parseInt(color.slice(3, 5), 16);
    const b = parseInt(color.slice(5, 7), 16);
    return `rgba(${r},${g},${b},${alpha})`;
  }
  if (color.startsWith("rgb")) return color;
  return `color-mix(in srgb, ${color} ${Math.round(alpha * 100)}%, transparent)`;
}

function formatTick(v: number): string {
  const abs = Math.abs(v);
  if (abs >= 100) return v.toFixed(0);
  if (abs >= 10) return v.toFixed(1);
  return v.toFixed(2);
}

function formatRpmTick(rpm: number): string {
  if (rpm >= 10_000) return `${(rpm / 1000).toFixed(0)}k`;
  if (rpm >= 1000) return `${(rpm / 1000).toFixed(1)}k`;
  return String(Math.round(rpm));
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

function sortByRpm(points: KnockRpmValuePoint[]): KnockRpmValuePoint[] {
  const valid = validRpmPoints(points);
  if (valid.length <= 1) return valid;
  for (let i = 1; i < valid.length; i += 1) {
    if (valid[i]!.rpm < valid[i - 1]!.rpm) {
      return [...valid].sort((a, b) => a.rpm - b.rpm);
    }
  }
  return valid;
}

function valueAtRpm(points: KnockRpmValuePoint[], rpm: number): number | null {
  const sorted = sortByRpm(points);
  if (sorted.length === 0) return null;
  for (const p of sorted) {
    if (Math.abs(p.rpm - rpm) < 0.5) return p.value;
  }
  if (rpm <= sorted[0]!.rpm) return sorted[0]!.value;
  if (rpm >= sorted[sorted.length - 1]!.rpm) return sorted[sorted.length - 1]!.value;
  for (let i = 1; i < sorted.length; i += 1) {
    const a = sorted[i - 1]!;
    const b = sorted[i]!;
    if (rpm >= a.rpm && rpm <= b.rpm) {
      const t = (rpm - a.rpm) / (b.rpm - a.rpm);
      return a.value + t * (b.value - a.value);
    }
  }
  return null;
}

function drawPlotFrame(
  ctx: CanvasRenderingContext2D,
  m: ChartMargins,
  plotW: number,
  plotH: number,
  colors: ChartTheme,
): void {
  const grad = ctx.createLinearGradient(0, m.top, 0, m.top + plotH);
  grad.addColorStop(0, withAlpha(colors.plotBg, 1));
  grad.addColorStop(1, withAlpha(colors.plotBg, 0.72));
  ctx.fillStyle = grad;
  ctx.fillRect(m.left, m.top, plotW, plotH);

  ctx.strokeStyle = colors.gridMajor;
  ctx.lineWidth = 1;
  ctx.strokeRect(m.left + 0.5, m.top + 0.5, plotW - 1, plotH - 1);
}

function drawGrid(
  ctx: CanvasRenderingContext2D,
  m: ChartMargins,
  plotW: number,
  plotH: number,
  xMin: number,
  xMax: number,
  yMin: number,
  yMax: number,
  colors: ChartTheme,
): void {
  ctx.save();
  ctx.beginPath();
  ctx.rect(m.left, m.top, plotW, plotH);
  ctx.clip();

  ctx.lineWidth = 1;
  for (let i = 1; i <= 4; i += 1) {
    const y = m.top + (plotH * i) / 5;
    ctx.strokeStyle = i === 2 || i === 4 ? colors.grid : colors.grid;
    ctx.setLineDash(i % 2 === 0 ? [] : [4, 6]);
    ctx.beginPath();
    ctx.moveTo(m.left, y);
    ctx.lineTo(m.left + plotW, y);
    ctx.stroke();
  }
  ctx.setLineDash([]);

  for (let i = 1; i <= 5; i += 1) {
    const x = m.left + (plotW * i) / 6;
    ctx.strokeStyle = colors.grid;
    ctx.beginPath();
    ctx.moveTo(x, m.top);
    ctx.lineTo(x, m.top + plotH);
    ctx.stroke();
  }
  ctx.restore();

  const toY = (v: number) => m.top + plotH - ((v - yMin) / (yMax - yMin)) * plotH;
  ctx.fillStyle = colors.text;
  ctx.font = FONT_AXIS;
  ctx.textAlign = "right";
  ctx.textBaseline = "middle";
  for (let i = 0; i <= 5; i += 1) {
    const v = yMin + ((yMax - yMin) * (5 - i)) / 5;
    ctx.fillText(formatTick(v), m.left - 8, toY(v));
  }

  const toX = (rpm: number) => m.left + ((rpm - xMin) / (xMax - xMin)) * plotW;
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  for (let i = 0; i <= 6; i += 1) {
    const rpm = xMin + ((xMax - xMin) * i) / 6;
    ctx.fillText(formatRpmTick(rpm), toX(rpm), m.top + plotH + 8);
  }

  ctx.textAlign = "center";
  ctx.fillText("RPM", m.left + plotW / 2, m.top + plotH + 24);
  ctx.save();
  ctx.translate(14, m.top + plotH / 2);
  ctx.rotate(-Math.PI / 2);
  ctx.textAlign = "center";
  ctx.fillText("dB", 0, 0);
  ctx.restore();
}

function drawAreaUnderCurve(
  ctx: CanvasRenderingContext2D,
  points: KnockRpmValuePoint[],
  toX: (rpm: number) => number,
  toY: (v: number) => number,
  yFloor: number,
  fill: string,
): void {
  const sorted = sortByRpm(points);
  if (sorted.length < 2) return;
  ctx.save();
  ctx.fillStyle = fill;
  ctx.beginPath();
  ctx.moveTo(toX(sorted[0]!.rpm), toY(sorted[0]!.value));
  for (let i = 1; i < sorted.length; i += 1) {
    ctx.lineTo(toX(sorted[i]!.rpm), toY(sorted[i]!.value));
  }
  ctx.lineTo(toX(sorted[sorted.length - 1]!.rpm), toY(yFloor));
  ctx.lineTo(toX(sorted[0]!.rpm), toY(yFloor));
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

function drawGapBands(
  ctx: CanvasRenderingContext2D,
  runPts: KnockRpmValuePoint[],
  thrPts: KnockRpmValuePoint[],
  toX: (rpm: number) => number,
  toY: (v: number) => number,
  fill: string,
): void {
  const sorted = sortByRpm(runPts);
  if (sorted.length === 0) return;
  ctx.save();
  ctx.fillStyle = fill;
  for (const p of sorted) {
    const thr = valueAtRpm(thrPts, p.rpm);
    if (thr == null || !Number.isFinite(thr)) continue;
    const y0 = toY(p.value);
    const y1 = toY(thr);
    const x = toX(p.rpm);
    const top = Math.min(y0, y1);
    const h = Math.abs(y1 - y0);
    if (h < 1) continue;
    ctx.fillRect(x - 3, top, 6, h);
  }
  ctx.restore();
}

function drawSeriesLine(
  ctx: CanvasRenderingContext2D,
  points: KnockRpmValuePoint[],
  toX: (rpm: number) => number,
  toY: (v: number) => number,
  color: string,
  style: { dashed?: boolean; opacity?: number; lineWidth?: number; glow?: string } = {},
): void {
  const sorted = sortByRpm(points);
  if (sorted.length === 0) return;

  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = style.lineWidth ?? 2.5;
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  ctx.globalAlpha = style.opacity ?? 1;
  if (style.dashed) ctx.setLineDash([7, 5]);

  if (sorted.length === 1) {
    const p = sorted[0]!;
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(toX(p.rpm), toY(p.value), 4, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
    return;
  }

  if (style.glow) {
    ctx.strokeStyle = style.glow;
    ctx.lineWidth = (style.lineWidth ?? 2.5) + 4;
    ctx.globalAlpha = 0.45;
    ctx.beginPath();
    ctx.moveTo(toX(sorted[0]!.rpm), toY(sorted[0]!.value));
    for (let i = 1; i < sorted.length; i += 1) {
      ctx.lineTo(toX(sorted[i]!.rpm), toY(sorted[i]!.value));
    }
    ctx.stroke();
    ctx.globalAlpha = style.opacity ?? 1;
    ctx.strokeStyle = color;
    ctx.lineWidth = style.lineWidth ?? 2.5;
  }

  ctx.beginPath();
  ctx.moveTo(toX(sorted[0]!.rpm), toY(sorted[0]!.value));
  for (let i = 1; i < sorted.length; i += 1) {
    ctx.lineTo(toX(sorted[i]!.rpm), toY(sorted[i]!.value));
  }
  ctx.stroke();
  ctx.restore();
}

function drawSeriesPoints(
  ctx: CanvasRenderingContext2D,
  points: KnockRpmValuePoint[],
  toX: (rpm: number) => number,
  toY: (v: number) => number,
  color: string,
  options: { radius?: number; ring?: boolean; empty?: boolean } = {},
): void {
  const sorted = sortByRpm(points);
  const r = options.radius ?? 4;
  ctx.save();
  for (const p of sorted) {
    const x = toX(p.rpm);
    const y = toY(p.value);
    if (options.empty) {
      ctx.strokeStyle = withAlpha(color, 0.55);
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.stroke();
      continue;
    }
    if (options.ring) {
      ctx.fillStyle = cssVar("--color-bg-elevated", "#12151a");
      ctx.strokeStyle = color;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(x, y, r + 1, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
    } else {
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
    }
  }
  ctx.restore();
}

function drawPendingBins(
  ctx: CanvasRenderingContext2D,
  allThr: KnockRpmValuePoint[],
  runPts: KnockRpmValuePoint[],
  toX: (rpm: number) => number,
  toY: (v: number) => number,
  yMin: number,
  color: string,
): void {
  const sampled = new Set(runPts.map((p) => Math.round(p.rpm)));
  const pending: KnockRpmValuePoint[] = [];
  for (const p of sortByRpm(allThr)) {
    if (!sampled.has(Math.round(p.rpm))) {
      pending.push({ rpm: p.rpm, value: yMin });
    }
  }
  if (pending.length === 0) return;
  ctx.save();
  ctx.strokeStyle = withAlpha(color, 0.35);
  ctx.lineWidth = 1;
  for (const p of pending) {
    const x = toX(p.rpm);
    ctx.setLineDash([2, 4]);
    ctx.beginPath();
    ctx.moveTo(x, toY(p.value));
    ctx.lineTo(x, toY(p.value) - 10);
    ctx.stroke();
  }
  ctx.setLineDash([]);
  ctx.restore();
}

function drawLiveCursor(
  ctx: CanvasRenderingContext2D,
  m: ChartMargins,
  plotH: number,
  toX: (rpm: number) => number,
  toY: (v: number) => number,
  rpm: number,
  level: number,
  colors: ChartTheme,
): void {
  const x = toX(rpm);
  const y = toY(level);
  ctx.save();
  ctx.strokeStyle = withAlpha(colors.live, 0.45);
  ctx.lineWidth = 1;
  ctx.setLineDash([4, 4]);
  ctx.beginPath();
  ctx.moveTo(x, m.top);
  ctx.lineTo(x, m.top + plotH);
  ctx.stroke();
  ctx.setLineDash([]);

  ctx.fillStyle = colors.live;
  ctx.strokeStyle = cssVar("--color-bg-elevated", "#12151a");
  ctx.lineWidth = 2;
  ctx.beginPath();
  ctx.arc(x, y, 5.5, 0, Math.PI * 2);
  ctx.fill();
  ctx.stroke();

  ctx.font = FONT_BADGE;
  ctx.textAlign = x > m.left + 80 ? "right" : "left";
  ctx.textBaseline = "bottom";
  ctx.fillStyle = withAlpha(colors.live, 0.95);
  const label = `${Math.round(rpm)} rpm · ${level.toFixed(1)} dB`;
  ctx.fillText(label, x + (x > m.left + 80 ? -8 : 8), y - 10);
  ctx.restore();
}

function drawLegend(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  items: { label: string; color: string; dashed?: boolean; swatch?: "band" }[],
  colors: ChartTheme,
): void {
  const lineH = 15;
  const padX = 10;
  const padY = 8;
  const w = 168;
  const h = padY * 2 + items.length * lineH;

  ctx.save();
  ctx.fillStyle = colors.legendBg;
  ctx.strokeStyle = colors.legendBorder;
  ctx.lineWidth = 1;
  roundRect(ctx, x, y, w, h, 6);
  ctx.fill();
  ctx.stroke();

  ctx.font = FONT_LABEL;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  items.forEach((item, i) => {
    const cy = y + padY + lineH * i + lineH / 2;
    const sx = x + padX;
    if (item.swatch === "band") {
      ctx.fillStyle = item.color;
      ctx.fillRect(sx, cy - 5, 14, 10);
    } else if (item.dashed) {
      ctx.strokeStyle = item.color;
      ctx.lineWidth = 2;
      ctx.setLineDash([5, 4]);
      ctx.beginPath();
      ctx.moveTo(sx, cy);
      ctx.lineTo(sx + 16, cy);
      ctx.stroke();
      ctx.setLineDash([]);
    } else {
      ctx.fillStyle = item.color;
      ctx.beginPath();
      ctx.arc(sx + 8, cy, 4, 0, Math.PI * 2);
      ctx.fill();
      ctx.strokeStyle = item.color;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(sx + 14, cy);
      ctx.lineTo(sx + 22, cy);
      ctx.stroke();
    }
    ctx.fillStyle = colors.text;
    ctx.fillText(item.label, sx + 28, cy);
  });
  ctx.restore();
}

function drawCoverageBadge(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  sampled: number,
  total: number,
  recording: boolean,
  gapDb: number | undefined,
  colors: ChartTheme,
): void {
  const label =
    total > 0
      ? recording
        ? `Бины ${sampled}/${total}`
        : `${sampled} бинов`
      : "";
  if (!label) return;

  ctx.save();
  ctx.font = FONT_BADGE;
  const gapLabel =
    gapDb != null && Number.isFinite(gapDb) ? `  ·  Δ ${gapDb.toFixed(1)} dB` : "";
  const text = label + gapLabel;
  const metrics = ctx.measureText(text);
  const padX = 8;
  const w = metrics.width + padX * 2;
  const h = 18;

  ctx.fillStyle = colors.legendBg;
  ctx.strokeStyle = colors.legendBorder;
  roundRect(ctx, x - w, y, w, h, 4);
  ctx.fill();
  ctx.stroke();

  ctx.fillStyle = recording ? colors.accentWarn : colors.textSubtle;
  ctx.textAlign = "right";
  ctx.textBaseline = "middle";
  ctx.fillText(text, x - padX, y + h / 2);
  ctx.restore();
}

function roundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.lineTo(x + w - r, y);
  ctx.quadraticCurveTo(x + w, y, x + w, y + r);
  ctx.lineTo(x + w, y + h - r);
  ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
  ctx.lineTo(x + r, y + h);
  ctx.quadraticCurveTo(x, y + h, x, y + h - r);
  ctx.lineTo(x, y + r);
  ctx.quadraticCurveTo(x, y, x + r, y);
  ctx.closePath();
}

function drawEmptyHint(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  title: string,
  subtitle: string,
  colors: ChartTheme,
): void {
  ctx.font = "13px Segoe UI, system-ui, sans-serif";
  ctx.fillStyle = colors.text;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(title, width / 2, height / 2 - 8);
  ctx.font = FONT_AXIS;
  ctx.fillStyle = colors.textSubtle;
  ctx.fillText(subtitle, width / 2, height / 2 + 12);
}

/**
 * Threshold autotune: RPM × dB.
 * — заливка под knock level, полосы зазора до порога, маркеры бинов
 * — пунктир: knockBaseNoise (было), сплошная: порог autotune, ● шум прогона
 */
export function drawKnockThresholdChart(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  threshold: KnockRpmValuePoint[],
  runLevels: KnockRpmValuePoint[],
  options: KnockThresholdChartOptions = {},
): void {
  ctx.clearRect(0, 0, width, height);

  const m = options.margins ?? DEFAULT_MARGINS;
  const plotW = width - m.left - m.right;
  const plotH = height - m.top - m.bottom;
  if (plotW <= 0 || plotH <= 0) return;

  const colors = themeColors();
  const thrPts = sortByRpm(threshold);
  const basePts = sortByRpm(options.baselineThreshold ?? []);
  const runPts = sortByRpm(runLevels);
  const prevPts = sortByRpm(options.previousRunLevels ?? []);
  const all = [...thrPts, ...basePts, ...runPts, ...prevPts];

  if (all.length === 0) {
    drawEmptyHint(
      ctx,
      width,
      height,
      "Threshold autotune",
      "Загрузите config и нажмите «Старт Threshold Autotune»",
      colors,
    );
    return;
  }

  let yMin = -60;
  let yMax = 10;
  {
    let min = Infinity;
    let max = -Infinity;
    for (const p of all) {
      min = Math.min(min, p.value);
      max = Math.max(max, p.value);
    }
    const liveLevel = options.liveLevel;
    if (liveLevel != null && Number.isFinite(liveLevel)) {
      min = Math.min(min, liveLevel);
      max = Math.max(max, liveLevel);
    }
    ({ min: yMin, max: yMax } = boundsWithPadding(min, max, 0.12));
  }

  let xMin = 0;
  let xMax = 7000;
  {
    xMin = Math.min(...all.map((p) => p.rpm));
    xMax = Math.max(...all.map((p) => p.rpm));
  }
  const liveRpm = options.liveRpm;
  if (liveRpm != null && Number.isFinite(liveRpm)) {
    xMin = Math.min(xMin, liveRpm);
    xMax = Math.max(xMax, liveRpm);
  }
  if (xMax - xMin < 400) xMax = xMin + 400;
  const xPad = (xMax - xMin) * 0.04;
  xMin -= xPad;
  xMax += xPad;

  const toX = (rpm: number) => m.left + ((rpm - xMin) / (xMax - xMin)) * plotW;
  const toY = (v: number) => m.top + plotH - ((v - yMin) / (yMax - yMin)) * plotH;

  drawPlotFrame(ctx, m, plotW, plotH, colors);
  drawGrid(ctx, m, plotW, plotH, xMin, xMax, yMin, yMax, colors);

  ctx.save();
  ctx.beginPath();
  ctx.rect(m.left, m.top, plotW, plotH);
  ctx.clip();

  if (options.recording && thrPts.length > 0) {
    drawPendingBins(ctx, thrPts, runPts, toX, toY, yMin, colors.textSubtle);
  }

  drawAreaUnderCurve(ctx, runPts, toX, toY, yMin, colors.levelFill);

  if (runPts.length > 0 && thrPts.length > 0) {
    drawGapBands(ctx, runPts, thrPts, toX, toY, colors.gapFill);
  }

  drawSeriesLine(ctx, prevPts, toX, toY, colors.prev, {
    dashed: true,
    opacity: 0.5,
    lineWidth: 1.5,
  });
  drawSeriesLine(ctx, basePts, toX, toY, colors.baseline, {
    dashed: true,
    opacity: 0.55,
    lineWidth: 1.5,
  });
  drawSeriesLine(ctx, runPts, toX, toY, colors.level, { lineWidth: 2.5 });
  drawSeriesLine(ctx, thrPts, toX, toY, colors.threshold, {
    lineWidth: 2.5,
    glow: colors.thresholdGlow,
  });

  drawSeriesPoints(ctx, runPts, toX, toY, colors.level, { radius: 3.5, ring: true });
  drawSeriesPoints(ctx, thrPts, toX, toY, colors.threshold, { radius: 3, ring: true });

  if (
    liveRpm != null &&
    options.liveLevel != null &&
    Number.isFinite(liveRpm) &&
    Number.isFinite(options.liveLevel)
  ) {
    drawLiveCursor(ctx, m, plotH, toX, toY, liveRpm, options.liveLevel, colors);
  }

  ctx.restore();

  const legendItems: { label: string; color: string; dashed?: boolean; swatch?: "band" }[] =
    [];
  if (runPts.length > 0) {
    legendItems.push({ label: "Шум (прогон)", color: colors.level });
  }
  if (thrPts.length > 0) {
    legendItems.push({ label: "Порог autotune", color: colors.threshold });
  }
  if (runPts.length > 0 && thrPts.length > 0) {
    legendItems.push({ label: "Зазор Δ", color: colors.gapFill, swatch: "band" });
  }
  if (basePts.length > 0) {
    legendItems.push({ label: "knockBaseNoise (было)", color: colors.baseline, dashed: true });
  }
  if (prevPts.length > 0) {
    legendItems.push({ label: "Прошлый прогон", color: colors.prev, dashed: true });
  }
  if (legendItems.length > 0) {
    drawLegend(ctx, m.left + 8, m.top + 6, legendItems, colors);
  }

  const totalBins = thrPts.length > 0 ? thrPts.length : basePts.length;
  drawCoverageBadge(
    ctx,
    m.left + plotW - 4,
    m.top + 8,
    runPts.length,
    totalBins,
    Boolean(options.recording),
    options.thresholdGapDb,
    colors,
  );

  if (thrPts.length === 0 && runPts.length > 0) {
    ctx.font = FONT_AXIS;
    ctx.fillStyle = colors.textSubtle;
    ctx.textAlign = "center";
    ctx.fillText("Порог появится после первых RPM-бинов", width / 2, m.top + plotH - 10);
  }
}
