/**
 * WebGL renderer для engine sniffer (логического анализатора) на webgl-plot.
 * Ступенчатые сигналы каналов, сетка, TDC-маркеры и crosshair — батч GPU-линий.
 * Подписи каналов и тултип — HTML-оверлей в EngineSniffer.vue.
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
  laneY,
  startLevel,
  xAtTime,
  type SnifferView,
} from "./engineSnifferGeometry";
import type { SnifferEvent } from "../../composables/useEngineSniffer";
import { canvasToDataPoint } from "../../composables/logChartLayout";

type Rgba = [number, number, number, number];

const MAX_LINES = 512;

function solidVLine(x: number, height: number, color: Rgba, thickness: number): LineConfig {
  const [x0, y0] = canvasToDataPoint(x, 0, height);
  const [x1, y1] = canvasToDataPoint(x, height, height);
  return { points: new Float32Array([x0, y0, x1, y1]), color, thickness };
}

/** Базовая линия (low-уровень) lane — ориентир канала. */
function laneBaseline(laneIdx: number, view: SnifferView, color: Rgba): LineConfig {
  const { yLow } = laneY(laneIdx, view);
  const [x0, y0] = canvasToDataPoint(view.plotLeft, yLow, view.cssH);
  const [x1, y1] = canvasToDataPoint(view.plotLeft + view.plotW, yLow, view.cssH);
  return { points: new Float32Array([x0, y0, x1, y1]), color, thickness: 1 };
}

/** Ступенчатый сигнал одного канала. */
function buildChannelWave(
  name: string,
  laneIdx: number,
  view: SnifferView,
  height: number,
  color: Rgba,
): LineConfig | null {
  const evs: SnifferEvent[] = [];
  for (const e of view.events) {
    if (!e.tdc && e.name === name) evs.push(e);
  }
  const { yHigh, yLow } = laneY(laneIdx, view);

  let level = startLevel(evs);
  let y = level ? yHigh : yLow;
  const pts: number[] = [];

  const [sx, sy] = canvasToDataPoint(xAtTime(view.t0, view), y, height);
  pts.push(sx, sy);

  for (const e of evs) {
    if (e.tUs < view.t0 || e.tUs > view.tEnd) continue;
    const x = xAtTime(e.tUs, view);
    const [hx, hy] = canvasToDataPoint(x, y, height);
    pts.push(hx, hy);
    const ny = e.up ? yHigh : yLow;
    if (ny !== y) {
      const [vx, vy] = canvasToDataPoint(x, ny, height);
      pts.push(vx, vy);
      y = ny;
      level = e.up;
    }
  }

  const [ex, ey] = canvasToDataPoint(xAtTime(view.tEnd, view), y, height);
  pts.push(ex, ey);

  if (pts.length < 4) return null;
  return { points: new Float32Array(pts), color, thickness: 2 };
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

export interface SnifferRenderRequest {
  width: number;
  height: number;
  view: SnifferView | null;
  /** Цвет на канал (по индексу lane). */
  channelColors: Rgba[];
  bgRgba: Rgba;
  gridRgba: Rgba;
  baselineRgba: Rgba;
  tdcRgba: Rgba;
  crosshairRgba: Rgba;
  crosshairX: number | null;
}

export class EngineSnifferRenderer {
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

  paint(req: SnifferRenderRequest): void {
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

    const { view } = req;
    const configs: LineConfig[] = [];

    // Вертикальная сетка (0, 1/4, 2/4, 3/4, 1).
    for (let i = 0; i <= 4; i++) {
      const x = view.plotLeft + (view.plotW * i) / 4;
      configs.push(solidVLine(x, height, req.gridRgba, 1));
    }

    // Базовые линии каналов.
    for (let i = 0; i < view.channels.length; i++) {
      configs.push(laneBaseline(i, view, req.baselineRgba));
    }

    // TDC-маркеры: сплошная заметная вертикаль.
    for (const tUs of view.tdcTimes) {
      const x = xAtTime(tUs, view);
      if (x >= view.plotLeft - 2 && x <= view.plotLeft + view.plotW + 2) {
        configs.push(solidVLine(x, height, req.tdcRgba, 2));
      }
    }

    // Сигналы каналов.
    for (let i = 0; i < view.channels.length; i++) {
      const color = req.channelColors[i] ?? [0.6, 0.8, 1, 1];
      const wave = buildChannelWave(view.channels[i]!.name, i, view, height, color);
      if (wave) configs.push(wave);
    }

    // Crosshair.
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
