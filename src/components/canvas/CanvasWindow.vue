<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import type { CanvasItemRect } from "../../composables/useCanvasLayout";
import { snapGrid } from "../../composables/useCanvasLayout";

const props = defineProps<{
  id: string;
  rect: CanvasItemRect;       // display rect из computedRects
  storedH: number;            // base h из stored (для min-height)
  editMode: boolean;
  minW?: number;
  minH?: number;
  locked?: boolean;
}>();

const emit = defineEmits<{
  (e: "update:rect", rect: CanvasItemRect): void;
  /** Drag/resize завершён — сохраняем resolved позиции соседей. */
  (e: "commit"): void;
  /** ResizeObserver сообщает фактическую высоту (не меняет stored). */
  (e: "actual-height", h: number): void;
  (e: "activate"): void;
}>();

const MIN_W = computed(() => props.minW ?? 80);
const MIN_H = computed(() => props.minH ?? 48);

// ── Drag / resize ──────────────────────────────────────────────
type ResizeDir = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";
type DragType = "move" | ResizeDir;

let drag: { sx: number; sy: number; orig: CanvasItemRect; type: DragType } | null = null;

function startDrag(e: PointerEvent, type: DragType) {
  if (!props.editMode) return;
  if (props.locked && type === "move") return;
  e.preventDefault();
  e.stopPropagation();
  drag = { sx: e.clientX, sy: e.clientY, orig: { ...props.rect }, type };
  (e.target as Element).setPointerCapture(e.pointerId);
  emit("activate");
}

function calcRect(e: PointerEvent): CanvasItemRect {
  if (!drag) return props.rect;
  const dx = e.clientX - drag.sx;
  const dy = e.clientY - drag.sy;
  const o = drag.orig;
  let { x, y, w, h, z, floating } = o;

  if (drag.type === "move") {
    x = snapGrid(Math.max(0, o.x + dx));
    y = snapGrid(Math.max(0, o.y + dy));
  } else {
    const dir = drag.type;
    if (dir.includes("e")) w = snapGrid(Math.max(MIN_W.value, o.w + dx));
    if (dir.includes("s")) h = snapGrid(Math.max(MIN_H.value, o.h + dy));
    if (dir.includes("w")) { w = snapGrid(Math.max(MIN_W.value, o.w - dx)); x = o.x + o.w - w; }
    if (dir.includes("n")) { h = snapGrid(Math.max(MIN_H.value, o.h - dy)); y = o.y + o.h - h; }
  }
  return { x, y, w, h, z, floating };
}

function onPointerMove(e: PointerEvent) {
  if (!drag) return;
  emit("update:rect", calcRect(e));
}

function onPointerUp(e: PointerEvent) {
  if (!drag) return;
  emit("update:rect", calcRect(e));
  drag = null;
  emit("commit");
}

// ── ResizeObserver: только сообщаем высоту, не трогаем stored ─
const rootRef = ref<HTMLElement | null>(null);
let ro: ResizeObserver | null = null;
let lastReportedH = 0;

onMounted(() => {
  if (!rootRef.value) return;
  ro = new ResizeObserver((entries) => {
    if (drag) return; // во время ручного resize не мешаем
    const entry = entries[0];
    if (!entry) return;
    const h = Math.ceil(
      entry.borderBoxSize[0]?.blockSize ?? entry.contentRect.height,
    );
    if (h !== lastReportedH) {
      lastReportedH = h;
      emit("actual-height", h);
    }
  });
  ro.observe(rootRef.value);
});

onBeforeUnmount(() => {
  ro?.disconnect();
  drag = null;
});

// ── Style ──────────────────────────────────────────────────────
// x/y/w из computedRects (с учётом выталкивания соседями).
// Высота: min-height = storedH (пользовательский минимум),
// но если computedRects.h больше (из-за роста соседей) — используем его.
// Контент может делать элемент ещё выше через min-height.
const windowStyle = computed(() => ({
  left: `${props.rect.x}px`,
  top: `${props.rect.y}px`,
  width: `${props.rect.w}px`,
  minHeight: `${Math.max(props.storedH, props.rect.h)}px`,
  zIndex: props.rect.z,
}));
</script>

<template>
  <div
    ref="rootRef"
    class="cw"
    :class="{ 'cw--edit': editMode }"
    :style="windowStyle"
    @pointerdown="emit('activate')"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
  >
    <div
      v-if="editMode"
      class="cw-bar"
      :class="{ 'cw-bar--locked': locked }"
      @pointerdown.stop="startDrag($event, 'move')"
    >
      <span class="cw-bar-label">{{ id }}</span>
      <svg v-if="!locked" class="cw-drag-icon" viewBox="0 0 10 10">
        <circle cx="3" cy="3" r="1" fill="currentColor"/>
        <circle cx="7" cy="3" r="1" fill="currentColor"/>
        <circle cx="3" cy="7" r="1" fill="currentColor"/>
        <circle cx="7" cy="7" r="1" fill="currentColor"/>
      </svg>
      <svg v-else class="cw-drag-icon" viewBox="0 0 10 10">
        <rect x="2" y="4" width="6" height="5" rx="1" stroke="currentColor" stroke-width="1.2" fill="none"/>
        <path d="M3.5 4V3a1.5 1.5 0 1 1 3 0v1" stroke="currentColor" stroke-width="1.2" fill="none"/>
      </svg>
    </div>

    <div class="cw-content"><slot /></div>

    <template v-if="editMode && !locked">
      <div class="cw-h cw-n"  @pointerdown.stop="startDrag($event, 'n')" />
      <div class="cw-h cw-s"  @pointerdown.stop="startDrag($event, 's')" />
      <div class="cw-h cw-e"  @pointerdown.stop="startDrag($event, 'e')" />
      <div class="cw-h cw-w"  @pointerdown.stop="startDrag($event, 'w')" />
      <div class="cw-h cw-ne" @pointerdown.stop="startDrag($event, 'ne')" />
      <div class="cw-h cw-nw" @pointerdown.stop="startDrag($event, 'nw')" />
      <div class="cw-h cw-se" @pointerdown.stop="startDrag($event, 'se')" />
      <div class="cw-h cw-sw" @pointerdown.stop="startDrag($event, 'sw')" />
    </template>
  </div>
</template>

<style scoped>
.cw {
  position: absolute;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
}
.cw--edit {
  border: 1.5px dashed var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-bg-elevated);
  box-shadow: 0 2px 8px rgba(0,0,0,.12);
}
.cw-bar {
  display: flex; align-items: center; gap: 0.3rem;
  padding: 0 0.5rem; height: 22px; flex-shrink: 0;
  cursor: grab; user-select: none;
  border-bottom: 1px solid var(--color-border);
  background: var(--color-bg-muted);
  border-radius: var(--radius-md) var(--radius-md) 0 0;
}
.cw-bar:active { cursor: grabbing; }
.cw-bar--locked { cursor: default; }
.cw-bar-label {
  font-size: 0.65rem; color: var(--color-text-muted);
  flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.cw-drag-icon { width: 10px; height: 10px; color: var(--color-text-subtle); flex-shrink: 0; }
.cw-content {
  flex: 1; min-height: 0;
  display: flex; align-items: flex-start; justify-content: flex-start;
}
.cw-h { position: absolute; z-index: 10; }
.cw-n  { top: -4px;    left: 8px;   right: 8px;   height: 8px;  cursor: n-resize; }
.cw-s  { bottom: -4px; left: 8px;   right: 8px;   height: 8px;  cursor: s-resize; }
.cw-e  { right: -4px;  top: 8px;    bottom: 8px;  width: 8px;   cursor: e-resize; }
.cw-w  { left: -4px;   top: 8px;    bottom: 8px;  width: 8px;   cursor: w-resize; }
.cw-nw { top: -4px;    left: -4px;  width: 12px;  height: 12px; cursor: nw-resize; }
.cw-ne { top: -4px;    right: -4px; width: 12px;  height: 12px; cursor: ne-resize; }
.cw-sw { bottom: -4px; left: -4px;  width: 12px;  height: 12px; cursor: sw-resize; }
.cw-se { bottom: -4px; right: -4px; width: 12px;  height: 12px; cursor: se-resize; }
.cw--edit .cw-ne, .cw--edit .cw-nw, .cw--edit .cw-se, .cw--edit .cw-sw {
  background: var(--color-accent, #3b82f6); border-radius: 2px; opacity: 0.7;
}
.cw--edit .cw-ne:hover, .cw--edit .cw-nw:hover,
.cw--edit .cw-se:hover, .cw--edit .cw-sw:hover { opacity: 1; }
</style>
