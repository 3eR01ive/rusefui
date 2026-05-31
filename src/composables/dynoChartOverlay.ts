import type { DynoRunPoint } from "./dynoTypes";
import {
  canvasXToRpm,
  hpToCanvasY,
  interpolateDynoAtRpm,
  torqueToCanvasY,
  type DynoChartLayout,
  type DynoCrosshairSpec,
} from "./dynoChartLayout";

export interface DynoOverlayLabel {
  text: string;
  left: number;
  top: number;
  align: "left" | "center" | "right";
  color?: string;
  transform?: string;
}

export interface DynoCrosshairMarker {
  x: number;
  y: number;
  color: string;
  label: string;
  align: "left" | "right";
}

export interface DynoChartOverlay {
  labels: DynoOverlayLabel[];
}

function formatTick(v: number): string {
  const abs = Math.abs(v);
  if (abs >= 1000) return v.toFixed(0);
  if (abs >= 100) return v.toFixed(0);
  if (abs >= 10) return v.toFixed(1);
  if (abs >= 1) return v.toFixed(1);
  return v.toFixed(2);
}

function buildAxisLabels(
  width: number,
  height: number,
  layout: DynoChartLayout,
  options: { showPreviousLegend: boolean },
): DynoOverlayLabel[] {
  const labels: DynoOverlayLabel[] = [];
  const { plotLeft, plotRight, plotTop, plotBottom, plotW, plotH } = layout;
  const xSpan = Math.max(layout.xMax - layout.xMin, 1);
  const tqSpan = Math.max(layout.tqHi - layout.tqLo, 1e-9);
  const hpSpan = Math.max(layout.hpHi - layout.hpLo, 1e-9);

  const yTicks = 5;
  for (let i = 0; i <= yTicks; i += 1) {
    const frac = i / yTicks;
    const y = plotTop + frac * plotH;
    const tqVal = layout.tqLo + (1 - frac) * tqSpan;
    const hpVal = layout.hpLo + (1 - frac) * hpSpan;
    labels.push({
      text: formatTick(tqVal),
      left: plotLeft - 8,
      top: y,
      align: "right",
      transform: "translate(-100%, -50%)",
    });
    labels.push({
      text: formatTick(hpVal),
      left: plotRight + 8,
      top: y,
      align: "left",
      transform: "translate(0, -50%)",
      color: "var(--color-success-text, #2d6a4f)",
    });
  }

  const xTicks = 6;
  for (let i = 0; i <= xTicks; i += 1) {
    const frac = i / xTicks;
    const rpm = layout.xMin + frac * xSpan;
    labels.push({
      text: formatTick(rpm),
      left: plotLeft + frac * plotW,
      top: plotBottom + 6,
      align: "center",
      transform: "translate(-50%, 0)",
    });
  }

  labels.push({
    text: "RPM",
    left: plotLeft + plotW / 2,
    top: height - 8,
    align: "center",
    transform: "translate(-50%, -100%)",
  });
  labels.push({
    text: "Nm",
    left: 12,
    top: plotTop + plotH / 2,
    align: "center",
    transform: "translate(-50%, -50%)",
    color: "var(--color-accent, #3d7ea6)",
  });
  labels.push({
    text: "HP",
    left: width - 10,
    top: plotTop + plotH / 2,
    align: "center",
    transform: "translate(-50%, -50%)",
    color: "var(--color-success-text, #2d6a4f)",
  });

  labels.push({
    text: "● Nm",
    left: plotLeft + 8,
    top: plotTop + 6,
    align: "left",
    transform: "translate(0, 0)",
    color: "var(--color-accent, #3d7ea6)",
  });
  labels.push({
    text: "● HP",
    left: plotLeft + 48,
    top: plotTop + 6,
    align: "left",
    transform: "translate(0, 0)",
    color: "var(--color-success-text, #2d6a4f)",
  });
  if (options.showPreviousLegend) {
    labels.push({
      text: "- - пр.",
      left: plotLeft + 88,
      top: plotTop + 6,
      align: "left",
      transform: "translate(0, 0)",
      color: "var(--color-text-muted, #8a8278)",
    });
  }

  return labels;
}

export function buildDynoChartOverlay(
  width: number,
  height: number,
  layout: DynoChartLayout,
  options: { showPreviousLegend: boolean },
): DynoChartOverlay {
  if (!layout.hasData) return { labels: [] };
  return { labels: buildAxisLabels(width, height, layout, options) };
}

export function buildDynoCrosshairMarkers(
  layout: DynoChartLayout,
  points: DynoRunPoint[],
  crosshair: DynoCrosshairSpec | null,
): DynoCrosshairMarker[] {
  if (crosshair === null || !layout.hasData || points.length < 2) return [];

  const { plotLeft, plotRight, plotTop } = layout;
  const x = Math.min(plotRight, Math.max(plotLeft, crosshair.x));
  const rpm = canvasXToRpm(x, layout);
  if (rpm === null) return [];

  const sample = interpolateDynoAtRpm(points, rpm);
  if (!sample) return [];

  const tqY = torqueToCanvasY(sample.torqueNm, layout);
  const hpY = hpToCanvasY(sample.hp, layout);

  return [
    {
      x,
      y: plotTop + 10,
      color: "var(--color-accent, #3d7ea6)",
      label: `${Math.round(rpm)} rpm`,
      align: "left",
    },
    {
      x,
      y: tqY,
      color: "var(--color-accent, #3d7ea6)",
      label: `Nm ${formatTick(sample.torqueNm)}`,
      align: "left",
    },
    {
      x,
      y: hpY,
      color: "var(--color-success-text, #2d6a4f)",
      label: `HP ${formatTick(sample.hp)}`,
      align: "right",
    },
  ];
}

export function dynoCrosshairMarkerStyle(
  marker: DynoCrosshairMarker,
  chartWidth: number,
): Record<string, string> {
  const estWidth = marker.label.length * 6.5 + 12;
  const flip =
    marker.align === "left" && marker.x + 10 + estWidth > chartWidth - 4;
  const left = marker.align === "right" || flip ? marker.x - 10 : marker.x + 10;
  const transform =
    marker.align === "right" || flip
      ? "translate(-100%, -50%)"
      : "translateY(-50%)";
  return {
    left: `${left}px`,
    top: `${marker.y}px`,
    color: marker.color,
    borderColor: marker.color,
    transform,
  };
}

export function dynoLayoutSignature(layout: DynoChartLayout, width: number, height: number): string {
  if (!layout.hasData) return "empty";
  return [
    width,
    height,
    layout.xMin.toFixed(2),
    layout.xMax.toFixed(2),
    layout.tqLo.toFixed(2),
    layout.tqHi.toFixed(2),
    layout.hpLo.toFixed(2),
    layout.hpHi.toFixed(2),
  ].join("|");
}
