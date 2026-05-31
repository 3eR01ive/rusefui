/** WebGL-отрисовка log-графика (webgl-plot): сетка, кривые, кроссхайр. */

import {
  clearCanvas,
  handleCanvasResize,
  WebglLinePlot,
  WebglPlot,
  type DataBounds,
  type LineConfig,
} from "webgl-plot";
import {
  type LogCrosshairSpec,
  type LogGraphPanelSpec,
  type LogTraceSpec,
} from "./drawTimeSeriesChart";
import {
  canvasToDataPoint,
  computeLogPanelLayouts,
  timeToCanvasX,
  valueToCanvasY,
  type LogPanelLayout,
} from "./logChartLayout";

const MAX_LINES = 512;
const GRID_LINES = 4;
const DASH_ON = 5;
const DASH_OFF = 4;

type Rgba = [number, number, number, number];

type ThemeColors = {
  bgElevated: string;
  border: string;
  borderStrong: string;
};

function readTheme(el: HTMLElement | null): ThemeColors {
  const root = el ?? document.documentElement;
  const s = getComputedStyle(root);
  const v = (name: string, fallback: string) =>
    s.getPropertyValue(name).trim() || fallback;
  return {
    bgElevated: v("--color-bg-elevated", "#ffffff"),
    border: v("--color-border", "#e0d9ce"),
    borderStrong: v("--color-border-strong", "#cfc6b8"),
  };
}

let colorProbe: CanvasRenderingContext2D | null = null;

function cssColorToRgba(css: string, alpha = 1): Rgba {
  if (!colorProbe) {
    colorProbe = document.createElement("canvas").getContext("2d");
  }
  const probe = colorProbe;
  if (!probe) return [0.5, 0.5, 0.5, alpha];
  probe.fillStyle = css;
  const parsed = probe.fillStyle;
  if (typeof parsed !== "string") return [0.5, 0.5, 0.5, alpha];
  if (parsed.startsWith("#")) {
    const hex = parsed.slice(1);
    const full = hex.length === 3 ? hex.split("").map((c) => c + c).join("") : hex;
    const n = Number.parseInt(full, 16);
    return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255, alpha];
  }
  const m = parsed.match(/rgba?\(([^)]+)\)/);
  if (!m) return [0.5, 0.5, 0.5, alpha];
  const parts = m[1]!.split(",").map((p) => Number.parseFloat(p.trim()));
  return [(parts[0] ?? 0) / 255, (parts[1] ?? 0) / 255, (parts[2] ?? 0) / 255, parts[3] ?? alpha];
}

function lineSegment(
  x0: number,
  y0: number,
  x1: number,
  y1: number,
  height: number,
): Float32Array {
  const [dx0, dy0] = canvasToDataPoint(x0, y0, height);
  const [dx1, dy1] = canvasToDataPoint(x1, y1, height);
  return new Float32Array([dx0, dy0, dx1, dy1]);
}

function circleLinePoints(
  cx: number,
  cy: number,
  radius: number,
  height: number,
  segments = 12,
): Float32Array {
  const out = new Float32Array((segments + 1) * 2);
  for (let i = 0; i <= segments; i += 1) {
    const a = (i / segments) * Math.PI * 2;
    const [dx, dy] = canvasToDataPoint(cx + Math.cos(a) * radius, cy + Math.sin(a) * radius, height);
    out[i * 2] = dx;
    out[i * 2 + 1] = dy;
  }
  return out;
}

function polylineInCanvas(
  points: { t: number; v: number }[],
  trace: LogTraceSpec,
  layout: LogPanelLayout,
  tMin: number,
  tMax: number,
): { x: number; y: number }[] {
  const sorted = [...points].sort((a, b) => a.t - b.t);
  return sorted
    .map((p) => ({
      x: timeToCanvasX(p.t, tMin, tMax, layout),
      y: valueToCanvasY(p.v, trace.vMin, trace.vMax, layout),
    }))
    .filter((p) => Number.isFinite(p.x) && Number.isFinite(p.y));
}

function buildSolidTraceLine(
  trace: LogTraceSpec,
  layout: LogPanelLayout,
  tMin: number,
  tMax: number,
  height: number,
): LineConfig | null {
  const pts = trace.series.points;
  if (pts.length === 0) return null;

  const color = cssColorToRgba(trace.color, trace.preview ? 0.72 : 1);
  const thickness = trace.preview ? 1.75 : 2;

  if (pts.length === 1) {
    const p = pts[0]!;
    const cx = timeToCanvasX(p.t, tMin, tMax, layout);
    const cy = valueToCanvasY(p.v, trace.vMin, trace.vMax, layout);
    return {
      points: circleLinePoints(cx, cy, trace.preview ? 2 : 2.5, height),
      color,
      thickness,
    };
  }

  const canvasPts = polylineInCanvas(pts, trace, layout, tMin, tMax);
  if (canvasPts.length < 2) return null;

  const out = new Float32Array(canvasPts.length * 2);
  for (let i = 0; i < canvasPts.length; i += 1) {
    const [dx, dy] = canvasToDataPoint(canvasPts[i]!.x, canvasPts[i]!.y, height);
    out[i * 2] = dx;
    out[i * 2 + 1] = dy;
  }
  return { points: out, color, thickness };
}

function buildDashedTraceLines(
  trace: LogTraceSpec,
  layout: LogPanelLayout,
  tMin: number,
  tMax: number,
  height: number,
): LineConfig[] {
  const canvasPts = polylineInCanvas(trace.series.points, trace, layout, tMin, tMax);
  if (canvasPts.length === 0) return [];
  if (canvasPts.length === 1) {
    const solid = buildSolidTraceLine(trace, layout, tMin, tMax, height);
    return solid ? [solid] : [];
  }

  const color = cssColorToRgba(trace.color, 0.72);
  const lines: LineConfig[] = [];
  let dashLeft = DASH_ON;
  let drawing = true;

  for (let i = 1; i < canvasPts.length; i += 1) {
    const a = canvasPts[i - 1]!;
    const b = canvasPts[i]!;
    let segLen = Math.hypot(b.x - a.x, b.y - a.y);
    if (segLen < 1e-9) continue;

    let t = 0;
    while (t < segLen - 1e-9) {
      const step = Math.min(dashLeft, segLen - t);
      const t1 = t + step;
      const fx0 = a.x + ((b.x - a.x) * t) / segLen;
      const fy0 = a.y + ((b.y - a.y) * t) / segLen;
      const fx1 = a.x + ((b.x - a.x) * t1) / segLen;
      const fy1 = a.y + ((b.y - a.y) * t1) / segLen;

      if (drawing && step > 0.5) {
        lines.push({
          points: lineSegment(fx0, fy0, fx1, fy1, height),
          color,
          thickness: 1.75,
        });
      }

      t = t1;
      dashLeft -= step;
      if (dashLeft <= 1e-9) {
        drawing = !drawing;
        dashLeft = drawing ? DASH_ON : DASH_OFF;
      }
    }
  }

  return lines;
}

function buildPanelGridAndBorder(
  layout: LogPanelLayout,
  height: number,
  borderRgba: Rgba,
  borderStrongRgba: Rgba,
): LineConfig[] {
  const lines: LineConfig[] = [];
  const { plotLeft, plotRight, plotTop, plotBottom, plotW, plotH } = layout;

  for (let i = 0; i <= GRID_LINES; i += 1) {
    const y = plotTop + (i / GRID_LINES) * plotH;
    lines.push({
      points: lineSegment(plotLeft, y, plotLeft + plotW, y, height),
      color: borderRgba,
      thickness: 1,
    });
  }

  lines.push({
    points: lineSegment(plotLeft, plotTop, plotRight, plotTop, height),
    color: borderStrongRgba,
    thickness: 1,
  });
  lines.push({
    points: lineSegment(plotRight, plotTop, plotRight, plotBottom, height),
    color: borderStrongRgba,
    thickness: 1,
  });
  lines.push({
    points: lineSegment(plotRight, plotBottom, plotLeft, plotBottom, height),
    color: borderStrongRgba,
    thickness: 1,
  });
  lines.push({
    points: lineSegment(plotLeft, plotBottom, plotLeft, plotTop, height),
    color: borderStrongRgba,
    thickness: 1,
  });

  return lines;
}

function pixelBounds(width: number, height: number): DataBounds {
  return {
    minX: 0,
    maxX: width,
    minY: 0,
    maxY: height,
    coordinateSpace: { x: "linear", y: "linear" },
  };
}

export interface LogChartPaintRequest {
  width: number;
  height: number;
  panels: LogGraphPanelSpec[];
  tMin: number;
  tMax: number;
  crosshair: LogCrosshairSpec | null;
}

export class LogChartRenderer {
  private canvas: HTMLCanvasElement | null = null;
  private plot: WebglPlot | null = null;
  private lines: WebglLinePlot | null = null;
  private paintedW = 0;
  private paintedH = 0;
  private lineLayoutSig = "";
  private theme: ThemeColors = readTheme(null);
  private bgRgba: Rgba = [1, 1, 1, 1];
  private borderRgba: Rgba = [0.88, 0.85, 0.81, 1];
  private borderStrongRgba: Rgba = [0.81, 0.78, 0.72, 1];

  attach(canvas: HTMLCanvasElement): void {
    this.canvas = canvas;
    this.theme = readTheme(canvas);
    this.refreshColorCache();
    this.plot = new WebglPlot(canvas, {
      antialias: false,
      backgroundColor: this.bgRgba,
    });
    this.lines = this.plot.newThinLinePlotter(MAX_LINES);
    this.paintedW = 0;
    this.paintedH = 0;
  }

  detach(): void {
    this.lines?.cleanup();
    this.lines = null;
    this.plot = null;
    this.canvas = null;
    this.paintedW = 0;
    this.paintedH = 0;
  }

  private refreshColorCache(): void {
    this.bgRgba = cssColorToRgba(this.theme.bgElevated, 1);
    this.borderRgba = cssColorToRgba(this.theme.border, 1);
    this.borderStrongRgba = cssColorToRgba(this.theme.borderStrong, 1);
  }

  private syncCanvasSize(width: number, height: number): void {
    const canvas = this.canvas;
    const plot = this.plot;
    if (!canvas || !plot) return;
    if (width <= 0 || height <= 0) return;

    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    if (width !== this.paintedW || height !== this.paintedH) {
      handleCanvasResize(canvas, plot.gl);
      this.paintedW = width;
      this.paintedH = height;
    }
  }

  private buildLineConfigs(req: LogChartPaintRequest): LineConfig[] {
    const { width, height, panels, tMin, tMax } = req;
    const { layouts } = computeLogPanelLayouts(width, height, panels);
    const configs: LineConfig[] = [];

    for (const layout of layouts) {
      configs.push(...buildPanelGridAndBorder(layout, height, this.borderRgba, this.borderStrongRgba));
      for (const trace of layout.traces) {
        if (trace.preview) {
          configs.push(...buildDashedTraceLines(trace, layout, tMin, tMax, height));
        } else {
          const solid = buildSolidTraceLine(trace, layout, tMin, tMax, height);
          if (solid) configs.push(solid);
        }
      }
    }

    return configs.slice(0, MAX_LINES);
  }

  private lineLayoutSignature(configs: LineConfig[]): string {
    return configs.map((c) => c.points.length).join(",");
  }

  private uploadLines(lines: WebglLinePlot, lineConfigs: LineConfig[]): void {
    const sig = this.lineLayoutSignature(lineConfigs);
    if (sig !== this.lineLayoutSig) {
      lines.initLines(lineConfigs);
      this.lineLayoutSig = sig;
      return;
    }
    for (let i = 0; i < lineConfigs.length; i += 1) {
      const cfg = lineConfigs[i]!;
      lines.updateLinePoints(i, cfg.points);
      lines.updateLineColor(i, cfg.color);
      lines.updateLineThickness(i, cfg.thickness ?? 1);
    }
  }

  paint(req: LogChartPaintRequest): void {
    const canvas = this.canvas;
    const plot = this.plot;
    const lines = this.lines;
    if (!canvas || !plot || !lines) return;

    const { width, height } = req;
    if (width <= 0 || height <= 0) return;

    this.theme = readTheme(canvas);
    this.refreshColorCache();
    this.syncCanvasSize(width, height);

    const lineConfigs = this.buildLineConfigs(req);
    this.uploadLines(lines, lineConfigs);

    const gl = plot.gl;
    clearCanvas(gl, this.bgRgba);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);

    lines.transformToLinearSpace(pixelBounds(width, height));
    lines.draw();
  }

  clear(width: number, height: number): void {
    const plot = this.plot;
    const canvas = this.canvas;
    if (!plot || !canvas) return;
    this.syncCanvasSize(width, height);
    clearCanvas(plot.gl, this.bgRgba);
    this.lineLayoutSig = "";
  }
}

export const logChartRenderer = new LogChartRenderer();
