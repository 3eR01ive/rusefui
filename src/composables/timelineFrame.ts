/** Viewport + сборка кадра таймлайна на клиенте (клипы статичны, IPC не нужен). */

export const TIMELINE_CHANNEL_LABELS = [
  { id: "logs", title: "Логи" },
  { id: "trigger", title: "Триггер" },
  { id: "spectrogram", title: "Спектрограмма" },
  { id: "runs", title: "Прогоны" },
] as const;

export interface ProjectTimelineClip {
  id: string;
  channel: string;
  startMs: number;
  endMs?: number | null;
  record: { path: string; kind?: string | null };
  label?: string | null;
}

export interface TimelineTickDraw {
  xPx: number;
  label: string;
}

export interface TimelineClipDraw {
  id: string;
  channel: string;
  laneIndex: number;
  leftPx: number;
  widthPx: number;
  label: string;
  tooltip: string;
}

export interface TimelineFrame {
  widthPx: number;
  heightPx: number;
  rulerHeightPx: number;
  laneCount: number;
  ticks: TimelineTickDraw[];
  clips: TimelineClipDraw[];
  nowXPx: number;
  spanLabel: string;
  viewCenterMs: number;
  spanMs: number;
}

export const RULER_HEIGHT_PX = 30;
const MIN_SPAN_MS = 30_000;
const MAX_SPAN_MS = 14 * 86_400_000;
export const DEFAULT_SPAN_MS = 3_600_000;
const MAX_TICKS = 14;
const MIN_CLIP_WIDTH_PX = 6;
const CULL_MARGIN_PX = 24;
const MIN_LABEL_PX = 52;
const MAX_LABELED_CLIPS = 80;

const TICK_STEPS_MS = [5_000, 15_000, 60_000, 300_000, 900_000, 3_600_000, 86_400_000];
const CHANNEL_ORDER = TIMELINE_CHANNEL_LABELS.map((c) => c.id);

function pxPerMs(width: number, span: number): number {
  return width / span;
}

function timeToX(ms: number, center: number, width: number, span: number): number {
  return (ms - center) * pxPerMs(width, span) + width / 2;
}

function xToTime(x: number, center: number, width: number, span: number): number {
  return center + (x - width / 2) / pxPerMs(width, span);
}

function clampSpan(span: number): number {
  return Math.min(MAX_SPAN_MS, Math.max(MIN_SPAN_MS, span));
}

function pickTickStep(spanMs: number): number {
  const rough = spanMs / 8;
  for (const step of TICK_STEPS_MS) {
    if (step >= rough) return step;
  }
  return TICK_STEPS_MS[TICK_STEPS_MS.length - 1]!;
}

function ymdFromDays(days: number): { y: number; m: number; d: number } {
  const z = days + 719_468;
  const era = Math.trunc(z >= 0 ? z : z - 146_096) / 146_097;
  const doe = z - era * 146_097;
  const yoe = Math.trunc((doe - Math.trunc(doe / 1_460) + Math.trunc(doe / 36_524) - Math.trunc(doe / 146_096)) / 365);
  let y = yoe + era * 400;
  const doy = doe - (365 * yoe + Math.trunc(yoe / 4) - Math.trunc(yoe / 100));
  const mp = Math.trunc((5 * doy + 2) / 153);
  const d = doy - Math.trunc((153 * mp + 2) / 5) + 1;
  let m = mp < 10 ? mp + 3 : mp - 9;
  if (m <= 2) y += 1;
  return { y, m, d };
}

function formatTickMs(ms: number): string {
  const sec = Math.trunc(ms / 1000);
  const ss = sec % 60;
  const mm = Math.trunc(sec / 60) % 60;
  const hh = Math.trunc(sec / 3600) % 24;
  const { m: mon, d: day } = ymdFromDays(Math.trunc(sec / 86_400));
  if (ss !== 0 || mm !== 0 || hh !== 0) {
    return `${String(mon).padStart(2, "0")}/${String(day).padStart(2, "0")} ${hh}:${String(mm).padStart(2, "0")}:${String(ss).padStart(2, "0")}`;
  }
  return `${String(mon).padStart(2, "0")}/${String(day).padStart(2, "0")}`;
}

export function formatSpanMs(spanMs: number): string {
  const sec = spanMs / 1000;
  if (sec < 120) return `${Math.round(sec)} с`;
  if (sec < 7200) return `${Math.round(sec / 60)} мин`;
  if (sec < 172_800) return `${(sec / 3600).toFixed(1)} ч`;
  return `${(sec / 86_400).toFixed(1)} д`;
}

function basename(path: string): string {
  const parts = path.split(/[/\\]/);
  const last = parts[parts.length - 1];
  return last && last.length > 0 ? last : path;
}

function clipDisplayLabel(clip: ProjectTimelineClip): string {
  if (clip.label && clip.label.length > 0) return clip.label;
  return basename(clip.record.path);
}

function clipEndMs(clip: ProjectTimelineClip): number {
  if (clip.endMs != null && clip.endMs > 0) return clip.endMs;
  return clip.startMs + 120_000;
}

function clipTooltip(clip: ProjectTimelineClip, endMs: number): string {
  const label = clipDisplayLabel(clip);
  return `${label}\n${formatTickMs(clip.startMs)} → ${formatTickMs(endMs)}`;
}

function laneIndex(channel: string): number {
  const idx = CHANNEL_ORDER.indexOf(channel as (typeof CHANNEL_ORDER)[number]);
  return idx >= 0 ? idx : 0;
}

function buildTicks(center: number, width: number, span: number): TimelineTickDraw[] {
  if (width <= 0 || span <= 0) return [];
  const ppm = pxPerMs(width, span);
  const half = width / 2;
  const t0 = center + (0 - half) / ppm;
  const t1 = center + (width - half) / ppm;
  const step = pickTickStep(t1 - t0);
  let t = Math.floor(t0 / step) * step;
  const tEnd = Math.trunc(t1) + step;
  const out: TimelineTickDraw[] = [];
  while (t <= tEnd && out.length < MAX_TICKS) {
    const x = timeToX(t, center, width, span);
    if (x >= -48 && x <= width + 48) {
      out.push({ xPx: x, label: formatTickMs(t) });
    }
    t += step;
  }
  return out;
}

function buildClips(
  clips: ProjectTimelineClip[],
  center: number,
  width: number,
  span: number,
): TimelineClipDraw[] {
  if (width <= 0 || span <= 0) return [];
  const drawLabels = clips.length <= MAX_LABELED_CLIPS;
  const out: TimelineClipDraw[] = [];
  for (const clip of clips) {
    const end = clipEndMs(clip);
    const left = timeToX(clip.startMs, center, width, span);
    const right = timeToX(end, center, width, span);
    if (right < -CULL_MARGIN_PX || left > width + CULL_MARGIN_PX) continue;
    const clipWidth = Math.max(MIN_CLIP_WIDTH_PX, right - left);
    const label = clipDisplayLabel(clip);
    out.push({
      id: clip.id,
      channel: clip.channel,
      laneIndex: laneIndex(clip.channel),
      leftPx: left,
      widthPx: clipWidth,
      label: drawLabels && clipWidth >= MIN_LABEL_PX ? label : "",
      tooltip: clipTooltip(clip, end),
    });
  }
  return out;
}

export class TimelineFrameBuilder {
  clips: ProjectTimelineClip[] = [];
  viewCenterMs = Date.now();
  spanMs = DEFAULT_SPAN_MS;
  widthPx = 800;
  heightPx = 230;

  reset(): void {
    this.viewCenterMs = Date.now();
    this.spanMs = DEFAULT_SPAN_MS;
  }

  setClips(clips: ProjectTimelineClip[]): void {
    this.clips = clips;
  }

  setSize(widthPx: number, heightPx: number): void {
    if (widthPx > 0 && Number.isFinite(widthPx)) this.widthPx = widthPx;
    if (heightPx > 0 && Number.isFinite(heightPx)) this.heightPx = heightPx;
  }

  panByPx(deltaPx: number): void {
    if (!Number.isFinite(deltaPx) || this.widthPx <= 0 || this.spanMs <= 0) return;
    const ppm = pxPerMs(this.widthPx, this.spanMs);
    this.viewCenterMs = Math.max(0, this.viewCenterMs - deltaPx / ppm);
  }

  zoomAt(anchorX: number, factor: number): void {
    if (factor <= 0 || !Number.isFinite(factor) || this.widthPx <= 0) return;
    const anchorMs = xToTime(anchorX, this.viewCenterMs, this.widthPx, this.spanMs);
    const nextSpan = clampSpan(Math.round(this.spanMs * factor));
    const nextPpm = pxPerMs(this.widthPx, nextSpan);
    this.viewCenterMs = Math.max(
      0,
      anchorMs - (anchorX - this.widthPx / 2) / nextPpm,
    );
    this.spanMs = nextSpan;
  }

  applyWheel(clientX: number, deltaY: number, deltaX: number, shiftPan: boolean): void {
    if (shiftPan) {
      const delta = Math.abs(deltaX) > Math.abs(deltaY) ? deltaX : deltaY;
      this.panByPx(delta);
    } else {
      const factor = 1.0015 ** -deltaY;
      this.zoomAt(clientX, factor);
    }
  }

  centerOnNow(): void {
    this.viewCenterMs = Date.now();
  }

  build(): TimelineFrame {
    const w = this.widthPx;
    const h = this.heightPx;
    const center = this.viewCenterMs;
    const span = this.spanMs;
    const now = Date.now();
    return {
      widthPx: w,
      heightPx: h,
      rulerHeightPx: RULER_HEIGHT_PX,
      laneCount: CHANNEL_ORDER.length,
      ticks: buildTicks(center, w, span),
      clips: buildClips(this.clips, center, w, span),
      nowXPx: timeToX(now, center, w, span),
      spanLabel: formatSpanMs(span),
      viewCenterMs: center,
      spanMs: span,
    };
  }
}
