/** WebGL-отрисовка dyno (webgl-plot): сетка, Nm/HP по RPM. */

import {
  clearCanvas,
  handleCanvasResize,
  WebglLinePlot,
  WebglPlot,
  type DataBounds,
  type LineConfig,
} from "webgl-plot";
import type { DynoRunPoint } from "./dynoTypes";
import {
  canvasXToRpm,
  computeDynoFixedLayout,
  hpToCanvasY,
  interpolateDynoAtRpm,
  refreshDynoLayoutPlotSize,
  rpmToCanvasX,
  torqueToCanvasY,
  type DynoAxisRange,
  type DynoChartLayout,
  type DynoCrosshairSpec,
} from "./dynoChartLayout";
import { canvasToDataPoint } from "./logChartLayout";

const MAX_LINES = 64;
const GRID_LINES = 5;
const CROSSHAIR_LINES = 7;
const MARKER_HALF = 5;

type Rgba = [number, number, number, number];

type ThemeColors = {
  bgElevated: string;
  border: string;
  borderStrong: string;
  torque: string;
  hp: string;
  prevTorque: string;
  prevHp: string;
  crosshair: string;
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
    torque: v("--color-accent", "#3d7ea6"),
    hp: v("--color-success-text", "#2d6a4f"),
    prevTorque: v("--color-text-muted", "#8a8278"),
    prevHp: v("--color-text-subtle", "#9c948a"),
    crosshair: v("--color-accent", "#3d7ea6"),
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

function validSortedPoints(points: DynoRunPoint[]): DynoRunPoint[] {
  return points
    .filter(
      (p) =>
        Number.isFinite(p.rpm) &&
        Number.isFinite(p.torqueNm) &&
        Number.isFinite(p.hp),
    )
    .sort((a, b) => a.rpm - b.rpm);
}

function polylineTorque(
  points: DynoRunPoint[],
  layout: DynoChartLayout,
): { x: number; y: number }[] {
  return points.map((p) => ({
    x: rpmToCanvasX(p.rpm, layout),
    y: torqueToCanvasY(p.torqueNm, layout),
  }));
}

function polylineHp(
  points: DynoRunPoint[],
  layout: DynoChartLayout,
): { x: number; y: number }[] {
  return points.map((p) => ({
    x: rpmToCanvasX(p.rpm, layout),
    y: hpToCanvasY(p.hp, layout),
  }));
}

function buildSolidLine(
  canvasPts: { x: number; y: number }[],
  height: number,
  color: Rgba,
  thickness: number,
): LineConfig | null {
  if (canvasPts.length < 2) return null;
  const out = new Float32Array(canvasPts.length * 2);
  for (let i = 0; i < canvasPts.length; i += 1) {
    const [dx, dy] = canvasToDataPoint(canvasPts[i]!.x, canvasPts[i]!.y, height);
    out[i * 2] = dx;
    out[i * 2 + 1] = dy;
  }
  return { points: out, color, thickness };
}

function buildCrossMarker(
  cx: number,
  cy: number,
  height: number,
  color: Rgba,
): [LineConfig, LineConfig] {
  return [
    {
      points: lineSegment(cx - MARKER_HALF, cy, cx + MARKER_HALF, cy, height),
      color,
      thickness: 2,
    },
    {
      points: lineSegment(cx, cy - MARKER_HALF, cx, cy + MARKER_HALF, height),
      color,
      thickness: 2,
    },
  ];
}

function buildCrosshairLineConfigs(
  crosshairX: number,
  layout: DynoChartLayout,
  points: DynoRunPoint[],
  height: number,
  crosshairRgba: Rgba,
  torqueRgba: Rgba,
  hpRgba: Rgba,
): LineConfig[] {
  const { plotLeft, plotRight, plotTop, plotBottom } = layout;
  const x = Math.min(plotRight, Math.max(plotLeft, crosshairX));
  const lines: LineConfig[] = [
    {
      points: lineSegment(x, plotTop, x, plotBottom, height),
      color: crosshairRgba,
      thickness: 1,
    },
  ];

  const rpm = canvasXToRpm(x, layout);
  const sample = rpm !== null ? interpolateDynoAtRpm(points, rpm) : null;
  if (!sample) {
    while (lines.length < CROSSHAIR_LINES) {
      lines.push({
        points: lineSegment(x, plotTop, x, plotTop, height),
        color: crosshairRgba,
        thickness: 1,
      });
    }
    return lines;
  }

  const tqY = torqueToCanvasY(sample.torqueNm, layout);
  const hpY = hpToCanvasY(sample.hp, layout);

  lines.push({
    points: lineSegment(plotLeft, tqY, x, tqY, height),
    color: torqueRgba,
    thickness: 1,
  });
  lines.push({
    points: lineSegment(x, hpY, plotRight, hpY, height),
    color: hpRgba,
    thickness: 1,
  });
  lines.push(...buildCrossMarker(x, tqY, height, torqueRgba));
  lines.push(...buildCrossMarker(x, hpY, height, hpRgba));
  return lines;
}

function buildGridAndBorder(
  layout: DynoChartLayout,
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

  for (let i = 1; i < 6; i += 1) {
    const x = plotLeft + (i / 6) * plotW;
    lines.push({
      points: lineSegment(x, plotTop, x, plotBottom, height),
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

function stableLayoutSignature(
  width: number,
  height: number,
  previousLen: number,
  layout: DynoChartLayout,
): string {
  if (!layout.hasData) return "empty";
  return [
    width,
    height,
    previousLen,
    layout.xMin.toFixed(0),
    layout.xMax.toFixed(0),
    layout.tqLo.toFixed(0),
    layout.tqHi.toFixed(0),
    layout.hpLo.toFixed(0),
    layout.hpHi.toFixed(0),
  ].join("|");
}

export interface DynoChartPaintRequest {
  width: number;
  height: number;
  points: DynoRunPoint[];
  previousPoints: DynoRunPoint[];
  crosshair: DynoCrosshairSpec | null;
  /** Live-запись: append точек на фиксированное поле. */
  recording?: boolean;
  axes: DynoAxisRange;
}

export class DynoChartRenderer {
  private canvas: HTMLCanvasElement | null = null;
  private plot: WebglPlot | null = null;
  private lines: WebglLinePlot | null = null;
  private paintedW = 0;
  private paintedH = 0;
  private cachedStaticSig = "";
  private cachedStaticConfigs: LineConfig[] = [];
  private staticLineCount = 0;
  private hasCrosshairLines = false;
  private cachedCurCount = 0;
  private lastDrawLayout: DynoChartLayout | null = null;
  private lastDrawPoints: DynoRunPoint[] = [];
  private theme: ThemeColors = readTheme(null);
  private bgRgba: Rgba = [1, 1, 1, 1];
  private borderRgba: Rgba = [0.88, 0.85, 0.81, 1];
  private borderStrongRgba: Rgba = [0.81, 0.78, 0.72, 1];
  private torqueRgba: Rgba = [0.24, 0.49, 0.65, 1];
  private hpRgba: Rgba = [0.18, 0.42, 0.31, 1];
  private prevTorqueRgba: Rgba = [0.54, 0.51, 0.47, 0.72];
  private prevHpRgba: Rgba = [0.61, 0.58, 0.54, 0.72];
  private crosshairRgba: Rgba = [0.24, 0.49, 0.65, 0.45];

  attach(canvas: HTMLCanvasElement): boolean {
    this.canvas = canvas;
    this.theme = readTheme(canvas);
    this.refreshColorCache();
    try {
      this.plot = new WebglPlot(canvas, {
        antialias: false,
        backgroundColor: this.bgRgba,
      });
    } catch {
      this.plot = null;
      return false;
    }
    this.lines = this.plot.newThinLinePlotter(MAX_LINES);
    this.resetSceneCache();
    return true;
  }

  detach(): void {
    this.lines?.cleanup();
    this.lines = null;
    this.plot = null;
    this.canvas = null;
    this.resetSceneCache();
  }

  /** Сброс кэша live-отрисовки (новый прогон, смена данных). */
  resetLiveCache(): void {
    this.resetSceneCache();
  }

  private resetSceneCache(): void {
    this.paintedW = 0;
    this.paintedH = 0;
    this.cachedStaticSig = "";
    this.cachedStaticConfigs = [];
    this.staticLineCount = 0;
    this.hasCrosshairLines = false;
    this.cachedCurCount = 0;
    this.lastDrawLayout = null;
    this.lastDrawPoints = [];
  }

  private refreshColorCache(): void {
    this.bgRgba = cssColorToRgba(this.theme.bgElevated, 1);
    this.borderRgba = cssColorToRgba(this.theme.border, 1);
    this.borderStrongRgba = cssColorToRgba(this.theme.borderStrong, 1);
    this.torqueRgba = cssColorToRgba(this.theme.torque, 1);
    this.hpRgba = cssColorToRgba(this.theme.hp, 1);
    this.prevTorqueRgba = cssColorToRgba(this.theme.prevTorque, 0.72);
    this.prevHpRgba = cssColorToRgba(this.theme.prevHp, 0.72);
    this.crosshairRgba = cssColorToRgba(this.theme.crosshair, 0.45);
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
      this.cachedStaticSig = "";
    }
  }

  private buildStaticLineConfigs(
    layout: DynoChartLayout,
    height: number,
    cur: DynoRunPoint[],
    prev: DynoRunPoint[],
  ): LineConfig[] {
    const configs: LineConfig[] = buildGridAndBorder(
      layout,
      height,
      this.borderRgba,
      this.borderStrongRgba,
    );

    if (prev.length >= 2) {
      const prevTorque = buildSolidLine(
        polylineTorque(prev, layout),
        height,
        this.prevTorqueRgba,
        1.75,
      );
      const prevHp = buildSolidLine(polylineHp(prev, layout), height, this.prevHpRgba, 1.75);
      if (prevTorque) configs.push(prevTorque);
      if (prevHp) configs.push(prevHp);
    }

    const torqueLine = buildSolidLine(polylineTorque(cur, layout), height, this.torqueRgba, 2.5);
    const hpLine = buildSolidLine(polylineHp(cur, layout), height, this.hpRgba, 2.5);
    if (torqueLine) configs.push(torqueLine);
    if (hpLine) configs.push(hpLine);

    return configs;
  }

  private uploadScene(
    lines: WebglLinePlot,
    staticConfigs: LineConfig[],
    crossConfigs: LineConfig[] | null,
    staticSig: string,
    curCount: number,
    forceRebuild: boolean,
  ): void {
    const layoutChanged = staticSig !== this.cachedStaticSig;
    const geometryChanged = curCount !== this.cachedCurCount;

    if (layoutChanged || geometryChanged || forceRebuild) {
      this.cachedStaticSig = staticSig;
      this.cachedStaticConfigs = staticConfigs;
      this.staticLineCount = staticConfigs.length;
      if (crossConfigs) {
        lines.initLines([...staticConfigs, ...crossConfigs]);
        this.hasCrosshairLines = true;
      } else {
        lines.initLines(staticConfigs);
        this.hasCrosshairLines = false;
      }
      return;
    }

    if (crossConfigs) {
      if (!this.hasCrosshairLines) {
        lines.initLines([...staticConfigs, ...crossConfigs]);
        this.hasCrosshairLines = true;
        return;
      }
      for (let i = 0; i < crossConfigs.length; i += 1) {
        lines.updateLinePoints(this.staticLineCount + i, crossConfigs[i]!.points);
      }
      return;
    }

    if (this.hasCrosshairLines) {
      lines.initLines(staticConfigs);
      this.hasCrosshairLines = false;
    }
  }

  private drawScene(lines: WebglLinePlot, width: number, height: number): void {
    const gl = this.plot!.gl;
    clearCanvas(gl, this.bgRgba);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    lines.transformToLinearSpace(pixelBounds(width, height));
    lines.draw();
  }

  paint(req: DynoChartPaintRequest): DynoChartLayout {
    const canvas = this.canvas;
    const plot = this.plot;
    const lines = this.lines;
    const { width, height } = req;

    const baseLayout = computeDynoFixedLayout(width, height, req.axes);

    if (!canvas || !plot || !lines) return baseLayout;
    if (width <= 0 || height <= 0) return baseLayout;

    this.syncCanvasSize(width, height);

    const cur = validSortedPoints(req.points);
    const prev = validSortedPoints(req.previousPoints);
    const drawLayout = refreshDynoLayoutPlotSize(baseLayout, width, height);
    const staticSig = stableLayoutSignature(width, height, prev.length, drawLayout);

    const crossConfigs =
      req.crosshair !== null && cur.length >= 2
        ? buildCrosshairLineConfigs(
            req.crosshair.x,
            drawLayout,
            cur,
            height,
            this.crosshairRgba,
            this.torqueRgba,
            this.hpRgba,
          )
        : null;

    const staticConfigs = this.buildStaticLineConfigs(drawLayout, height, cur, prev);
    this.uploadScene(
      lines,
      staticConfigs,
      crossConfigs,
      staticSig,
      cur.length,
      req.recording === true,
    );
    this.cachedCurCount = cur.length;
    this.drawScene(lines, width, height);

    this.lastDrawLayout = drawLayout;
    this.lastDrawPoints = cur;
    return drawLayout;
  }

  /** Только crosshair — без пересборки кривых (движение мыши). */
  repaintCrosshair(crosshair: DynoCrosshairSpec | null): void {
    const lines = this.lines;
    const layout = this.lastDrawLayout;
    if (!lines || !layout || !layout.hasData || this.paintedW <= 0 || this.paintedH <= 0) {
      return;
    }
    if (this.cachedStaticConfigs.length === 0) return;

    const width = this.paintedW;
    const height = this.paintedH;
    const crossConfigs =
      crosshair !== null && this.lastDrawPoints.length >= 2
        ? buildCrosshairLineConfigs(
            crosshair.x,
            layout,
            this.lastDrawPoints,
            height,
            this.crosshairRgba,
            this.torqueRgba,
            this.hpRgba,
          )
        : null;

    this.uploadScene(
      lines,
      this.cachedStaticConfigs,
      crossConfigs,
      this.cachedStaticSig,
      this.lastDrawPoints.length,
      false,
    );
    this.drawScene(lines, width, height);
  }

  lastLayout(): DynoChartLayout | null {
    return this.lastDrawLayout;
  }
}

export const dynoChartRenderer = new DynoChartRenderer();
