/** WebGL-отрисовка таймлайна (webgl-plot): клипы, сетка, «сейчас». */

import {
  clearCanvas,
  handleCanvasResize,
  WebglLinePlot,
  WebglPlot,
  WebglPolygonPlot,
  type DataBounds,
  type LineConfig,
  type PolygonConfig,
} from "webgl-plot";
import {
  RULER_HEIGHT_PX,
  TimelineFrameBuilder,
  TIMELINE_CHANNEL_LABELS,
  formatSpanMs,
  type ProjectTimelineClip,
  type TimelineFrame,
} from "./timelineFrame";

export { TIMELINE_CHANNEL_LABELS } from "./timelineFrame";
export type {
  ProjectTimelineClip,
  TimelineClipDraw,
  TimelineFrame,
  TimelineTickDraw,
} from "./timelineFrame";

export const TIMELINE_CLIP_FILL: Record<string, string> = {
  logs: "#7eb8e0",
  trigger: "#e07060",
  spectrogram: "#a882dc",
  runs: "#b8bcc4",
};

const CHANNEL_ORDER = TIMELINE_CHANNEL_LABELS.map((c) => c.id);
const LANE_PAD = 0.07;
const MAX_LINES = 48;
/** Секунды от центра viewport — float32-safe (не Unix ms ~1.7e12). */
const MS_TO_PLOT_X = 1 / 1000;

type ThemeColors = {
  bgElevated: string;
  border: string;
  accent: string;
};

type Rgba = [number, number, number, number];

function readTheme(el: HTMLElement | null): ThemeColors {
  const root = el ?? document.documentElement;
  const s = getComputedStyle(root);
  const v = (name: string, fallback: string) =>
    s.getPropertyValue(name).trim() || fallback;
  return {
    bgElevated: v("--color-bg-elevated", "#ffffff"),
    border: v("--color-border", "#d8dce3"),
    accent: v("--color-accent", "#c0392b"),
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

function laneIndex(channel: string): number {
  const idx = CHANNEL_ORDER.indexOf(channel as (typeof CHANNEL_ORDER)[number]);
  return idx >= 0 ? idx : 0;
}

function clipEndMs(clip: ProjectTimelineClip): number {
  if (clip.endMs != null && clip.endMs > 0) return clip.endMs;
  return clip.startMs + 120_000;
}

function pxPerMs(width: number, span: number): number {
  return width / span;
}

function panMs(frame: TimelineFrame, panOffsetPx: number): number {
  if (panOffsetPx === 0) return 0;
  return -panOffsetPx / pxPerMs(frame.widthPx, frame.spanMs);
}

/** Viewport в секундах от plotOriginMs; zoom/pan — только bounds. */
function plotBounds(
  frame: TimelineFrame,
  laneCount: number,
  panOffsetPx: number,
  originMs: number,
): DataBounds {
  const centerMs = frame.viewCenterMs + panMs(frame, panOffsetPx);
  const halfMs = frame.spanMs / 2;
  return {
    minX: (centerMs - halfMs - originMs) * MS_TO_PLOT_X,
    maxX: (centerMs + halfMs - originMs) * MS_TO_PLOT_X,
    minY: 0,
    maxY: laneCount,
    coordinateSpace: { x: "linear", y: "linear" },
  };
}

function msToPlotX(ms: number, originMs: number): number {
  return (ms - originMs) * MS_TO_PLOT_X;
}

function clipRectPoints(
  startMs: number,
  endMs: number,
  lane: number,
  originMs: number,
): Float32Array {
  const x0 = msToPlotX(startMs, originMs);
  const x1 = msToPlotX(endMs, originMs);
  const y0 = lane + LANE_PAD;
  const y1 = lane + 1 - LANE_PAD;
  return new Float32Array([x0, y0, x1, y0, x1, y1, x0, y1]);
}

function viewportUiKey(frame: TimelineFrame): string {
  return `${frame.viewCenterMs}|${frame.spanMs}|${frame.widthPx}`;
}

export class TimelineRenderer {
  private readonly builder = new TimelineFrameBuilder();
  private canvas: HTMLCanvasElement | null = null;
  private plot: WebglPlot | null = null;
  private polygons: WebglPolygonPlot | null = null;
  private lines: WebglLinePlot | null = null;
  private frame: TimelineFrame | null = null;
  private panOffsetPx = 0;
  private lanesHeightPx = 0;
  private paintedW = 0;
  private lastLanesH = 0;
  private theme: ThemeColors = readTheme(null);
  private bgRgba: Rgba = [1, 1, 1, 1];
  private borderRgba: Rgba = [0.5, 0.5, 0.5, 1];
  private accentRgba: Rgba = [0.75, 0.22, 0.17, 1];
  private clipFillRgba = new Map<string, Rgba>();
  private polygonCount = 0;
  private lineCount = 0;
  private plotOriginMs = 0;
  private polyClipsRef: ProjectTimelineClip[] | null = null;
  private lastUiKey = "";
  private lastSpanLabel = "";
  private rebuildRaf = 0;
  private zoomRaf = 0;
  private paintRaf = 0;
  private rulerFlushTimer = 0;
  private pendingRulerFrame: TimelineFrame | null = null;
  private hitTestTimer = 0;
  private onSpanLabelChange: ((label: string) => void) | null = null;
  private onFrameChange: ((frame: TimelineFrame) => void) | null = null;

  attach(
    canvas: HTMLCanvasElement,
    hooks?: {
      onSpanLabelChange?: (label: string) => void;
      onFrameChange?: (frame: TimelineFrame) => void;
    },
  ): void {
    this.canvas = canvas;
    this.theme = readTheme(canvas);
    this.refreshColorCache();
    this.onSpanLabelChange = hooks?.onSpanLabelChange ?? null;
    this.onFrameChange = hooks?.onFrameChange ?? null;
    this.plot = new WebglPlot(canvas, {
      antialias: false,
      backgroundColor: this.bgRgba,
    });
    this.polygons = new WebglPolygonPlot(this.plot.gl);
    this.lines = this.plot.newThinLinePlotter(MAX_LINES);
    this.paintedW = 0;
    this.lanesHeightPx = 0;
  }

  detach(): void {
    this.cancelScheduled();
    this.lines?.cleanup();
    this.polygons?.cleanup();
    this.lines = null;
    this.polygons = null;
    this.plot = null;
    this.canvas = null;
    this.frame = null;
    this.panOffsetPx = 0;
    this.polygonCount = 0;
    this.lineCount = 0;
    this.plotOriginMs = 0;
    this.polyClipsRef = null;
    this.lastUiKey = "";
    this.lastSpanLabel = "";
    this.pendingRulerFrame = null;
    if (this.rulerFlushTimer) {
      clearTimeout(this.rulerFlushTimer);
      this.rulerFlushTimer = 0;
    }
    if (this.hitTestTimer) {
      clearTimeout(this.hitTestTimer);
      this.hitTestTimer = 0;
    }
    this.onSpanLabelChange = null;
    this.onFrameChange = null;
  }

  private cancelScheduled(): void {
    if (this.rebuildRaf) {
      cancelAnimationFrame(this.rebuildRaf);
      this.rebuildRaf = 0;
    }
    if (this.zoomRaf) {
      cancelAnimationFrame(this.zoomRaf);
      this.zoomRaf = 0;
    }
    if (this.paintRaf) {
      cancelAnimationFrame(this.paintRaf);
      this.paintRaf = 0;
    }
  }

  private markPlotOrigin(): void {
    this.plotOriginMs = this.builder.viewCenterMs;
    this.polyClipsRef = null;
  }

  private refreshColorCache(): void {
    this.bgRgba = cssColorToRgba(this.theme.bgElevated, 1);
    this.borderRgba = cssColorToRgba(this.theme.border, 1);
    this.accentRgba = cssColorToRgba(this.theme.accent, 1);
    this.clipFillRgba.clear();
    for (const [channel, css] of Object.entries(TIMELINE_CLIP_FILL)) {
      this.clipFillRgba.set(channel, cssColorToRgba(css, 0.92));
    }
  }

  setClips(clips: ProjectTimelineClip[]): void {
    this.builder.setClips(clips);
    this.markPlotOrigin();
  }

  setSize(widthPx: number, heightPx: number): void {
    this.builder.setSize(widthPx, heightPx);
    this.lanesHeightPx = Math.max(1, heightPx - RULER_HEIGHT_PX);
    this.scheduleRebuild();
  }

  reset(paint = true): void {
    this.builder.reset();
    this.markPlotOrigin();
    this.rebuild(paint);
  }

  resetView(paint = true): void {
    this.panOffsetPx = 0;
    this.builder.reset();
    this.markPlotOrigin();
    this.rebuild(paint);
  }

  applyWheel(clientX: number, deltaY: number, deltaX: number, shiftPan: boolean): void {
    this.builder.applyWheel(clientX, deltaY, deltaX, shiftPan);
    if (shiftPan) {
      this.scheduleRebuild();
    } else {
      this.scheduleZoomUpdate();
    }
  }

  zoomAt(factor: number, anchorX: number): void {
    this.builder.zoomAt(anchorX, factor);
    this.scheduleZoomUpdate();
  }

  centerOnNow(): void {
    this.builder.centerOnNow();
    this.scheduleRebuild();
  }

  setPanOffset(px: number): void {
    this.panOffsetPx = px;
    this.schedulePaint();
  }

  commitPan(deltaPx: number): void {
    this.panOffsetPx = 0;
    if (deltaPx !== 0) {
      this.builder.panByPx(deltaPx);
      this.scheduleRebuild();
    } else {
      this.schedulePaint();
    }
  }

  getFrame(): TimelineFrame | null {
    return this.frame;
  }

  getPanOffsetPx(): number {
    return this.panOffsetPx;
  }

  spanLabel(): string {
    return this.frame?.spanLabel ?? "…";
  }

  private scheduleZoomUpdate(): void {
    if (this.zoomRaf) return;
    this.zoomRaf = requestAnimationFrame(() => {
      this.zoomRaf = 0;
      this.applyZoomFrame();
    });
  }

  /** Zoom: viewport + WebGL; линейка и hit-test — с debounce. */
  private applyZoomFrame(): void {
    const prev = this.frame;
    if (!prev) {
      this.rebuild(true);
      return;
    }
    prev.viewCenterMs = this.builder.viewCenterMs;
    prev.spanMs = this.builder.spanMs;
    prev.spanLabel = formatSpanMs(prev.spanMs);
    prev.nowXPx = prev.widthPx / 2;
    this.notifyUi(prev, { deferRuler: true });
    this.paint();
    this.scheduleHitTestRefresh();
  }

  private scheduleHitTestRefresh(): void {
    if (this.hitTestTimer) clearTimeout(this.hitTestTimer);
    this.hitTestTimer = window.setTimeout(() => {
      this.hitTestTimer = 0;
      if (!this.frame) return;
      const full = this.builder.build();
      this.frame.clips = full.clips;
      this.frame.ticks = full.ticks;
      this.flushRuler();
    }, 120);
  }

  private flushRuler(): void {
    const frame = this.pendingRulerFrame ?? this.frame;
    if (!frame) return;
    this.pendingRulerFrame = null;
    if (this.rulerFlushTimer) {
      clearTimeout(this.rulerFlushTimer);
      this.rulerFlushTimer = 0;
    }
    const uiKey = viewportUiKey(frame);
    if (uiKey !== this.lastUiKey) {
      this.lastUiKey = uiKey;
      this.onFrameChange?.(frame);
    }
  }

  private scheduleRebuild(): void {
    if (this.rebuildRaf) return;
    this.rebuildRaf = requestAnimationFrame(() => {
      this.rebuildRaf = 0;
      this.flushRuler();
      this.rebuild(true);
    });
  }

  private schedulePaint(): void {
    if (this.paintRaf) return;
    this.paintRaf = requestAnimationFrame(() => {
      this.paintRaf = 0;
      this.paint();
    });
  }

  rebuild(paint = true): void {
    this.frame = this.builder.build();
    if (this.plotOriginMs === 0) this.plotOriginMs = this.builder.viewCenterMs;
    this.notifyUi(this.frame);
    if (paint) this.paint();
  }

  private notifyUi(frame: TimelineFrame, opts?: { deferRuler?: boolean }): void {
    if (frame.spanLabel !== this.lastSpanLabel) {
      this.lastSpanLabel = frame.spanLabel;
      this.onSpanLabelChange?.(frame.spanLabel);
    }
    if (opts?.deferRuler) {
      this.pendingRulerFrame = frame;
      if (!this.rulerFlushTimer) {
        this.rulerFlushTimer = window.setTimeout(() => this.flushRuler(), 100);
      }
      return;
    }
    const uiKey = viewportUiKey(frame);
    if (uiKey !== this.lastUiKey) {
      this.lastUiKey = uiKey;
      this.onFrameChange?.(frame);
    }
  }

  private syncCanvasSize(widthPx: number, lanesH: number): void {
    const canvas = this.canvas;
    const plot = this.plot;
    if (!canvas || !plot) return;
    if (widthPx <= 0 || lanesH <= 0) return;

    canvas.style.width = `${widthPx}px`;
    canvas.style.height = `${lanesH}px`;
    if (widthPx !== this.paintedW || lanesH !== this.lastLanesH) {
      handleCanvasResize(canvas, plot.gl);
      this.paintedW = widthPx;
      this.lastLanesH = lanesH;
    }
  }

  private needsPolygonRefresh(): boolean {
    return this.builder.clips !== this.polyClipsRef;
  }

  private buildPolygons(clips: ProjectTimelineClip[], originMs: number): PolygonConfig[] {
    const out: PolygonConfig[] = [];
    for (const clip of clips) {
      const lane = laneIndex(clip.channel);
      const end = clipEndMs(clip);
      const fill = this.clipFillRgba.get(clip.channel) ?? this.clipFillRgba.get("logs")!;
      out.push({
        points: clipRectPoints(clip.startMs, end, lane, originMs),
        fillColor: fill,
        strokeColor: [0, 0, 0, 0],
        strokeWeight: 0,
        isFilled: true,
        isStroked: false,
      });
    }
    return out;
  }

  private buildLines(
    bounds: DataBounds,
    laneCount: number,
    originMs: number,
    nowMs: number,
  ): LineConfig[] {
    const out: LineConfig[] = [];

    for (let i = 1; i < laneCount; i += 1) {
      out.push({
        points: new Float32Array([bounds.minX, i, bounds.maxX, i]),
        color: this.borderRgba,
        thickness: 1.5,
      });
    }

    const nowX = msToPlotX(nowMs, originMs);
    if (nowX >= bounds.minX && nowX <= bounds.maxX) {
      out.push({
        points: new Float32Array([nowX, 0, nowX, laneCount]),
        color: this.accentRgba,
        thickness: 2.5,
      });
    }

    return out;
  }

  paint(nowMs = Date.now()): void {
    const canvas = this.canvas;
    const plot = this.plot;
    const polygons = this.polygons;
    const lines = this.lines;
    const frame = this.frame;
    if (!canvas || !plot || !polygons || !lines || !frame) return;

    const w = frame.widthPx;
    const lanesH = this.lanesHeightPx;
    if (w <= 0 || lanesH <= 0) return;

    this.syncCanvasSize(w, lanesH);

    const laneCount = frame.laneCount || CHANNEL_ORDER.length;
    const originMs = this.plotOriginMs;
    const bounds = plotBounds(frame, laneCount, this.panOffsetPx, originMs);

    if (this.needsPolygonRefresh()) {
      const polyConfigs = this.buildPolygons(this.builder.clips, originMs);
      if (polyConfigs.length !== this.polygonCount) {
        polygons.initPolygons(polyConfigs);
        this.polygonCount = polyConfigs.length;
      } else {
        for (let i = 0; i < polyConfigs.length; i += 1) {
          polygons.updatePolygonPoints(i, polyConfigs[i]!.points);
        }
      }
      this.polyClipsRef = this.builder.clips;
    }

    const lineConfigs = this.buildLines(bounds, laneCount, originMs, nowMs);
    if (lineConfigs.length !== this.lineCount) {
      lines.initLines(lineConfigs);
      this.lineCount = lineConfigs.length;
    } else {
      for (let i = 0; i < lineConfigs.length; i += 1) {
        lines.updateLinePoints(i, lineConfigs[i]!.points);
      }
    }

    const gl = plot.gl;
    clearCanvas(gl, this.bgRgba);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);

    lines.transformToLinearSpace(bounds);
    polygons.setGlobalTransform(lines.getGlobalScale(), lines.getGlobalOffset());
    lines.draw();
    polygons.draw();
  }

  hitTest(x: number, y: number): string | null {
    const frame = this.frame;
    if (!frame) return null;
    const xx = x - this.panOffsetPx;

    if (y <= 0 || y > this.lanesHeightPx) return null;
    const laneCount = frame.laneCount || CHANNEL_ORDER.length;
    const laneH = this.lanesHeightPx / laneCount;
    const laneIdx = Math.floor(y / laneH);
    if (laneIdx < 0 || laneIdx >= laneCount) return null;

    const barTop = laneIdx * laneH + 5;
    const barH = Math.max(4, laneH - 10);
    if (y < barTop || y > barTop + barH) return null;

    for (const clip of frame.clips) {
      if (clip.laneIndex !== laneIdx) continue;
      const right = clip.leftPx + clip.widthPx;
      if (xx >= clip.leftPx && xx <= right) return clip.tooltip;
    }
    return null;
  }
}

export const timelineRenderer = new TimelineRenderer();
