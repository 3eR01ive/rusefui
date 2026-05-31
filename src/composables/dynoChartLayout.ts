import type { DynoRunPoint } from "./dynoTypes";

export interface DynoChartMargins {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export const DEFAULT_DYNO_MARGINS: DynoChartMargins = {
  top: 20,
  right: 52,
  bottom: 36,
  left: 52,
};

export interface DynoChartLayout {
  margins: DynoChartMargins;
  plotLeft: number;
  plotRight: number;
  plotTop: number;
  plotBottom: number;
  plotW: number;
  plotH: number;
  xMin: number;
  xMax: number;
  tqLo: number;
  tqHi: number;
  hpLo: number;
  hpHi: number;
  hasData: boolean;
}

/** Фиксированные диапазоны осей из настроек dyno. */
export interface DynoAxisRange {
  rpmMin: number;
  rpmMax: number;
  nmMin: number;
  nmMax: number;
  hpMin: number;
  hpMax: number;
}

export const DEFAULT_DYNO_AXIS: DynoAxisRange = {
  rpmMin: 0,
  rpmMax: 8000,
  nmMin: 0,
  nmMax: 1000,
  hpMin: 0,
  hpMax: 1000,
};

export function normalizeDynoAxisRange(
  raw: Partial<DynoAxisRange>,
  soft = false,
): DynoAxisRange {
  let rpmMin = Math.max(0, Math.round(Number(raw.rpmMin ?? DEFAULT_DYNO_AXIS.rpmMin)));
  let rpmMax = Math.max(0, Math.round(Number(raw.rpmMax ?? DEFAULT_DYNO_AXIS.rpmMax)));
  let nmMin = Math.max(0, Math.round(Number(raw.nmMin ?? DEFAULT_DYNO_AXIS.nmMin)));
  let nmMax = Math.max(0, Math.round(Number(raw.nmMax ?? DEFAULT_DYNO_AXIS.nmMax)));
  let hpMin = Math.max(0, Math.round(Number(raw.hpMin ?? DEFAULT_DYNO_AXIS.hpMin)));
  let hpMax = Math.max(0, Math.round(Number(raw.hpMax ?? DEFAULT_DYNO_AXIS.hpMax)));

  if (rpmMax <= rpmMin) {
    if (soft) rpmMax = rpmMin + 500;
    else {
      rpmMin = DEFAULT_DYNO_AXIS.rpmMin;
      rpmMax = DEFAULT_DYNO_AXIS.rpmMax;
    }
  }
  if (nmMax <= nmMin) {
    if (soft) nmMax = nmMin + 100;
    else {
      nmMin = DEFAULT_DYNO_AXIS.nmMin;
      nmMax = DEFAULT_DYNO_AXIS.nmMax;
    }
  }
  if (hpMax <= hpMin) {
    if (soft) hpMax = hpMin + 100;
    else {
      hpMin = DEFAULT_DYNO_AXIS.hpMin;
      hpMax = DEFAULT_DYNO_AXIS.hpMax;
    }
  }

  return { rpmMin, rpmMax, nmMin, nmMax, hpMin, hpMax };
}

/** Layout графика только из настроек осей (без подстройки под данные). */
export function computeDynoFixedLayout(
  width: number,
  height: number,
  axes: DynoAxisRange,
  margins: DynoChartMargins = DEFAULT_DYNO_MARGINS,
): DynoChartLayout {
  const plotW = width - margins.left - margins.right;
  const plotH = height - margins.top - margins.bottom;
  const normalized = normalizeDynoAxisRange(axes);
  return {
    margins,
    plotLeft: margins.left,
    plotRight: width - margins.right,
    plotTop: margins.top,
    plotBottom: margins.top + Math.max(plotH, 0),
    plotW: Math.max(plotW, 0),
    plotH: Math.max(plotH, 0),
    xMin: normalized.rpmMin,
    xMax: normalized.rpmMax,
    tqLo: normalized.nmMin,
    tqHi: normalized.nmMax,
    hpLo: normalized.hpMin,
    hpHi: normalized.hpMax,
    hasData: true,
  };
}

/** Обновить только pixel-геометрию plot; оси RPM/Nm/HP не трогаем. */
export function refreshDynoLayoutPlotSize(
  layout: DynoChartLayout,
  width: number,
  height: number,
): DynoChartLayout {
  const plotW = width - layout.margins.left - layout.margins.right;
  const plotH = height - layout.margins.top - layout.margins.bottom;
  return {
    ...layout,
    plotLeft: layout.margins.left,
    plotRight: width - layout.margins.right,
    plotTop: layout.margins.top,
    plotBottom: layout.margins.top + Math.max(plotH, 0),
    plotW: Math.max(plotW, 0),
    plotH: Math.max(plotH, 0),
  };
}

export interface DynoCrosshairSpec {
  x: number;
}

export function rpmToCanvasX(rpm: number, layout: DynoChartLayout): number {
  const span = Math.max(layout.xMax - layout.xMin, 1);
  return layout.plotLeft + ((rpm - layout.xMin) / span) * layout.plotW;
}

export function torqueToCanvasY(nm: number, layout: DynoChartLayout): number {
  const span = Math.max(layout.tqHi - layout.tqLo, 1e-9);
  return layout.plotTop + layout.plotH - ((nm - layout.tqLo) / span) * layout.plotH;
}

export function hpToCanvasY(hp: number, layout: DynoChartLayout): number {
  const span = Math.max(layout.hpHi - layout.hpLo, 1e-9);
  return layout.plotTop + layout.plotH - ((hp - layout.hpLo) / span) * layout.plotH;
}

export function canvasXToRpm(x: number, layout: DynoChartLayout): number | null {
  if (x < layout.plotLeft || x > layout.plotRight) return null;
  const span = Math.max(layout.xMax - layout.xMin, 1);
  return layout.xMin + ((x - layout.plotLeft) / layout.plotW) * span;
}

/** Линейная интерполяция Nm/HP по RPM вдоль текущего прогона. */
export function interpolateDynoAtRpm(
  points: DynoRunPoint[],
  rpm: number,
): { torqueNm: number; hp: number } | null {
  const sorted = points
    .filter(
      (p) =>
        Number.isFinite(p.rpm) &&
        Number.isFinite(p.torqueNm) &&
        Number.isFinite(p.hp),
    )
    .sort((a, b) => a.rpm - b.rpm);
  if (sorted.length === 0) return null;
  if (rpm <= sorted[0]!.rpm) {
    return { torqueNm: sorted[0]!.torqueNm, hp: sorted[0]!.hp };
  }
  const last = sorted[sorted.length - 1]!;
  if (rpm >= last.rpm) {
    return { torqueNm: last.torqueNm, hp: last.hp };
  }
  for (let i = 1; i < sorted.length; i += 1) {
    const p1 = sorted[i]!;
    if (p1.rpm >= rpm) {
      const p0 = sorted[i - 1]!;
      const dr = p1.rpm - p0.rpm;
      if (dr < 1e-9) return { torqueNm: p1.torqueNm, hp: p1.hp };
      const f = (rpm - p0.rpm) / dr;
      return {
        torqueNm: p0.torqueNm + (p1.torqueNm - p0.torqueNm) * f,
        hp: p0.hp + (p1.hp - p0.hp) * f,
      };
    }
  }
  return { torqueNm: last.torqueNm, hp: last.hp };
}
