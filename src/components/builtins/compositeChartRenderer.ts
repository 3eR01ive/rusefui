/**
 * WebGL renderer for the composite trigger chart (webgl-plot).
 * Renders step-function waveforms, grid, TDC markers, and crosshair as batched GPU lines.
 * Text labels and crosshair tooltip are HTML overlay in CompositeChart.vue.
 */
import {
  clearCanvas,
  handleCanvasResize,
  WebglLinePlot,
  WebglPlot,
  type DataBounds,
  type LineConfig,
} from "webgl-plot";
import {
  channelValue,
  laneY,
  valueAtTime,
  xAtTime,
  type ChannelKey,
  type ChartView,
} from "./compositeChartGeometry";
import type { CrankEdgeMode } from "../../composables/useProject";
import { canvasToDataPoint } from "../../composables/logChartLayout";

type Rgba = [number, number, number, number];

// Channels × polyline points + TDC dashes + grid + misc
const MAX_LINES = 512;
const DASH_ON = 5;
const DASH_OFF = 4;

// ---- Helpers ----------------------------------------------------------------

function solidVLine(x: number, height: number, color: Rgba, thickness: number): LineConfig {
  const [x0, y0] = canvasToDataPoint(x, 0, height);
  const [x1, y1] = canvasToDataPoint(x, height, height);
  return { points: new Float32Array([x0, y0, x1, y1]), color, thickness };
}

function dashedVLine(
  x: number,
  height: number,
  color: Rgba,
  thickness: number,
  dashOn = DASH_ON,
  dashOff = DASH_OFF,
): LineConfig[] {
  const segs: LineConfig[] = [];
  let y = 0;
  let on = true;
  while (y < height - 0.5) {
    const step = on ? dashOn : dashOff;
    const y1 = Math.min(y + step, height);
    if (on) {
      const [x0, y0d] = canvasToDataPoint(x, y, height);
      const [x1, y1d] = canvasToDataPoint(x, y1, height);
      segs.push({ points: new Float32Array([x0, y0d, x1, y1d]), color, thickness });
    }
    y = y1;
    on = !on;
  }
  return segs;
}

/** Step-function polyline for a single channel — one LineConfig covers all visible events. */
function buildStepWave(
  key: ChannelKey,
  laneIdx: number,
  view: ChartView,
  height: number,
  color: Rgba,
): LineConfig | null {
  const { visible } = view;
  if (visible.length === 0) return null;

  const { yHigh, yLow } = laneY(laneIdx, view, true);
  const pts: number[] = [];

  let prevT = view.t0;
  let prevVal = valueAtTime(view.t0, visible, key);
  let prevY = prevVal ? yHigh : yLow;

  const [ix, iy] = canvasToDataPoint(xAtTime(view.t0, view), prevY, height);
  pts.push(ix, iy);

  for (const ev of visible) {
    const x = xAtTime(ev.tUs, view);
    const val = channelValue(ev, key);
    const newY = val ? yHigh : yLow;

    if (ev.tUs > prevT) {
      const [xd, yd] = canvasToDataPoint(x, prevY, height);
      pts.push(xd, yd);
    }
    if (val !== prevVal) {
      const [xd, yd] = canvasToDataPoint(x, newY, height);
      pts.push(xd, yd);
      prevY = newY;
    }
    prevT = ev.tUs;
    prevVal = val;
  }

  // Extend to right edge
  const [xe, ye] = canvasToDataPoint(view.plotLeft + view.plotW, prevY, height);
  pts.push(xe, ye);

  if (pts.length < 4) return null;
  return { points: new Float32Array(pts), color, thickness: 2 };
}

/** Tick marks for rise-only or fall-only edge mode (primary channel). */
function buildEdgeTicks(
  key: ChannelKey,
  laneIdx: number,
  view: ChartView,
  height: number,
  edgeMode: "rise" | "fall",
  color: Rgba,
): LineConfig[] {
  const { yHigh, yLow } = laneY(laneIdx, view, true);
  const tickH = (yLow - yHigh) * 0.65;
  const baseline = edgeMode === "rise" ? yLow : yHigh;
  const tipY = baseline + (edgeMode === "rise" ? -tickH : tickH);

  const segs: LineConfig[] = [];
  let prevVal = valueAtTime(view.t0, view.visible, key);

  for (const ev of view.visible) {
    const val = channelValue(ev, key);
    if (val !== prevVal) {
      const isRise = val && !prevVal;
      if ((edgeMode === "rise" && isRise) || (edgeMode === "fall" && !isRise)) {
        const x = xAtTime(ev.tUs, view);
        const [x0, y0] = canvasToDataPoint(x, baseline, height);
        const [x1, y1] = canvasToDataPoint(x, tipY, height);
        segs.push({ points: new Float32Array([x0, y0, x1, y1]), color, thickness: 2 });
      }
    }
    prevVal = val;
  }
  return segs;
}

function pixelBounds(width: number, height: number): DataBounds {
  return {
    minX: 0, maxX: width,
    minY: 0, maxY: height,
    coordinateSpace: { x: "linear", y: "linear" },
  };
}

// ---- Public types -----------------------------------------------------------

export interface CompositeChannelDef {
  key: ChannelKey;
  color: Rgba;
}

export interface CompositeRenderRequest {
  width: number;
  height: number;
  view: ChartView | null;
  channels: CompositeChannelDef[];
  edgeMode: CrankEdgeMode;
  bgRgba: Rgba;
  gridRgba: Rgba;
  tdcRgba: Rgba;
  accentRgba: Rgba;
  crosshairRgba: Rgba;
  realTdcTUs: number | null;
  crosshairX: number | null;
}

// ---- Renderer ---------------------------------------------------------------

export class CompositeChartRenderer {
  private canvas: HTMLCanvasElement | null = null;
  private plot: WebglPlot | null = null;
  private lines: WebglLinePlot | null = null;
  private paintedW = 0;
  private paintedH = 0;
  private lineSig = "";

  attach(canvas: HTMLCanvasElement): void {
    this.canvas = canvas;
    this.plot = new WebglPlot(canvas, { antialias: false, backgroundColor: [0, 0, 0, 1] });
    this.lines = this.plot.newThinLinePlotter(MAX_LINES);
    this.paintedW = 0;
    this.paintedH = 0;
    this.lineSig = "";
  }

  detach(): void {
    this.lines?.cleanup();
    this.lines = null;
    this.plot = null;
    this.canvas = null;
    this.paintedW = 0;
    this.paintedH = 0;
    this.lineSig = "";
  }

  private syncSize(width: number, height: number): void {
    const { canvas, plot } = this;
    if (!canvas || !plot) return;
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    if (width !== this.paintedW || height !== this.paintedH) {
      handleCanvasResize(canvas, plot.gl);
      this.paintedW = width;
      this.paintedH = height;
    }
  }

  paint(req: CompositeRenderRequest): void {
    const { canvas, plot, lines } = this;
    if (!canvas || !plot || !lines) return;
    const { width, height } = req;
    if (width <= 0 || height <= 0) return;

    this.syncSize(width, height);
    const gl = plot.gl;
    clearCanvas(gl, req.bgRgba);

    if (!req.view) {
      this.lineSig = "";
      return;
    }

    const { view, channels, edgeMode } = req;
    const configs: LineConfig[] = [];

    // Vertical grid (5 lines: 0, 1/4, 2/4, 3/4, 1)
    for (let i = 0; i <= 4; i++) {
      const x = view.plotLeft + (view.plotW * i) / 4;
      configs.push(solidVLine(x, height, req.gridRgba, 1));
    }

    // TDC cycle markers (dashed)
    for (const m of view.tdcMarkers) {
      const x = xAtTime(m.tUs, view);
      if (x >= view.plotLeft - 2 && x <= view.plotLeft + view.plotW + 2) {
        configs.push(...dashedVLine(x, height, req.tdcRgba, 1));
      }
    }

    // Real TDC marker (dashed, accent color)
    if (req.realTdcTUs != null) {
      const x = xAtTime(req.realTdcTUs, view);
      if (x >= view.plotLeft - 2 && x <= view.plotLeft + view.plotW + 2) {
        configs.push(...dashedVLine(x, height, req.accentRgba, 2, 3, 3));
      }
    }

    // Channel waveforms
    for (let i = 0; i < channels.length; i++) {
      const ch = channels[i]!;
      if (ch.key === "pri" && edgeMode !== "both") {
        configs.push(...buildEdgeTicks(ch.key, i, view, height, edgeMode, ch.color));
      } else {
        const line = buildStepWave(ch.key, i, view, height, ch.color);
        if (line) configs.push(line);
      }
    }

    // Crosshair vertical line
    if (req.crosshairX != null) {
      configs.push(solidVLine(req.crosshairX, height, req.crosshairRgba, 1));
    }

    const capped = configs.slice(0, MAX_LINES);
    const sig = capped.map((c) => c.points.length).join(",");
    if (sig !== this.lineSig) {
      lines.initLines(capped);
      this.lineSig = sig;
    } else {
      for (let i = 0; i < capped.length; i++) {
        const cfg = capped[i]!;
        lines.updateLinePoints(i, cfg.points);
        lines.updateLineColor(i, cfg.color);
        lines.updateLineThickness(i, cfg.thickness ?? 1);
      }
    }

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    lines.transformToLinearSpace(pixelBounds(width, height));
    lines.draw();
  }
}
