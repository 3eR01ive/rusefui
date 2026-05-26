export interface DynoRunPoint {
  rpm: number;
  torqueNm: number;
  hp: number;
}

export interface ChartMargins {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

const FONT_AXIS = "10px Segoe UI, system-ui, sans-serif";
const DEFAULT_MARGINS: ChartMargins = { top: 20, right: 52, bottom: 36, left: 52 };

function cssVar(name: string, fallback: string): string {
  return (
    getComputedStyle(document.documentElement).getPropertyValue(name).trim() ||
    fallback
  );
}

function formatTick(v: number): string {
  const abs = Math.abs(v);
  if (abs >= 1000) return v.toFixed(0);
  if (abs >= 100) return v.toFixed(0);
  if (abs >= 10) return v.toFixed(1);
  if (abs >= 1) return v.toFixed(1);
  return v.toFixed(2);
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

function drawSeries(
  ctx: CanvasRenderingContext2D,
  points: DynoRunPoint[],
  toX: (rpm: number) => number,
  toY: (v: number) => number,
  pickY: (p: DynoRunPoint) => number,
  color: string,
): void {
  if (points.length < 2) return;
  ctx.strokeStyle = color;
  ctx.lineWidth = 2;
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(toX(points[0]!.rpm), toY(pickY(points[0]!)));
  for (let i = 1; i < points.length; i += 1) {
    ctx.lineTo(toX(points[i]!.rpm), toY(pickY(points[i]!)));
  }
  ctx.stroke();
}

/** Кривая dyno: RPM по X, крутящий момент (слева) и мощность (справа). */
export function drawDynoChart(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  points: DynoRunPoint[],
  margins: ChartMargins = DEFAULT_MARGINS,
): void {
  ctx.clearRect(0, 0, width, height);

  const plotW = width - margins.left - margins.right;
  const plotH = height - margins.top - margins.bottom;
  if (plotW <= 0 || plotH <= 0) return;

  const valid = points.filter(
    (p) =>
      Number.isFinite(p.rpm) &&
      Number.isFinite(p.torqueNm) &&
      Number.isFinite(p.hp),
  );

  if (valid.length === 0) {
    ctx.font = FONT_AXIS;
    ctx.fillStyle = cssVar("--color-text-subtle", "#9c948a");
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText("Нажмите Start и выполните разгон", width / 2, height / 2);
    return;
  }

  const sorted = [...valid].sort((a, b) => a.rpm - b.rpm);
  const rpmMinRaw = sorted[0]!.rpm;
  const rpmMaxRaw = sorted[sorted.length - 1]!.rpm;
  let tqMin = sorted[0]!.torqueNm;
  let tqMax = sorted[0]!.torqueNm;
  let hpMin = sorted[0]!.hp;
  let hpMax = sorted[0]!.hp;
  for (const p of sorted) {
    tqMin = Math.min(tqMin, p.torqueNm);
    tqMax = Math.max(tqMax, p.torqueNm);
    hpMin = Math.min(hpMin, p.hp);
    hpMax = Math.max(hpMax, p.hp);
  }

  const { min: xMin, max: xMax } = boundsWithPadding(rpmMinRaw, rpmMaxRaw);
  const { min: tqLo, max: tqHi } = boundsWithPadding(tqMin, tqMax);
  const { min: hpLo, max: hpHi } = boundsWithPadding(hpMin, hpMax);
  const xSpan = Math.max(xMax - xMin, 1);
  const tqSpan = Math.max(tqHi - tqLo, 1e-9);
  const hpSpan = Math.max(hpHi - hpLo, 1e-9);

  const toX = (rpm: number) => margins.left + ((rpm - xMin) / xSpan) * plotW;
  const toTqY = (v: number) => margins.top + plotH - ((v - tqLo) / tqSpan) * plotH;
  const toHpY = (v: number) => margins.top + plotH - ((v - hpLo) / hpSpan) * plotH;

  ctx.fillStyle = cssVar("--color-bg-elevated", "#fff");
  ctx.fillRect(margins.left, margins.top, plotW, plotH);

  ctx.strokeStyle = cssVar("--color-border", "#e0d9ce");
  ctx.lineWidth = 1;
  ctx.font = FONT_AXIS;
  ctx.fillStyle = cssVar("--color-text-subtle", "#9c948a");

  const yTicks = 5;
  ctx.textAlign = "right";
  ctx.textBaseline = "middle";
  for (let i = 0; i <= yTicks; i += 1) {
    const frac = i / yTicks;
    const v = tqLo + (1 - frac) * tqSpan;
    const y = margins.top + frac * plotH;
    ctx.beginPath();
    ctx.moveTo(margins.left, y);
    ctx.lineTo(margins.left + plotW, y);
    ctx.stroke();
    ctx.fillText(formatTick(v), margins.left - 6, y);
  }

  ctx.textAlign = "left";
  for (let i = 0; i <= yTicks; i += 1) {
    const frac = i / yTicks;
    const v = hpLo + (1 - frac) * hpSpan;
    const y = margins.top + frac * plotH;
    ctx.fillText(formatTick(v), margins.left + plotW + 6, y);
  }

  const xTicks = 6;
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  for (let i = 0; i <= xTicks; i += 1) {
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

  const torqueColor = cssVar("--color-accent", "#3d7ea6");
  const hpColor = cssVar("--color-success-text", "#2d6a4f");

  ctx.save();
  ctx.beginPath();
  ctx.rect(margins.left, margins.top, plotW, plotH);
  ctx.clip();

  drawSeries(ctx, sorted, toX, toTqY, (p) => p.torqueNm, torqueColor);
  drawSeries(ctx, sorted, toX, toHpY, (p) => p.hp, hpColor);

  ctx.restore();

  ctx.font = "11px Segoe UI, system-ui, sans-serif";
  ctx.textAlign = "left";
  ctx.textBaseline = "top";
  ctx.fillStyle = torqueColor;
  ctx.fillText("● Nm", margins.left + 8, margins.top + 6);
  ctx.fillStyle = hpColor;
  ctx.fillText("● HP", margins.left + 48, margins.top + 6);

  ctx.fillStyle = cssVar("--color-text-subtle", "#9c948a");
  ctx.textAlign = "center";
  ctx.fillText("RPM", margins.left + plotW / 2, height - 8);
  ctx.save();
  ctx.translate(12, margins.top + plotH / 2);
  ctx.rotate(-Math.PI / 2);
  ctx.textAlign = "center";
  ctx.fillText("Nm", 0, 0);
  ctx.restore();
  ctx.save();
  ctx.translate(width - 10, margins.top + plotH / 2);
  ctx.rotate(Math.PI / 2);
  ctx.textAlign = "center";
  ctx.fillText("HP", 0, 0);
  ctx.restore();
}
