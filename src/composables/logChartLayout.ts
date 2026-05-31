import type { ChartMargins } from "./drawTimeSeriesChart";
import { logPanelMargins } from "./drawTimeSeriesChart";
import type { LogGraphPanelSpec, LogTraceSpec } from "./drawTimeSeriesChart";

export const LOG_PANEL_GAP = 2;
export const LOG_OUTER_TOP = 1;
export const LOG_OUTER_BOTTOM = 2;
export const LOG_CORNER_LINE_H = 10;

export interface LogPanelLayout {
  y0: number;
  panelH: number;
  margins: ChartMargins;
  plotLeft: number;
  plotRight: number;
  plotTop: number;
  plotBottom: number;
  plotW: number;
  plotH: number;
  title: string;
  traces: LogTraceSpec[];
}

export function maxTraceCount(panels: LogGraphPanelSpec[]): number {
  return panels.reduce((m, p) => Math.max(m, p.traces.length), 0);
}

export function computeLogPanelLayouts(
  width: number,
  height: number,
  panels: LogGraphPanelSpec[],
): { layouts: LogPanelLayout[]; sharedMargins: ChartMargins } {
  const sharedMargins = logPanelMargins(maxTraceCount(panels));
  const n = Math.max(1, panels.length);
  const usable = height - LOG_OUTER_TOP - LOG_OUTER_BOTTOM;
  const panelH = (usable - LOG_PANEL_GAP * Math.max(0, n - 1)) / n;

  const layouts = panels.map((panel, i) => {
    const y0 = LOG_OUTER_TOP + i * (panelH + LOG_PANEL_GAP);
    const margins = logPanelMargins(panel.traces.length);
    const plotW = width - margins.left - margins.right;
    const plotH = panelH - margins.top - margins.bottom;
    return {
      y0,
      panelH,
      margins,
      plotLeft: margins.left,
      plotRight: width - margins.right,
      plotTop: y0 + margins.top,
      plotBottom: y0 + margins.top + plotH,
      plotW,
      plotH,
      title: panel.title,
      traces: panel.traces,
    };
  });

  return { layouts, sharedMargins };
}

export function timeToCanvasX(
  t: number,
  tMin: number,
  tMax: number,
  layout: LogPanelLayout,
): number {
  const tSpan = Math.max(tMax - tMin, 0.001);
  return layout.plotLeft + ((t - tMin) / tSpan) * layout.plotW;
}

export function valueToCanvasY(
  v: number,
  vMin: number,
  vMax: number,
  layout: LogPanelLayout,
): number {
  const vSpan = Math.max(vMax - vMin, 1e-9);
  return layout.plotTop + layout.plotH - ((v - vMin) / vSpan) * layout.plotH;
}

/** Canvas Y (вниз) → data Y (вверх) для webgl-plot. */
export function canvasToDataY(canvasY: number, height: number): number {
  return height - canvasY;
}

export function canvasToDataPoint(
  canvasX: number,
  canvasY: number,
  height: number,
): [number, number] {
  return [canvasX, canvasToDataY(canvasY, height)];
}
