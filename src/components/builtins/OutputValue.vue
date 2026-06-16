<script setup lang="ts">
import { computed, inject, onMounted, ref, watch, watchEffect } from "vue";
import type { ComponentInstance, ComponentMeta, DataBinding } from "../../core/types";
import { initOutputChannels, useOutputChannels } from "../../composables/useOutputChannels";
import { useTabFrozenDisplay } from "../../composables/useTabActivity";
import { useOutputTimeline } from "../../composables/useOutputTimeline";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const bind = computed(() => props.instance.bind as DataBinding | undefined);
const fieldName = computed(() => bind.value?.field ?? "");
const label = computed(() => String(props.props.label ?? (fieldName.value || "—")));
const unit = computed(() => String(props.props.unit ?? ""));
const decimals = computed(() => Number(props.props.decimals ?? 1));

const displayType = ref<"badge" | "gauge">(
  (props.props.displayType as string) === "gauge" ? "gauge" : "badge",
);
const minVal = ref(Number(props.props.min ?? 0));
const maxVal = ref(Number(props.props.max ?? 100));

interface WarnCondition {
  op: "gt" | "lt" | "gte" | "lte";
  value: number;
  level: "warn" | "danger";
}

function parseWarnings(raw: unknown): WarnCondition[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((item) => {
    if (typeof item !== "object" || !item) return [];
    const r = item as Record<string, unknown>;
    const op = String(r.op ?? "gt");
    const val = Number(r.value ?? 0);
    const level = String(r.level ?? "warn");
    if (!["gt", "lt", "gte", "lte"].includes(op)) return [];
    if (!["warn", "danger"].includes(level)) return [];
    return [{ op, value: val, level } as WarnCondition];
  });
}

const warnings = ref<WarnCondition[]>(parseWarnings(props.props.warnings));

watch(() => props.props, (p) => {
  if (typeof p.displayType === "string")
    displayType.value = p.displayType === "gauge" ? "gauge" : "badge";
  if (p.min != null) minVal.value = Number(p.min);
  if (p.max != null) maxVal.value = Number(p.max);
  if (p.warnings != null) warnings.value = parseWarnings(p.warnings);
});

// Сообщаем CanvasWindow минимальные размеры под тип отображения
const cwReportMinW = inject<((w: number) => void) | undefined>('cwReportMinW', undefined);
const cwReportMinH = inject<((h: number) => void) | undefined>('cwReportMinH', undefined);
watchEffect(() => {
  const isGauge = displayType.value === 'gauge';
  cwReportMinW?.(isGauge ? 128 : 144); // gauge=8rem, badge=9rem
  cwReportMinH?.(isGauge ? 128 : 56);  // gauge=8rem square, badge=~3.5rem
});

const settingsOpen = ref(false);
const newOp = ref<WarnCondition["op"]>("gt");
const newValue = ref(0);
const newLevel = ref<WarnCondition["level"]>("warn");
function addWarning() {
  warnings.value = [...warnings.value, { op: newOp.value, value: newValue.value, level: newLevel.value }];
}
function removeWarning(i: number) {
  warnings.value = warnings.value.filter((_, idx) => idx !== i);
}

// --- Output ---
const { snapshot, getField } = useOutputChannels();
onMounted(() => { void initOutputChannels(); });

const rawValue = computed(() => fieldName.value ? getField(fieldName.value) : null);

const { status: timelineStatus } = useOutputTimeline();
const isLogMode = computed(() => snapshot.value.valuesSource === "logCursor");
const logLoading = computed(() => isLogMode.value && !!timelineStatus.value.fileLoading);

const displayValue = useTabFrozenDisplay(() => {
  const v = rawValue.value;
  if (v === null) return "—";
  if (Number.isInteger(v) && decimals.value === 0) return String(v);
  return v.toFixed(decimals.value);
}, "—");

const stale = computed(
  () => snapshot.value.connected && rawValue.value === null && !!fieldName.value,
);

// --- Warnings ---
function evalCond(w: WarnCondition, v: number): boolean {
  switch (w.op) {
    case "gt": return v > w.value;
    case "lt": return v < w.value;
    case "gte": return v >= w.value;
    case "lte": return v <= w.value;
  }
}

const activeWarn = computed((): WarnCondition | null => {
  const v = rawValue.value;
  if (v === null) return null;
  return (
    warnings.value.find((w) => w.level === "danger" && evalCond(w, v)) ??
    warnings.value.find((w) => w.level === "warn" && evalCond(w, v)) ??
    null
  );
});

const warnClass = computed(() => activeWarn.value?.level ?? "");

// ── Gauge geometry ──────────────────────────────────────────────
// SVG 120×120, center (60,60)
// Sweep 270° starting at 135° (7 o'clock) going clockwise to 45° (5 o'clock)
const CX = 60; const CY = 60;
const R_FACE = 56;     // outer circle
const R_TRACK = 47;   // arc track radius
const R_TICK_OUT = 51;
const R_TICK_MID = 46; // minor tick inner
const R_TICK_IN = 43;  // major tick inner
const R_LABEL = 34;   // tick label radius
const R_NEEDLE = 41;  // needle tip radius
const START_DEG = 135;
const SWEEP = 270;

function toRad(d: number) { return (d * Math.PI) / 180; }

function gaugePt(frac: number, r: number) {
  const a = toRad(START_DEG + frac * SWEEP);
  return { x: CX + r * Math.cos(a), y: CY + r * Math.sin(a) };
}

function gaugeArc(f0: number, f1: number, r: number): string {
  if (Math.abs(f1 - f0) < 0.001) return "";
  const s = gaugePt(f0, r);
  const e = gaugePt(f1, r);
  const large = (f1 - f0) * SWEEP > 180 ? 1 : 0;
  return `M ${s.x.toFixed(2)} ${s.y.toFixed(2)} A ${r} ${r} 0 ${large} 1 ${e.x.toFixed(2)} ${e.y.toFixed(2)}`;
}

const frac = computed(() => {
  const v = rawValue.value;
  if (v === null) return 0;
  const mn = minVal.value; const mx = maxVal.value;
  if (mx <= mn) return 0;
  return Math.max(0, Math.min(1, (v - mn) / (mx - mn)));
});

const trackPath = computed(() => gaugeArc(0, 1, R_TRACK));
const fillPath = computed(() => frac.value > 0.002 ? gaugeArc(0, frac.value, R_TRACK) : "");

const warnZones = computed(() => {
  const mn = minVal.value; const mx = maxVal.value;
  if (mx <= mn) return [];
  return warnings.value.map((w) => {
    const f = Math.max(0, Math.min(1, (w.value - mn) / (mx - mn)));
    const f0 = (w.op === "lt" || w.op === "lte") ? 0 : f;
    const f1 = (w.op === "lt" || w.op === "lte") ? f : 1;
    if (f0 >= f1) return null;
    return { path: gaugeArc(f0, f1, R_TRACK), level: w.level };
  }).filter(Boolean);
});

// Tick marks: 10 divisions = 11 marks; label at 0, 0.25, 0.5, 0.75, 1.0
const ticks = computed(() => {
  const mn = minVal.value; const mx = maxVal.value;
  return Array.from({ length: 11 }, (_, i) => {
    const f = i / 10;
    const major = i % 2 === 0; // 0,2,4,6,8,10 are major
    const a = toRad(START_DEG + f * SWEEP);
    const r1 = major ? R_TICK_IN : R_TICK_MID;
    return {
      x1: (CX + R_TICK_OUT * Math.cos(a)).toFixed(2),
      y1: (CY + R_TICK_OUT * Math.sin(a)).toFixed(2),
      x2: (CX + r1 * Math.cos(a)).toFixed(2),
      y2: (CY + r1 * Math.sin(a)).toFixed(2),
      major,
      // label only at every other major (0%, 50%, 100%) to avoid clutter
      label: i % 5 === 0 ? String(Math.round(mn + f * (mx - mn))) : null,
      lx: (CX + R_LABEL * Math.cos(a)).toFixed(2),
      ly: (CY + R_LABEL * Math.sin(a)).toFixed(2),
    };
  });
});

const needlePt = computed(() => gaugePt(frac.value, R_NEEDLE));
// needle base offset slightly from center for visual
const needleBase = computed(() => gaugePt(frac.value, -8));
</script>

<template>
  <div class="ov" :class="[`ov--${displayType}`, warnClass, { stale, 'log-loading': logLoading }]">

    <!-- ── Badge ── -->
    <template v-if="displayType === 'badge'">
      <div class="ov-row">
        <span class="ov-label">{{ label }}</span>
        <span class="ov-spacer"/>
        <span class="ov-src" :class="{ 'log-src': isLogMode }">
          <span v-if="logLoading" class="ov-spin"/>
          <template v-else>{{ isLogMode ? "log" : "live" }}</template>
        </span>
        <button class="ov-gear" :class="{ active: settingsOpen }" @click="settingsOpen = !settingsOpen">
          <svg viewBox="0 0 12 12" fill="none"><circle cx="6" cy="6" r="4.5" stroke="currentColor" stroke-width="1.1"/><circle cx="6" cy="6" r="1.6" stroke="currentColor" stroke-width="1.1"/></svg>
        </button>
      </div>
      <div class="ov-badge-val">
        <span class="ov-value">{{ displayValue }}</span>
        <span v-if="unit" class="ov-unit">{{ unit }}</span>
      </div>
    </template>

    <!-- ── Gauge ── -->
    <template v-else>
      <div class="ov-gauge-wrap">
        <svg class="ov-gauge" viewBox="0 0 120 120" xmlns="http://www.w3.org/2000/svg">
          <!-- face -->
          <circle :cx="CX" :cy="CY" :r="R_FACE" class="g-face"/>

          <!-- warning zones (behind track) -->
          <path v-for="(z, i) in warnZones" :key="i" :d="z!.path"
            class="g-zone" :class="`g-zone--${z!.level}`"/>

          <!-- track -->
          <path :d="trackPath" class="g-track"/>

          <!-- fill -->
          <path v-if="fillPath" :d="fillPath" class="g-fill" :class="warnClass"/>

          <!-- ticks -->
          <line v-for="(t, i) in ticks" :key="i"
            :x1="t.x1" :y1="t.y1" :x2="t.x2" :y2="t.y2"
            class="g-tick" :class="{ 'g-tick--major': t.major }"/>

          <!-- tick labels -->
          <text v-for="(t, i) in ticks.filter(t => t.label)" :key="`l${i}`"
            :x="t.lx" :y="t.ly" class="g-tick-label" text-anchor="middle" dominant-baseline="middle">
            {{ t.label }}
          </text>

          <!-- needle -->
          <line
            :x1="needleBase.x.toFixed(2)" :y1="needleBase.y.toFixed(2)"
            :x2="needlePt.x.toFixed(2)"  :y2="needlePt.y.toFixed(2)"
            class="g-needle" :class="warnClass"/>
          <circle :cx="CX" :cy="CY" r="5" class="g-hub"/>
          <circle :cx="CX" :cy="CY" r="2.5" class="g-hub-inner"/>

          <!-- label -->
          <text :x="CX" :y="CY - 12" class="g-label" text-anchor="middle">{{ label }}</text>

          <!-- value -->
          <text :x="CX" :y="CY + 16" class="g-value" text-anchor="middle" :class="warnClass">{{ displayValue }}</text>
          <text v-if="unit" :x="CX" :y="CY + 26" class="g-unit" text-anchor="middle">{{ unit }}</text>

          <!-- log/live source -->
          <text :x="CX" :y="114" class="g-src" text-anchor="middle" :class="{ 'g-src--log': isLogMode }">
            {{ logLoading ? '…' : (isLogMode ? 'log' : 'live') }}
          </text>

          <!-- settings gear overlay -->
          <g class="g-gear-btn" @click="settingsOpen = !settingsOpen">
            <circle cx="109" cy="11" r="8" class="g-gear-bg"/>
            <circle cx="109" cy="11" r="4" class="g-gear-ring" :class="{ active: settingsOpen }"/>
            <circle cx="109" cy="11" r="1.5" class="g-gear-dot"/>
          </g>
        </svg>
      </div>
    </template>

    <!-- ── Settings ── -->
    <div v-if="settingsOpen" class="ov-settings">
      <div class="s-row">
        <button class="s-type" :class="{ active: displayType === 'badge' }" @click="displayType = 'badge'">Плашка</button>
        <button class="s-type" :class="{ active: displayType === 'gauge' }" @click="displayType = 'gauge'">Gauge</button>
        <span class="s-spacer"/>
        <span class="s-lbl">Min</span><input v-model.number="minVal" type="number" class="s-num"/>
        <span class="s-lbl">Max</span><input v-model.number="maxVal" type="number" class="s-num"/>
      </div>
      <div v-for="(w, i) in warnings" :key="i" class="s-row">
        <select v-model="w.op" class="s-sel"><option value="gt">&gt;</option><option value="gte">≥</option><option value="lt">&lt;</option><option value="lte">≤</option></select>
        <input v-model.number="w.value" type="number" class="s-num"/>
        <select v-model="w.level" class="s-sel"><option value="warn">warn</option><option value="danger">danger</option></select>
        <button class="s-rm" @click="removeWarning(i)">✕</button>
      </div>
      <div class="s-row">
        <select v-model="newOp" class="s-sel"><option value="gt">&gt;</option><option value="gte">≥</option><option value="lt">&lt;</option><option value="lte">≤</option></select>
        <input v-model.number="newValue" type="number" class="s-num"/>
        <select v-model="newLevel" class="s-sel"><option value="warn">warn</option><option value="danger">danger</option></select>
        <button class="s-add" @click="addWarning">＋</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* ── Root ── */
.ov {
  display: inline-flex;
  flex-direction: column;
  /* Запрет растяжения в любом flex-родителе (RowLayout, SectionLayout и т.д.) */
  flex-grow: 0 !important;
  flex-shrink: 0 !important;
  align-self: flex-start !important;
  border-radius: var(--radius-md);
}

/* Badge: compact card */
.ov--badge {
  min-width: 9rem;
  background: var(--color-bg-muted);
  border: 1px solid var(--color-border);
}
.ov--badge.warn   { border-color: var(--color-warn, #d97706); }
.ov--badge.danger { border-color: var(--color-danger, #dc2626); }
.ov--badge.stale  { border-style: dashed; opacity: 0.8; }

/* Gauge: no rect border — circle draws its own */
.ov--gauge {
  background: transparent !important;
  border: none !important;
  padding: 0;
  min-width: 8rem;
}

.ov.log-loading { animation: ov-pulse 1.2s ease-in-out infinite; }
@keyframes ov-pulse { 0%,100% { opacity:1 } 50% { opacity:.55 } }

/* ── Badge internals ── */
.ov-row {
  display: flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0.28rem 0.4rem;
}
.ov-badge-val {
  display: flex;
  align-items: baseline;
  gap: 0.2rem;
  padding: 0.05rem 0.4rem 0.3rem;
}
.ov-spacer, .s-spacer { flex: 1; }

.ov-label {
  font-size: 0.7rem;
  color: var(--color-text-muted);
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ov-value {
  font-size: 1.05rem;
  font-weight: 700;
  color: var(--color-text);
  font-variant-numeric: tabular-nums;
}
.ov--badge.warn   .ov-value { color: var(--color-warn, #d97706); }
.ov--badge.danger .ov-value { color: var(--color-danger, #dc2626); }

.ov-unit { font-size: 0.68rem; color: var(--color-text-muted); }

.ov-src {
  font-size: 0.58rem;
  color: var(--color-text-subtle);
  background: var(--color-bg);
  border: 1px solid var(--color-border);
  border-radius: 3px;
  padding: 0 0.2em;
  line-height: 1.5;
  display: flex;
  align-items: center;
  gap: 0.15em;
}
.ov-src.log-src { border-color: var(--color-accent, #3b82f6); color: var(--color-accent, #3b82f6); }

.ov-spin {
  display: inline-block;
  width: 0.5rem;
  height: 0.5rem;
  border: 1.5px solid currentColor;
  border-top-color: transparent;
  border-radius: 50%;
  animation: ov-spin 0.7s linear infinite;
}
@keyframes ov-spin { to { transform: rotate(360deg); } }

.ov-gear {
  flex-shrink: 0;
  width: 1rem;
  height: 1rem;
  background: none;
  border: none;
  border-radius: var(--radius-sm);
  cursor: pointer;
  color: var(--color-text-muted);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  opacity: 0;
  transition: opacity 0.1s;
}
.ov:hover .ov-gear, .ov-gear.active { opacity: 1; }
.ov-gear svg { width: 0.6rem; height: 0.6rem; }

/* ── Gauge SVG ── */
.ov-gauge-wrap { line-height: 0; }
.ov-gauge { width: 100%; height: auto; display: block; }

/* face */
.g-face {
  fill: var(--color-bg-muted);
  stroke: var(--color-border);
  stroke-width: 1.5;
  filter: drop-shadow(0 1px 3px rgba(0,0,0,.15));
}

/* track */
.g-track {
  fill: none;
  stroke: var(--color-border);
  stroke-width: 4.5;
  stroke-linecap: round;
  opacity: 0.6;
}

/* warning zones */
.g-zone { fill: none; stroke-width: 4.5; stroke-linecap: round; opacity: 0.35; }
.g-zone--warn   { stroke: var(--color-warn, #d97706); }
.g-zone--danger { stroke: var(--color-danger, #dc2626); }

/* fill */
.g-fill      { fill: none; stroke: var(--color-accent, #3b82f6); stroke-width: 4.5; stroke-linecap: round; }
.g-fill.warn { stroke: var(--color-warn, #d97706); }
.g-fill.danger { stroke: var(--color-danger, #dc2626); }

/* ticks */
.g-tick { stroke: var(--color-text-muted); stroke-width: 1; stroke-linecap: round; opacity: 0.5; }
.g-tick--major { stroke: var(--color-text-muted); stroke-width: 1.5; opacity: 0.8; }

.g-tick-label {
  font-size: 7px;
  fill: var(--color-text-muted);
  font-variant-numeric: tabular-nums;
}

/* needle */
.g-needle {
  stroke: var(--color-text);
  stroke-width: 2;
  stroke-linecap: round;
}
.g-needle.warn   { stroke: var(--color-warn, #d97706); }
.g-needle.danger { stroke: var(--color-danger, #dc2626); }

.g-hub       { fill: var(--color-bg-muted); stroke: var(--color-border); stroke-width: 1.2; }
.g-hub-inner { fill: var(--color-text-muted); }

/* text in SVG */
.g-label {
  font-size: 8.5px;
  fill: var(--color-text-muted);
  font-weight: 500;
}
.g-value {
  font-size: 13px;
  font-weight: 700;
  fill: var(--color-text);
  font-variant-numeric: tabular-nums;
}
.g-value.warn   { fill: var(--color-warn, #d97706); }
.g-value.danger { fill: var(--color-danger, #dc2626); }
.g-unit { font-size: 7px; fill: var(--color-text-muted); }
.g-src  { font-size: 6px; fill: var(--color-text-subtle); }
.g-src--log { fill: var(--color-accent, #3b82f6); }

/* gear overlay in svg */
.g-gear-btn { cursor: pointer; opacity: 0; transition: opacity 0.15s; }
.ov:hover .g-gear-btn { opacity: 1; }
.g-gear-bg   { fill: var(--color-bg-muted); opacity: 0.85; }
.g-gear-ring { fill: none; stroke: var(--color-text-muted); stroke-width: 1.5; }
.g-gear-ring.active { stroke: var(--color-accent, #3b82f6); }
.g-gear-dot  { fill: var(--color-text-muted); }

/* ── Settings ── */
.ov-settings {
  border-top: 1px solid var(--color-border);
  padding: 0.3rem 0.4rem 0.35rem;
  display: flex;
  flex-direction: column;
  gap: 0.22rem;
  background: var(--color-bg);
  border-radius: 0 0 var(--radius-md) var(--radius-md);
  border: 1px solid var(--color-border);
  border-top: none;
}
.ov--gauge .ov-settings {
  border-radius: var(--radius-md);
  border-top: 1px solid var(--color-border);
  margin-top: 0.25rem;
}
.s-row { display: flex; align-items: center; gap: 0.22rem; flex-wrap: wrap; }
.s-lbl { font-size: 0.65rem; color: var(--color-text-muted); }
.s-type {
  font-size: 0.68rem;
  padding: 0.12rem 0.32rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: none;
  color: var(--color-text-muted);
  cursor: pointer;
}
.s-type.active { border-color: var(--color-accent); color: var(--color-text); }
.s-num {
  width: 3rem;
  font-size: 0.7rem;
  padding: 0.12rem 0.25rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: var(--color-bg);
  color: var(--color-text);
}
.s-sel {
  font-size: 0.68rem;
  padding: 0.12rem 0.18rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: var(--color-bg);
  color: var(--color-text);
}
.s-rm {
  font-size: 0.65rem;
  width: 1.1rem; height: 1.1rem;
  border: none; background: none; cursor: pointer;
  color: var(--color-text-muted);
  border-radius: var(--radius-sm);
  display: flex; align-items: center; justify-content: center;
}
.s-rm:hover { color: var(--color-danger, #dc2626); }
.s-add {
  font-size: 0.75rem;
  width: 1.3rem; height: 1.3rem;
  border: 1px solid var(--color-border);
  background: none; cursor: pointer;
  color: var(--color-text);
  border-radius: var(--radius-sm);
  display: flex; align-items: center; justify-content: center;
}
.s-add:hover { border-color: var(--color-accent); }
</style>
