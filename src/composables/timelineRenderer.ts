/** Canvas-отрисовка таймлайна — без IPC, кадр собирается локально. */

import {
  TimelineFrameBuilder,
  TIMELINE_CHANNEL_LABELS,
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

const LANE_PAD_Y = 5;
const MIN_LABEL_PX = 52;

type ThemeColors = {
  bgMuted: string;
  bgElevated: string;
  border: string;
  borderStrong: string;
  textSubtle: string;
  accent: string;
};

function readTheme(el: HTMLElement | null): ThemeColors {
  const root = el ?? document.documentElement;
  const s = getComputedStyle(root);
  const v = (name: string, fallback: string) =>
    s.getPropertyValue(name).trim() || fallback;
  return {
    bgMuted: v("--color-bg-muted", "#eceef1"),
    bgElevated: v("--color-bg-elevated", "#fff"),
    border: v("--color-border", "#d8dce3"),
    borderStrong: v("--color-border-strong", "#b8bec8"),
    textSubtle: v("--color-text-subtle", "#6b7280"),
    accent: v("--color-accent", "#c0392b"),
  };
}

function nowLinePx(frame: TimelineFrame, nowMs: number): number {
  const ppm = frame.widthPx / frame.spanMs;
  return (nowMs - frame.viewCenterMs) * ppm + frame.widthPx / 2;
}

export class TimelineRenderer {
  private readonly builder = new TimelineFrameBuilder();
  private canvas: HTMLCanvasElement | null = null;
  private frame: TimelineFrame | null = null;
  private panOffsetPx = 0;
  private paintedW = 0;
  private paintedH = 0;
  private theme: ThemeColors = readTheme(null);
  private onSpanLabelChange: ((label: string) => void) | null = null;

  attach(canvas: HTMLCanvasElement, onSpanLabelChange?: (label: string) => void): void {
    this.canvas = canvas;
    this.onSpanLabelChange = onSpanLabelChange ?? null;
    this.theme = readTheme(canvas);
    this.paintedW = 0;
    this.paintedH = 0;
  }

  detach(): void {
    this.canvas = null;
    this.frame = null;
    this.panOffsetPx = 0;
    this.paintedW = 0;
    this.paintedH = 0;
    this.onSpanLabelChange = null;
  }

  setClips(clips: ProjectTimelineClip[]): void {
    this.builder.setClips(clips);
  }

  setSize(widthPx: number, heightPx: number): void {
    this.builder.setSize(widthPx, heightPx);
    this.rebuild();
  }

  reset(): void {
    this.builder.reset();
    this.rebuild();
  }

  applyWheel(clientX: number, deltaY: number, deltaX: number, shiftPan: boolean): void {
    this.builder.applyWheel(clientX, deltaY, deltaX, shiftPan);
    this.rebuild();
  }

  zoomAt(factor: number, anchorX: number): void {
    this.builder.zoomAt(anchorX, factor);
    this.rebuild();
  }

  centerOnNow(): void {
    this.builder.centerOnNow();
    this.rebuild();
  }

  setPanOffset(px: number): void {
    this.panOffsetPx = px;
  }

  commitPan(deltaPx: number): void {
    this.panOffsetPx = 0;
    if (deltaPx !== 0) {
      this.builder.panByPx(deltaPx);
      this.rebuild();
    } else {
      this.paint();
    }
  }

  getFrame(): TimelineFrame | null {
    return this.frame;
  }

  spanLabel(): string {
    return this.frame?.spanLabel ?? "…";
  }

  rebuild(): void {
    this.frame = this.builder.build();
    this.onSpanLabelChange?.(this.frame.spanLabel);
    this.paint();
  }

  paint(nowMs = Date.now()): void {
    const canvas = this.canvas;
    const frame = this.frame;
    if (!canvas || !frame) return;

    const w = frame.widthPx;
    const h = frame.heightPx;
    if (w <= 0 || h <= 0) return;

    const dpr = window.devicePixelRatio || 1;
    if (w !== this.paintedW || h !== this.paintedH) {
      this.paintedW = w;
      this.paintedH = h;
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
      canvas.style.width = `${w}px`;
      canvas.style.height = `${h}px`;
    }

    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const ox = this.panOffsetPx;
    const rulerH = frame.rulerHeightPx;
    const lanesH = h - rulerH;
    const laneCount = frame.laneCount || TIMELINE_CHANNEL_LABELS.length;
    const laneH = lanesH / laneCount;
    const t = this.theme;

    ctx.fillStyle = t.bgElevated;
    ctx.fillRect(0, 0, w, h);
    ctx.fillStyle = t.bgMuted;
    ctx.fillRect(0, 0, w, rulerH);

    ctx.strokeStyle = t.border;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, rulerH);
    ctx.lineTo(w, rulerH);
    for (let i = 1; i < laneCount; i += 1) {
      const y = rulerH + i * laneH;
      ctx.moveTo(0, y);
      ctx.lineTo(w, y);
    }
    ctx.stroke();

    ctx.fillStyle = t.textSubtle;
    ctx.font = "500 10px system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    for (const tick of frame.ticks) {
      const x = tick.xPx + ox;
      if (x < -60 || x > w + 60) continue;
      ctx.strokeStyle = t.borderStrong;
      ctx.beginPath();
      ctx.moveTo(x, rulerH - 6);
      ctx.lineTo(x, rulerH);
      ctx.stroke();
      ctx.fillText(tick.label, x, 4);
    }

    ctx.textAlign = "left";
    ctx.textBaseline = "middle";
    ctx.font = "500 11px system-ui, sans-serif";
    for (const clip of frame.clips) {
      const y0 = rulerH + clip.laneIndex * laneH;
      const barTop = y0 + LANE_PAD_Y;
      const barH = Math.max(4, laneH - LANE_PAD_Y * 2);
      const left = clip.leftPx + ox;
      if (left + clip.widthPx < -24 || left > w + 24) continue;

      ctx.fillStyle = TIMELINE_CLIP_FILL[clip.channel] ?? "#888";
      ctx.fillRect(left, barTop, clip.widthPx, barH);

      if (clip.label && clip.widthPx >= MIN_LABEL_PX) {
        ctx.fillStyle = "#1a1a1a";
        ctx.save();
        ctx.beginPath();
        ctx.rect(left + 3, barTop, clip.widthPx - 6, barH);
        ctx.clip();
        ctx.fillText(clip.label, left + 5, barTop + barH / 2);
        ctx.restore();
      }
    }

    const nowX = nowLinePx(frame, nowMs) + ox;
    if (nowX >= -2 && nowX <= w + 2) {
      ctx.fillStyle = t.accent;
      ctx.fillRect(nowX - 1, 0, 2, h);
      ctx.beginPath();
      ctx.moveTo(nowX, 0);
      ctx.lineTo(nowX - 5, 8);
      ctx.lineTo(nowX + 5, 8);
      ctx.closePath();
      ctx.fill();
    }
  }

  hitTest(x: number, y: number): string | null {
    const frame = this.frame;
    if (!frame) return null;
    const xx = x - this.panOffsetPx;

    if (y <= frame.rulerHeightPx) return null;
    const lanesH = frame.heightPx - frame.rulerHeightPx;
    if (lanesH <= 0) return null;

    const laneCount = frame.laneCount || TIMELINE_CHANNEL_LABELS.length;
    const laneH = lanesH / laneCount;
    const laneIdx = Math.floor((y - frame.rulerHeightPx) / laneH);
    if (laneIdx < 0 || laneIdx >= laneCount) return null;

    const barTop = frame.rulerHeightPx + laneIdx * laneH + LANE_PAD_Y;
    const barH = Math.max(4, laneH - LANE_PAD_Y * 2);
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
