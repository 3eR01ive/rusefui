<script setup lang="ts">
import {
  computed,
  onMounted,
  onUnmounted,
  ref,
  useTemplateRef,
} from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useProject } from "../../composables/useProject";
import {
  TIMELINE_CHANNELS,
  basename,
  clipEndMs,
  formatSpanMs,
  formatTimelineMs,
  initProjectTimeline,
  useProjectTimeline,
  type ProjectTimelineClip,
  type TimelineChannel,
} from "../../composables/useProjectTimeline";

defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

void initProjectTimeline();

const { hasOpenProject } = useProject();
const { clips, loading, error } = useProjectTimeline();

const trackRef = useTemplateRef<HTMLElement>("trackRef");
const trackWidth = ref(800);

/** Центр окна на шкале (Unix ms). «Сейчас» по умолчанию в центре. */
const viewCenterMs = ref(Date.now());
/** Видимый интервал времени. */
const spanMs = ref(3_600_000);
const nowMs = ref(Date.now());

let nowTimer: ReturnType<typeof setInterval> | null = null;
let resizeObs: ResizeObserver | null = null;

onMounted(() => {
  nowTimer = setInterval(() => {
    nowMs.value = Date.now();
  }, 1000);

  const el = trackRef.value;
  if (el) {
    trackWidth.value = el.clientWidth;
    resizeObs = new ResizeObserver(() => {
      if (trackRef.value) trackWidth.value = trackRef.value.clientWidth;
    });
    resizeObs.observe(el);
  }
});

onUnmounted(() => {
  if (nowTimer) clearInterval(nowTimer);
  resizeObs?.disconnect();
});

const dragging = ref(false);
const dragLastX = ref(0);

const pxPerMs = computed(() => trackWidth.value / spanMs.value);

function timeToX(ms: number): number {
  return (ms - viewCenterMs.value) * pxPerMs.value + trackWidth.value / 2;
}

function xToTime(x: number): number {
  return viewCenterMs.value + (x - trackWidth.value / 2) / pxPerMs.value;
}

function clipsForChannel(channel: TimelineChannel): ProjectTimelineClip[] {
  return clips.value.filter((c) => c.channel === channel);
}

function clipStyle(clip: ProjectTimelineClip): Record<string, string> {
  const end = clipEndMs(clip, nowMs.value);
  const left = timeToX(clip.startMs);
  const right = timeToX(end);
  const width = Math.max(6, right - left);
  return {
    left: `${left}px`,
    width: `${width}px`,
  };
}

function clipLabel(clip: ProjectTimelineClip): string {
  return clip.label || basename(clip.record.path);
}

function clipTitle(clip: ProjectTimelineClip): string {
  const end = clip.endMs ? formatTimelineMs(clip.endMs) : "…сейчас";
  return `${clipLabel(clip)}\n${formatTimelineMs(clip.startMs)} → ${end}`;
}

const rulerTicks = computed(() => {
  const w = trackWidth.value;
  if (w <= 0) return [];
  const t0 = xToTime(0);
  const t1 = xToTime(w);
  const span = t1 - t0;
  const rough = span / 8;
  const step =
    rough >= 3_600_000
      ? 3_600_000
      : rough >= 900_000
        ? 900_000
        : rough >= 300_000
          ? 300_000
          : rough >= 60_000
            ? 60_000
            : rough >= 15_000
              ? 15_000
              : 5_000;
  const start = Math.floor(t0 / step) * step;
  const out: { x: number; label: string }[] = [];
  for (let t = start; t <= t1 + step; t += step) {
    const x = timeToX(t);
    if (x < -40 || x > w + 40) continue;
    out.push({ x, label: formatTimelineMs(t) });
  }
  return out;
});

const nowX = computed(() => timeToX(nowMs.value));

function zoomAt(clientX: number, factor: number): void {
  const el = trackRef.value;
  if (!el || factor <= 0) return;
  const rect = el.getBoundingClientRect();
  const x = clientX - rect.left;
  const anchorMs = xToTime(x);
  const nextSpan = clampSpan(spanMs.value * factor);
  const nextPxPerMs = trackWidth.value / nextSpan;
  viewCenterMs.value = anchorMs - (x - trackWidth.value / 2) / nextPxPerMs;
  spanMs.value = nextSpan;
}

function onWheel(e: WheelEvent): void {
  e.preventDefault();
  if (e.shiftKey && !e.ctrlKey && !e.metaKey) {
    const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
    viewCenterMs.value += delta / pxPerMs.value;
    return;
  }
  const factor = Math.pow(1.0015, -e.deltaY);
  zoomAt(e.clientX, factor);
}

function onPointerDown(e: PointerEvent): void {
  if (e.button !== 0) return;
  dragging.value = true;
  dragLastX.value = e.clientX;
  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
}

function onPointerMove(e: PointerEvent): void {
  if (!dragging.value) return;
  const dx = e.clientX - dragLastX.value;
  dragLastX.value = e.clientX;
  viewCenterMs.value -= dx / pxPerMs.value;
}

function onPointerUp(e: PointerEvent): void {
  dragging.value = false;
  try {
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
  } catch {
    /* already released */
  }
}

function centerOnNow(): void {
  viewCenterMs.value = nowMs.value;
}

function zoom(factor: number): void {
  const el = trackRef.value;
  if (!el) {
    spanMs.value = clampSpan(spanMs.value * factor);
    return;
  }
  const rect = el.getBoundingClientRect();
  zoomAt(rect.left + rect.width / 2, factor);
}

function clampSpan(v: number): number {
  return Math.min(14 * 86_400_000, Math.max(30_000, v));
}
</script>

<template>
  <section class="tl">
    <p v-if="!hasOpenProject" class="tl-empty">Откройте проект.</p>
    <p v-else-if="error" class="tl-err">{{ error }}</p>

    <template v-else>
      <div class="tl-toolbar">
        <button type="button" class="btn-clear btn-clear--mini" title="Уменьшить" @click="zoom(1.25)">
          −
        </button>
        <span class="tl-span">{{ formatSpanMs(spanMs) }}</span>
        <button type="button" class="btn-clear btn-clear--mini" title="Увеличить" @click="zoom(0.8)">
          +
        </button>
        <button type="button" class="btn-clear btn-clear--mini" @click="centerOnNow">Сейчас</button>
        <span v-if="loading" class="tl-hint">…</span>
      </div>

      <div class="tl-editor">
        <div class="tl-labels">
          <div class="tl-label-ruler" aria-hidden="true" />
          <div class="tl-labels-body">
            <div v-for="ch in TIMELINE_CHANNELS" :key="ch.id" class="tl-label">
              {{ ch.title }}
            </div>
          </div>
        </div>

        <div
          ref="trackRef"
          class="tl-track"
          :class="{ 'tl-track--dragging': dragging }"
          @wheel.prevent="onWheel"
          @pointerdown="onPointerDown"
          @pointermove="onPointerMove"
          @pointerup="onPointerUp"
          @pointercancel="onPointerUp"
        >
          <div class="tl-ruler">
            <span
              v-for="(tick, i) in rulerTicks"
              :key="i"
              class="tl-tick"
              :style="{ left: `${tick.x}px` }"
            >
              {{ tick.label }}
            </span>
          </div>

          <div class="tl-now" :style="{ left: `${nowX}px` }" title="Сейчас" />

          <div class="tl-lanes">
            <div v-for="ch in TIMELINE_CHANNELS" :key="ch.id" class="tl-lane">
              <div
                v-for="clip in clipsForChannel(ch.id)"
                :key="clip.id"
                class="tl-clip"
                :data-channel="ch.id"
                :style="clipStyle(clip)"
                :title="clipTitle(clip)"
              >
                <span class="tl-clip-label">{{ clipLabel(clip) }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </template>
  </section>
</template>

<style scoped>
.tl {
  display: flex;
  flex-direction: column;
  flex: 1;
  height: 100%;
  min-height: calc(100vh - var(--app-header-h, 5.5rem) - var(--footer-height) - 1.25rem);
  gap: 0.5rem;
  user-select: none;
  box-sizing: border-box;
}

.tl-empty {
  margin: 0;
  padding: 0.75rem 1rem;
  border-radius: var(--radius-md);
  color: var(--color-text-muted);
  background: var(--color-bg-muted);
  font-size: 0.88rem;
}

.tl-err {
  margin: 0;
  padding: 0.75rem 1rem;
  border-radius: var(--radius-md);
  color: var(--color-error);
  background: var(--color-error-bg);
  border-left: 3px solid var(--color-accent);
  font-size: 0.88rem;
}

.tl-toolbar {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  flex-shrink: 0;
  padding: 0.15rem 0;
}

.btn-clear {
  padding: 0.3rem 0.65rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-gray);
  font-size: 0.78rem;
  cursor: pointer;
}

.btn-clear:hover {
  background: var(--color-bg-muted);
}

.btn-clear--mini {
  min-width: 2rem;
  padding: 0.25rem 0.5rem;
  text-align: center;
}

.tl-span {
  font-size: 0.78rem;
  color: var(--color-text-muted);
  font-variant-numeric: tabular-nums;
  min-width: 4rem;
  text-align: center;
}

.tl-hint {
  margin-left: auto;
  font-size: 0.75rem;
  color: var(--color-text-subtle);
}

.tl-editor {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 5.75rem 1fr;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--color-bg-elevated);
  box-shadow: var(--shadow-card);
}

.tl-labels {
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-right: 1px solid var(--color-border);
  background: var(--color-bg-muted);
}

.tl-label-ruler {
  height: 1.85rem;
  flex-shrink: 0;
  border-bottom: 1px solid var(--color-border);
}

.tl-labels-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.tl-label {
  flex: 1;
  min-height: 2.5rem;
  display: flex;
  align-items: center;
  padding: 0 0.55rem;
  font-size: 0.72rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-text-muted);
  border-bottom: 1px solid var(--color-border);
}

.tl-label:last-child {
  border-bottom: none;
}

.tl-track {
  position: relative;
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
  cursor: grab;
  background: var(--color-bg-elevated);
}

.tl-track--dragging {
  cursor: grabbing;
}

.tl-ruler {
  position: relative;
  flex-shrink: 0;
  height: 1.85rem;
  border-bottom: 1px solid var(--color-border);
  background: var(--color-bg-muted);
}

.tl-tick {
  position: absolute;
  top: 0;
  transform: translateX(-50%);
  padding-top: 0.25rem;
  font-size: 0.65rem;
  color: var(--color-text-subtle);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  pointer-events: none;
}

.tl-tick::before {
  content: "";
  position: absolute;
  bottom: 0;
  left: 50%;
  width: 1px;
  height: 0.35rem;
  background: var(--color-border-strong);
  transform: translateX(-50%);
}

.tl-lanes {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.tl-now {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  margin-left: -1px;
  background: var(--color-accent);
  box-shadow: 0 0 6px color-mix(in srgb, var(--color-accent) 45%, transparent);
  z-index: 4;
  pointer-events: none;
}

.tl-now::after {
  content: "";
  position: absolute;
  top: 0;
  left: 50%;
  transform: translateX(-50%);
  border: 5px solid transparent;
  border-top-color: var(--color-accent);
}

.tl-lane {
  position: relative;
  flex: 1;
  min-height: 2.5rem;
  border-bottom: 1px solid var(--color-border);
}

.tl-lane:last-child {
  border-bottom: none;
}

.tl-lane::before {
  content: "";
  position: absolute;
  inset: 0;
  background: repeating-linear-gradient(
    90deg,
    transparent,
    transparent 59px,
    color-mix(in srgb, var(--color-border) 55%, transparent) 59px,
    color-mix(in srgb, var(--color-border) 55%, transparent) 60px
  );
  pointer-events: none;
}

.tl-clip {
  position: absolute;
  top: 0.4rem;
  bottom: 0.4rem;
  min-width: 8px;
  border-radius: var(--radius-sm);
  border: 1px solid transparent;
  overflow: hidden;
  z-index: 2;
  box-shadow: 0 1px 2px color-mix(in srgb, var(--color-text) 8%, transparent);
}

.tl-clip[data-channel="logs"] {
  background: linear-gradient(
    180deg,
    color-mix(in srgb, #5b9fd4 88%, white),
    color-mix(in srgb, #5b9fd4 62%, white)
  );
  border-color: color-mix(in srgb, #5b9fd4 70%, var(--color-border));
}

.tl-clip[data-channel="trigger"] {
  background: linear-gradient(
    180deg,
    color-mix(in srgb, var(--color-accent) 82%, white),
    color-mix(in srgb, var(--color-accent-muted) 75%, white)
  );
  border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
}

.tl-clip[data-channel="spectrogram"] {
  background: linear-gradient(
    180deg,
    color-mix(in srgb, #9b6fd4 85%, white),
    color-mix(in srgb, #9b6fd4 58%, white)
  );
  border-color: color-mix(in srgb, #9b6fd4 65%, var(--color-border));
}

.tl-clip[data-channel="runs"] {
  background: linear-gradient(
    180deg,
    var(--color-gray-light),
    color-mix(in srgb, var(--color-gray-light) 70%, var(--color-bg-muted))
  );
  border-color: var(--color-border-strong);
}

.tl-clip-label {
  display: block;
  padding: 0.2rem 0.4rem;
  font-size: 0.68rem;
  font-weight: 500;
  color: var(--color-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  pointer-events: none;
}
</style>
