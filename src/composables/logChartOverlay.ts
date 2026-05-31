import {
  formatTick,
  interpolateSeriesAtTime,
  plotXToTime,
  type LogCrosshairSpec,
  type LogGraphPanelSpec,
} from "./drawTimeSeriesChart";
import {
  LOG_CORNER_LINE_H,
  computeLogPanelLayouts,
  valueToCanvasY,
} from "./logChartLayout";

export interface LogOverlayLabel {
  text: string;
  left: number;
  top: number;
  color: string;
  fontSize: string;
  fontWeight: string;
  textAlign: "left" | "right";
}

export interface LogCrosshairMarker {
  x: number;
  y: number;
  color: string;
  label: string;
}

export interface LogChartOverlay {
  crosshairX: number | null;
  labels: LogOverlayLabel[];
  markers: LogCrosshairMarker[];
}

export function buildLogChartOverlay(
  width: number,
  height: number,
  panels: LogGraphPanelSpec[],
  tMin: number,
  tMax: number,
  crosshair: LogCrosshairSpec | null,
): LogChartOverlay {
  const { layouts, sharedMargins } = computeLogPanelLayouts(width, height, panels);
  const crosshairT =
    crosshair !== null ? plotXToTime(crosshair.x, width, sharedMargins, tMin, tMax) : null;

  const labels: LogOverlayLabel[] = [];
  const markers: LogCrosshairMarker[] = [];

  let crosshairX: number | null = null;
  if (crosshair !== null) {
    const plotLeft = sharedMargins.left;
    const plotRight = width - sharedMargins.right;
    if (crosshair.x >= plotLeft && crosshair.x <= plotRight) {
      crosshairX = crosshair.x;
    }
  }

  for (const layout of layouts) {
    labels.push({
      text: layout.title,
      left: width - 10,
      top: layout.y0 + 4,
      color: "var(--color-text-subtle)",
      fontSize: "9px",
      fontWeight: "600",
      textAlign: "right",
    });

    let yMax = layout.plotTop + 2;
    for (const tr of layout.traces) {
      const unit = tr.units ? ` (${tr.units})` : "";
      labels.push({
        text: `Max = ${formatTick(tr.vMax)}${unit}`,
        left: layout.plotLeft + 4,
        top: yMax,
        color: tr.color,
        fontSize: "10px",
        fontWeight: "400",
        textAlign: "left",
      });
      yMax += LOG_CORNER_LINE_H;
    }

    let yMin = layout.plotBottom - 2 - layout.traces.length * LOG_CORNER_LINE_H;
    for (const tr of layout.traces) {
      const unit = tr.units ? ` (${tr.units})` : "";
      labels.push({
        text: `Min = ${formatTick(tr.vMin)}${unit}`,
        left: layout.plotLeft + 4,
        top: yMin,
        color: tr.color,
        fontSize: "10px",
        fontWeight: "400",
        textAlign: "left",
      });
      yMin += LOG_CORNER_LINE_H;
    }

    const tRight = tMax;
    let yCur =
      layout.plotTop + layout.plotH * 0.5 - ((layout.traces.length - 1) * LOG_CORNER_LINE_H) / 2;
    for (const tr of layout.traces) {
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
        labels.push({
          text: `${formatTick(val)}${unit}`,
          left: layout.plotRight - 4,
          top: yCur,
          color: tr.color,
          fontSize: "11px",
          fontWeight: "600",
          textAlign: "right",
        });
      }
      yCur += LOG_CORNER_LINE_H;
    }

    if (crosshairT !== null && crosshairX !== null) {
      for (const tr of layout.traces) {
        const v = interpolateSeriesAtTime(tr.series.points, crosshairT);
        if (v === null) continue;
        const cy = valueToCanvasY(v, tr.vMin, tr.vMax, layout);
        if (!Number.isFinite(cy)) continue;
        const unit = tr.units ? ` ${tr.units}` : "";
        markers.push({
          x: crosshairX,
          y: cy,
          color: tr.color,
          label: `${tr.name} ${formatTick(v)}${unit}`,
        });
      }
    }
  }

  return { crosshairX, labels, markers };
}

export function labelStyle(lb: LogOverlayLabel): Record<string, string> {
  const transforms: string[] = [];
  if (lb.textAlign === "right") transforms.push("translateX(-100%)");
  if (lb.fontWeight === "600" && lb.fontSize === "11px") transforms.push("translateY(-50%)");
  return {
    left: `${lb.left}px`,
    top: `${lb.top}px`,
    color: lb.color,
    fontSize: lb.fontSize,
    fontWeight: lb.fontWeight,
    textAlign: lb.textAlign,
    ...(transforms.length > 0 ? { transform: transforms.join(" ") } : {}),
  };
}

export function markerLabelStyle(
  marker: LogCrosshairMarker,
  chartWidth: number,
): Record<string, string> {
  const estWidth = marker.label.length * 6.5 + 12;
  const flip = marker.x + 8 + estWidth > chartWidth - 4;
  return {
    left: flip ? `${marker.x - 8}px` : `${marker.x + 8}px`,
    top: `${marker.y}px`,
    color: marker.color,
    borderColor: marker.color,
    transform: flip ? "translate(-100%, -50%)" : "translateY(-50%)",
  };
}
