<script setup lang="ts">
import {
  onMounted,
  onUnmounted,
  ref,
  useTemplateRef,
  watch,
} from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useProject } from "../../composables/useProject";
import { activeTabId } from "../../composables/useTabState";
import {
  TIMELINE_CHANNEL_LABELS,
  ensureTimelineClipsLoaded,
  ensureTimelineListeners,
  timelineRenderer,
  useProjectTimeline,
} from "../../composables/useProjectTimeline";

defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const { hasOpenProject } = useProject();
const { spanLabel, loading, error: timelineError } = useProjectTimeline();

const trackRef = useTemplateRef<HTMLElement>("trackRef");
const canvasRef = useTemplateRef<HTMLCanvasElement>("canvasRef");

const dragging = ref(false);
const dragLastX = ref(0);
const panOffsetPx = ref(0);

let resizeObs: ResizeObserver | null = null;
let nowTimer: ReturnType<typeof setInterval> | null = null;
let lastTrackWidth = 0;
let lastTrackHeight = 0;
let lastTooltip = "";

function trackSize(): { w: number; h: number } {
  const el = trackRef.value;
  return { w: el?.clientWidth ?? 0, h: el?.clientHeight ?? 0 };
}

function syncTrackSize(): void {
  const { w, h } = trackSize();
  if (w <= 0 || h <= 0) return;
  if (w === lastTrackWidth && h === lastTrackHeight) return;
  lastTrackWidth = w;
  lastTrackHeight = h;
  timelineRenderer.setSize(w, h);
}

function onWheel(e: WheelEvent): void {
  e.preventDefault();
  const rect = trackRef.value?.getBoundingClientRect();
  const clientX = rect ? e.clientX - rect.left : e.clientX;
  timelineRenderer.applyWheel(
    clientX,
    e.deltaY,
    e.deltaX,
    e.shiftKey && !e.ctrlKey && !e.metaKey,
  );
}

function onPointerDown(e: PointerEvent): void {
  if (e.button !== 0) return;
  dragging.value = true;
  dragLastX.value = e.clientX;
  panOffsetPx.value = 0;
  timelineRenderer.setPanOffset(0);
  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
}

function onPointerMove(e: PointerEvent): void {
  if (!dragging.value) return;
  panOffsetPx.value += e.clientX - dragLastX.value;
  dragLastX.value = e.clientX;
  timelineRenderer.setPanOffset(panOffsetPx.value);
  timelineRenderer.paint();
}

function onPointerUp(e: PointerEvent): void {
  if (!dragging.value) return;
  dragging.value = false;
  const dx = panOffsetPx.value;
  panOffsetPx.value = 0;
  timelineRenderer.commitPan(dx);
  try {
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
  } catch {
    /* already released */
  }
}

function centerOnNow(): void {
  timelineRenderer.centerOnNow();
}

function zoom(factor: number): void {
  const { w } = trackSize();
  timelineRenderer.zoomAt(factor, w > 0 ? w / 2 : 400);
}

function onCanvasMove(e: MouseEvent): void {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const rect = canvas.getBoundingClientRect();
  const tip = timelineRenderer.hitTest(
    e.clientX - rect.left,
    e.clientY - rect.top,
  );
  const next = tip ?? "";
  if (next === lastTooltip) return;
  lastTooltip = next;
  if (next) canvas.title = next;
  else canvas.removeAttribute("title");
}

function onCanvasLeave(): void {
  lastTooltip = "";
  canvasRef.value?.removeAttribute("title");
}

function startNowTimer(): void {
  if (nowTimer) return;
  nowTimer = setInterval(() => {
    if (timelineRenderer.getFrame()) timelineRenderer.paint();
  }, 1000);
}

function stopNowTimer(): void {
  if (!nowTimer) return;
  clearInterval(nowTimer);
  nowTimer = null;
}

watch(
  activeTabId,
  (id) => {
    if (id === "timeline") {
      void ensureTimelineClipsLoaded().then(syncTrackSize);
      startNowTimer();
    } else {
      stopNowTimer();
    }
  },
  { immediate: true },
);

onMounted(async () => {
  await ensureTimelineListeners();
  const canvas = canvasRef.value;
  const track = trackRef.value;
  if (canvas) {
    timelineRenderer.attach(canvas, (label) => {
      spanLabel.value = label;
    });
  }
  if (track) {
    lastTrackWidth = track.clientWidth;
    lastTrackHeight = track.clientHeight;
    resizeObs = new ResizeObserver(syncTrackSize);
    resizeObs.observe(track);
    if (activeTabId.value === "timeline") {
      await ensureTimelineClipsLoaded();
      syncTrackSize();
    }
  }
});

onUnmounted(() => {
  stopNowTimer();
  resizeObs?.disconnect();
  timelineRenderer.detach();
});
</script>

<template>
  <section class="tl">
    <p v-if="!hasOpenProject" class="tl-empty">Откройте проект.</p>
    <p v-else-if="timelineError" class="tl-err">{{ timelineError }}</p>

    <template v-else>
      <div class="tl-toolbar">
        <button type="button" class="btn-clear btn-clear--mini" title="Уменьшить" @click="zoom(1.25)">
          −
        </button>
        <span class="tl-span">{{ spanLabel }}</span>
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
            <div v-for="ch in TIMELINE_CHANNEL_LABELS" :key="ch.id" class="tl-label">
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
          <canvas
            ref="canvasRef"
            class="tl-canvas"
            @mousemove="onCanvasMove"
            @mouseleave="onCanvasLeave"
          />
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
  height: 30px;
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
  min-height: 0;
  overflow: hidden;
  cursor: grab;
  background: var(--color-bg-elevated);
  touch-action: none;
}

.tl-track--dragging {
  cursor: grabbing;
}

.tl-canvas {
  position: absolute;
  inset: 0;
  display: block;
}
</style>
