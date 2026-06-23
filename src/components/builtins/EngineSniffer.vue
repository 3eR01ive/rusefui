<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useInstanceBind } from "../../composables/useInstanceBind";
import { useDataContext } from "../../core/data-context";
import { useTabActivity } from "../../composables/useTabActivity";
import {
  initEngineSniffer,
  useEngineSniffer,
} from "../../composables/useEngineSniffer";
import { configCanEdit, initConfig, useConfig } from "../../composables/useConfig";
import { useEcuConsole } from "../../composables/useEcuConsole";
import {
  buildSnifferView,
  laneY,
  timeAtX,
  type SnifferView,
  type SnifferTimeRange,
} from "./engineSnifferGeometry";
import { EngineSnifferRenderer } from "./engineSnifferRenderer";

const yamlProps = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

type Rgba = [number, number, number, number];

const LABEL_W = 56;

/** Палитра каналов (циклична). */
const PALETTE = [
  "#3b82f6", "#f59e0b", "#10b981", "#ef4444", "#8b5cf6",
  "#06b6d4", "#ec4899", "#84cc16", "#f97316", "#14b8a6",
];
function channelCss(i: number): string {
  return PALETTE[i % PALETTE.length]!;
}

const instanceRef = computed(() => yamlProps.instance);
const { source: bindSource } = useInstanceBind(instanceRef);
if (bindSource.value && bindSource.value !== "engineSniffer") {
  console.warn(
    `[engine-sniffer] ожидался bind.source=engineSniffer, получен ${bindSource.value}`,
  );
}

const chartHeight = computed(() => {
  const h = Number(yamlProps.props.height ?? 320);
  return h >= 200 ? h : 320;
});

const { snapshot, setEnabled } = useEngineSniffer();
const { isActive: tabActive } = useTabActivity();

// ---- Параметры sniffer из INI (no-hardcode: только имена полей) -------------
const { snapshot: cfgSnap, getField, getFieldInfo, setField } = useConfig();
const SNIFFER_FIELDS = {
  window: "engineChartSize",
  threshold: "engineSnifferRpmThreshold",
  focusInputs: "engineSnifferFocusOnInputs",
  logicLevels: "displayLogicLevelsInEngineSniffer",
  verbose: "verboseTriggerSynchDetails",
} as const;

const cfgEditable = computed(() => configCanEdit(cfgSnap.value));
function hasField(name: string): boolean {
  void cfgSnap.value;
  return getFieldInfo(name) != null;
}
function fieldNum(name: string): number | null {
  void cfgSnap.value;
  return getField(name);
}
function fieldBool(name: string): boolean {
  return (fieldNum(name) ?? 0) > 0.5;
}
const showSettings = computed(
  () =>
    hasField(SNIFFER_FIELDS.window) ||
    hasField(SNIFFER_FIELDS.threshold) ||
    hasField(SNIFFER_FIELDS.focusInputs) ||
    hasField(SNIFFER_FIELDS.logicLevels) ||
    hasField(SNIFFER_FIELDS.verbose),
);

const windowSize = computed(() => fieldNum(SNIFFER_FIELDS.window));
const threshold = computed(() => fieldNum(SNIFFER_FIELDS.threshold));
const focusInputs = computed(() => fieldBool(SNIFFER_FIELDS.focusInputs));
const logicLevels = computed(() => fieldBool(SNIFFER_FIELDS.logicLevels));
const verboseTrigger = computed(() => fieldBool(SNIFFER_FIELDS.verbose));

// ---- Лог синхронизации триггера ---------------------------------------------
// Прошивка печатает детали гэпов из printGaps() с префиксами [vrb] (вербоз,
// каждый оборот), [err]/newerr (при ошибке декодирования). Эти сообщения идут
// обычным консольным текстом ECU — фильтруем их из общего потока консоли.
const TRIGGER_PREFIXES = ["[vrb]", "[err]", "newerr"] as const;
/**
 * Маркер ошибки триггера в строке printGaps(): прошивка печатает
 * `… error=Yes`/`error=No` (boolToString) в каждой строке гэпа — в т.ч. в
 * вербозных [vrb], — поэтому ловим ошибку по подстроке, а не по префиксу.
 */
const ERROR_MARKER = "error=Yes";
/** Сколько записей дописать после первой ошибки, прежде чем заморозить лог. */
const FREEZE_AFTER_ERROR = 100;
const { lines: consoleLines } = useEcuConsole();
const trigLogRef = ref<HTMLElement | null>(null);
/** Локальный «водяной знак» для очистки лога без слива общего буфера консоли. */
const logClearedBeforeId = ref(0);

function isErrorLine(text: string): boolean {
  return text.includes(ERROR_MARKER);
}

const liveTriggerLog = computed(() =>
  consoleLines.value
    .filter(
      (l) =>
        l.id >= logClearedBeforeId.value &&
        TRIGGER_PREFIXES.some((p) => l.text.startsWith(p)),
    )
    .slice(-200),
);

// ---- Заморозка при ошибке ---------------------------------------------------
// Общий буфер консоли (1000 строк) и срез лога (200) перетирают старые записи,
// поэтому при первой ошибке триггера фиксируем «якорь», дописываем ещё
// FREEZE_AFTER_ERROR записей и замораживаем снимок — ошибки не теряются.
const frozen = ref(false);
const frozenLog = ref<typeof liveTriggerLog.value>([]);
const errorAnchorId = ref<number | null>(null);

watch(liveTriggerLog, (lines) => {
  if (frozen.value) return;
  if (errorAnchorId.value === null) {
    const firstErr = lines.find((l) => isErrorLine(l.text));
    if (firstErr) errorAnchorId.value = firstErr.id;
  }
  if (errorAnchorId.value !== null) {
    const anchor = errorAnchorId.value;
    const after = lines.filter((l) => l.id > anchor).length;
    if (after >= FREEZE_AFTER_ERROR) {
      frozenLog.value = lines.slice();
      frozen.value = true;
    }
  }
});

/** Лог, отображаемый в панели: замороженный снимок либо живой поток. */
const triggerLog = computed(() =>
  frozen.value ? frozenLog.value : liveTriggerLog.value,
);

function clearTriggerLog(): void {
  const last = consoleLines.value[consoleLines.value.length - 1];
  logClearedBeforeId.value = last ? last.id + 1 : 0;
  resumeTriggerLog();
}

/** Снять заморозку и вернуться к живому потоку. */
function resumeTriggerLog(): void {
  frozen.value = false;
  errorAnchorId.value = null;
  frozenLog.value = [];
}

watch(
  () => triggerLog.value.length,
  () => {
    if (frozen.value) return;
    void nextTick(() => {
      const el = trigLogRef.value;
      if (el) el.scrollTop = el.scrollHeight;
    });
  },
);

/** При заморозке прокручиваем к первой ошибке, чтобы её было видно. */
watch(frozen, (isFrozen) => {
  if (!isFrozen) return;
  void nextTick(() => {
    const el = trigLogRef.value;
    if (!el) return;
    const errEl = el.querySelector<HTMLElement>(".es-triglog-line--error");
    if (errEl) el.scrollTop = Math.max(0, errEl.offsetTop - el.clientHeight / 2);
  });
});

async function commitNum(name: string, e: Event): Promise<void> {
  const v = Number((e.target as HTMLInputElement).value);
  if (!Number.isFinite(v) || v < 0) return;
  try {
    await setField(name, v);
  } catch (err) {
    console.warn(`[engine-sniffer] не удалось записать ${name}:`, err);
  }
}
async function commitBool(name: string, e: Event): Promise<void> {
  try {
    await setField(name, (e.target as HTMLInputElement).checked ? 1 : 0);
  } catch (err) {
    console.warn(`[engine-sniffer] не удалось записать ${name}:`, err);
  }
}

const panelRef = ref<HTMLElement | null>(null);
const wrapRef = ref<HTMLElement | null>(null);
const canvasRef = ref<HTMLCanvasElement | null>(null);

const renderer = new EngineSnifferRenderer();
let redrawRaf = 0;
let resizeObs: ResizeObserver | null = null;
const sizeTick = ref(0);
const crosshairX = ref<number | null>(null);
/** Привязывать левый край окна к TDC (фазовая стабилизация по горизонтали). */
const alignTdc = ref(true);
/** Фиксировать масштаб по горизонтали = один цикл (период TDC), сглажено. */
const lockScale = ref(false);
/** EMA периода цикла (µs) — гасит дрожание масштаба между кадрами. */
const emaPeriod = ref(0);

/** Группы каналов: тон подложки и подпись. */
const GROUP_TINT: Record<string, string> = {
  trigger: "rgba(59,130,246,0.12)",
  ignition: "rgba(239,68,68,0.12)",
  injector: "rgba(16,185,129,0.12)",
  other: "rgba(148,163,184,0.08)",
};
const GROUP_LABEL: Record<string, string> = {
  trigger: "Триггеры",
  ignition: "Зажигание",
  injector: "Форсунки",
  other: "Прочее",
};

// ---- CSS var → RGBA ---------------------------------------------------------
let colorProbe: CanvasRenderingContext2D | null = null;
function cssColorToRgba(css: string, alpha = 1): Rgba {
  if (!colorProbe) colorProbe = document.createElement("canvas").getContext("2d");
  const probe = colorProbe;
  if (!probe) return [0.5, 0.5, 0.5, alpha];
  probe.fillStyle = css;
  const p = probe.fillStyle as string;
  if (p.startsWith("#")) {
    const hex = p.slice(1);
    const full = hex.length === 3 ? hex.split("").map((c) => c + c).join("") : hex;
    const n = Number.parseInt(full, 16);
    return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255, alpha];
  }
  const m = p.match(/rgba?\(([^)]+)\)/);
  if (!m) return [0.5, 0.5, 0.5, alpha];
  const parts = m[1]!.split(",").map((s) => Number.parseFloat(s.trim()));
  return [(parts[0] ?? 0) / 255, (parts[1] ?? 0) / 255, (parts[2] ?? 0) / 255, parts[3] ?? alpha];
}

function readGlColors(canvas: HTMLCanvasElement) {
  const s = getComputedStyle(canvas);
  const v = (n: string, fb: string) => s.getPropertyValue(n).trim() || fb;
  return {
    bg: cssColorToRgba(v("--color-bg", "#0f1115")),
    grid: cssColorToRgba(v("--color-border", "#333"), 1),
    baseline: cssColorToRgba(v("--color-border", "#333"), 0.45),
    tdc: cssColorToRgba(v("--color-warning", "#d97706"), 0.85),
    crosshair: cssColorToRgba(v("--color-fg", "#e5e7eb"), 0.55),
  };
}

// ---- Reactive state ---------------------------------------------------------
// Подключение берём из глобального DataContext, а не из снапшота sniffer:
// его поле `connected` обновляется только во время поллинга (после Старта),
// иначе кнопка «Старт» оставалась бы заблокированной (курица-яйцо).
const dataCtx = useDataContext();
const connected = computed(() => dataCtx.connection.value.connected);
const polling = computed(() => snapshot.value.polling);
const channels = computed(() => snapshot.value.channels);
const rpm = computed(() => snapshot.value.rpm ?? null);
const framesReceived = computed(() => snapshot.value.framesReceived ?? 0);
const lastError = computed(() => snapshot.value.lastError ?? null);

/**
 * Фаза кадра: якорь t0 (первый TDC при alignTdc), «сырая» ширина (весь кадр /
 * все циклы) и период одного цикла (если TDC ≥ 2).
 */
const phase = computed(() => {
  const snap = snapshot.value;
  if (snap.frameSpanUs <= 0) return null;

  const tdc: number[] = [];
  if (alignTdc.value) {
    for (const e of snap.events) if (e.tdc) tdc.push(e.tUs);
  }

  let t0 = 0;
  let span = Math.max(1, snap.frameSpanUs);
  let period: number | null = null;

  if (tdc.length >= 1) t0 = tdc[0]!;
  if (tdc.length >= 2) {
    span = Math.max(1, tdc[tdc.length - 1]! - t0);
    period = span / (tdc.length - 1);
  } else if (tdc.length === 1) {
    const s = snap.frameSpanUs - t0;
    if (s > snap.frameSpanUs * 0.2) span = s;
    else t0 = 0;
  }
  return { t0, span, period };
});

watch(phase, (p) => {
  if (!p) return;
  const target = p.period ?? p.span;
  emaPeriod.value =
    emaPeriod.value <= 0 ? target : emaPeriod.value + 0.2 * (target - emaPeriod.value);
});

/**
 * Окно по времени. Привязка к TDC фиксирует левый край (фаза). «Масштаб» (lock)
 * фиксирует ширину = сглажённый период одного цикла — тогда вывод не «дышит».
 */
const timeRange = computed<SnifferTimeRange | null>(() => {
  const p = phase.value;
  if (!p) return null;
  if (lockScale.value && emaPeriod.value > 0) {
    return { t0: p.t0, spanUs: emaPeriod.value };
  }
  return { t0: p.t0, spanUs: p.span };
});

const view = computed<SnifferView | null>(() => {
  void sizeTick.value;
  const wrap = wrapRef.value;
  if (!wrap) return null;
  const w = wrap.clientWidth;
  const snap = snapshot.value;
  return buildSnifferView(
    snap.channels,
    snap.events,
    snap.frameSpanUs,
    w,
    chartHeight.value,
    LABEL_W,
    timeRange.value,
  );
});

const channelLabels = computed(() => {
  const v = view.value;
  if (!v) return [];
  return v.channels.map((ch, i) => {
    const { yMid } = laneY(i, v);
    return { name: ch.name, css: channelCss(i), top: yMid - 10 };
  });
});

/** Полосы подложки по группам каналов (тон + подпись группы). */
const groupBands = computed(() => {
  const v = view.value;
  if (!v) return [];
  const bands: { top: number; height: number; tint: string; label: string }[] = [];
  let i = 0;
  while (i < v.channels.length) {
    const g = v.channels[i]!.group;
    let j = i;
    while (j < v.channels.length && v.channels[j]!.group === g) j++;
    bands.push({
      top: i * v.laneH + 4,
      height: (j - i) * v.laneH,
      tint: GROUP_TINT[g] ?? GROUP_TINT.other!,
      label: GROUP_LABEL[g] ?? g,
    });
    i = j;
  }
  return bands;
});

const crosshairLabel = computed(() => {
  const v = view.value;
  const x = crosshairX.value;
  if (!v || x == null) return null;
  const ms = (timeAtX(x, v) - v.t0) / 1000;
  return `${ms.toFixed(2)} ms`;
});

const statusLine = computed(() => {
  const parts: string[] = [];
  parts.push(connected.value ? "ECU: подключена" : "ECU: нет связи");
  if (polling.value) parts.push("live");
  parts.push(`каналов ${channels.value.length}`);
  if (rpm.value != null) parts.push(`${rpm.value.toFixed(0)} rpm`);
  if (framesReceived.value > 0) parts.push(`кадров ${framesReceived.value}`);
  return parts.join(" · ");
});

const hint = computed(() => {
  if (lastError.value) return lastError.value;
  if (!connected.value) return "Подключите ECU, затем «Старт».";
  if (!polling.value) {
    return "Старт — логический анализатор (wave_chart). Sniffer активен ниже engineSnifferRpmThreshold.";
  }
  if (channels.value.length === 0) {
    return "Ждём кадр… (sniffer работает при rpm < engineSnifferRpmThreshold)";
  }
  return null;
});

// ---- Render -----------------------------------------------------------------
function redraw() {
  const canvas = canvasRef.value;
  const wrap = wrapRef.value;
  if (!canvas || !wrap) return;
  const colors = readGlColors(canvas);
  const v = view.value;
  const width = wrap.clientWidth;
  const height = chartHeight.value;
  const channelColors = (v?.channels ?? []).map((_, i) =>
    cssColorToRgba(channelCss(i)),
  );
  renderer.paint({
    width,
    height,
    view: v,
    channelColors,
    bgRgba: colors.bg,
    gridRgba: colors.grid,
    baselineRgba: colors.baseline,
    tdcRgba: colors.tdc,
    crosshairRgba: colors.crosshair,
    crosshairX: crosshairX.value,
  });
}

function scheduleRedraw() {
  if (!tabActive.value) return;
  if (redrawRaf !== 0) return;
  redrawRaf = requestAnimationFrame(() => {
    redrawRaf = 0;
    redraw();
  });
}

watch([view, chartHeight, crosshairX], () => scheduleRedraw());
watch(tabActive, (active, wasActive) => {
  if (active && !wasActive) scheduleRedraw();
});

// ---- Pointer (crosshair) ----------------------------------------------------
function onPointerMove(e: PointerEvent): void {
  const wrap = wrapRef.value;
  const v = view.value;
  if (!wrap || !v) return;
  const rect = wrap.getBoundingClientRect();
  const x = e.clientX - rect.left;
  crosshairX.value = x >= v.plotLeft && x <= v.plotLeft + v.plotW ? x : null;
}
function onPointerLeave(): void {
  crosshairX.value = null;
}

// ---- Lifecycle --------------------------------------------------------------
onMounted(async () => {
  if (canvasRef.value) renderer.attach(canvasRef.value);
  void initConfig();
  await initEngineSniffer();
  const target = wrapRef.value ?? panelRef.value;
  if (target) {
    resizeObs = new ResizeObserver(() => {
      sizeTick.value += 1;
      scheduleRedraw();
    });
    resizeObs.observe(target);
  }
  scheduleRedraw();
});

onUnmounted(() => {
  resizeObs?.disconnect();
  resizeObs = null;
  if (redrawRaf !== 0) cancelAnimationFrame(redrawRaf);
  renderer.detach();
  if (polling.value) void setEnabled(false);
});

async function toggle(): Promise<void> {
  await setEnabled(!polling.value);
}
</script>

<template>
  <div ref="panelRef" class="es-panel">
    <div class="es-toolbar">
      <button type="button" class="btn" :disabled="!connected" @click="toggle">
        {{ polling ? "Стоп" : "Старт" }}
      </button>
      <label class="es-align">
        <input v-model="alignTdc" type="checkbox" />
        по TDC
      </label>
      <label class="es-align">
        <input v-model="lockScale" type="checkbox" />
        масштаб
      </label>
      <span class="es-status">{{ statusLine }}</span>
    </div>
    <div v-if="showSettings" class="es-settings">
      <label v-if="hasField(SNIFFER_FIELDS.window)" class="es-field">
        Окно (событий)
        <input
          type="number"
          min="0"
          max="300"
          step="10"
          :value="windowSize ?? ''"
          :disabled="!cfgEditable"
          @change="commitNum(SNIFFER_FIELDS.window, $event)"
        />
      </label>
      <label v-if="hasField(SNIFFER_FIELDS.threshold)" class="es-field">
        Порог, об/мин
        <input
          type="number"
          min="0"
          max="30000"
          step="100"
          :value="threshold ?? ''"
          :disabled="!cfgEditable"
          @change="commitNum(SNIFFER_FIELDS.threshold, $event)"
        />
      </label>
      <label v-if="hasField(SNIFFER_FIELDS.focusInputs)" class="es-field es-field--check">
        <input
          type="checkbox"
          :checked="focusInputs"
          :disabled="!cfgEditable"
          @change="commitBool(SNIFFER_FIELDS.focusInputs, $event)"
        />
        только входы
      </label>
      <label v-if="hasField(SNIFFER_FIELDS.logicLevels)" class="es-field es-field--check">
        <input
          type="checkbox"
          :checked="logicLevels"
          :disabled="!cfgEditable"
          @change="commitBool(SNIFFER_FIELDS.logicLevels, $event)"
        />
        лог. уровни
      </label>
      <label
        v-if="hasField(SNIFFER_FIELDS.verbose)"
        class="es-field es-field--check"
        title="Печатать детали синхронизации триггера в консоль. [vrb] выводится только ниже порога об/мин, [err]/newerr — на любых оборотах."
      >
        <input
          type="checkbox"
          :checked="verboseTrigger"
          :disabled="!cfgEditable"
          @change="commitBool(SNIFFER_FIELDS.verbose, $event)"
        />
        вербоз триггера
      </label>
    </div>
    <p v-if="hint" class="es-hint">{{ hint }}</p>
    <div
      ref="wrapRef"
      class="es-canvas-wrap"
      :style="{ height: `${chartHeight}px` }"
      @pointermove="onPointerMove"
      @pointerleave="onPointerLeave"
    >
      <canvas ref="canvasRef" class="es-canvas" />
      <div class="es-bands" aria-hidden="true">
        <div
          v-for="(b, i) in groupBands"
          :key="i"
          class="es-band"
          :style="{ top: `${b.top}px`, height: `${b.height}px`, background: b.tint }"
        >
          <span class="es-band-label">{{ b.label }}</span>
        </div>
      </div>
      <div class="es-labels" aria-hidden="true">
        <div
          v-for="lbl in channelLabels"
          :key="lbl.name"
          class="es-label"
          :style="{ top: `${lbl.top}px`, color: lbl.css }"
        >
          {{ lbl.name }}
        </div>
      </div>
      <div
        v-if="crosshairLabel"
        class="es-crosshair-label"
        :style="{ left: `${crosshairX}px` }"
      >
        {{ crosshairLabel }}
      </div>
    </div>
    <div v-if="verboseTrigger" class="es-triglog">
      <div class="es-triglog-head">
        <span class="es-triglog-title">Лог синхронизации триггера</span>
        <button
          v-if="frozen"
          type="button"
          class="btn es-triglog-resume"
          @click="resumeTriggerLog"
        >
          Возобновить
        </button>
        <button type="button" class="btn es-triglog-clear" @click="clearTriggerLog">
          Очистить
        </button>
        <span v-if="frozen" class="es-triglog-frozen">
          ⏸ Заморожен на ошибке (+{{ FREEZE_AFTER_ERROR }} записей)
        </span>
        <span v-else class="es-triglog-hint">
          [vrb] — ниже порога об/мин · [err]/newerr — на любых
        </span>
      </div>
      <div ref="trigLogRef" class="es-triglog-body selectable">
        <div
          v-for="l in triggerLog"
          :key="l.id"
          class="es-triglog-line"
          :class="{ 'es-triglog-line--error': isErrorLine(l.text) }"
        >
          <span class="es-triglog-ts">{{ l.ts }}</span>{{ l.text }}
        </div>
        <p v-if="triggerLog.length === 0" class="es-triglog-empty">
          Ждём сообщения триггера…
        </p>
      </div>
    </div>
  </div>
</template>

<style scoped>
.es-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 220px;
}

.es-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
}

.es-status {
  font-size: 12px;
  color: var(--color-text-muted);
  font-variant-numeric: tabular-nums;
}

.es-hint {
  margin: 0;
  font-size: 12px;
  color: var(--color-text-muted);
}

.es-settings {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
}

.es-field {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 12px;
  color: var(--color-text-muted);
}

.es-field input[type="number"] {
  width: 72px;
  padding: 2px 5px;
  font-size: 12px;
  background: var(--color-bg, #0f1115);
  color: var(--color-fg, #e5e7eb);
  border: 1px solid var(--color-border);
  border-radius: 4px;
}

.es-field input:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.es-field--check {
  cursor: pointer;
  user-select: none;
}

.es-canvas-wrap {
  position: relative;
  width: 100%;
  overflow: hidden;
  border-radius: 6px;
  border: 1px solid var(--color-border);
  background: var(--color-bg, #0f1115);
}

.es-canvas {
  display: block;
  width: 100%;
  height: 100%;
}

.es-bands {
  position: absolute;
  inset: 0;
  pointer-events: none;
}

.es-band {
  position: absolute;
  left: 0;
  right: 0;
}

.es-band-label {
  position: absolute;
  right: 8px;
  top: 2px;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.03em;
  text-transform: uppercase;
  color: var(--color-text-muted);
  opacity: 0.85;
}

.es-labels {
  position: absolute;
  inset: 0;
  pointer-events: none;
}

.es-label {
  position: absolute;
  left: 4px;
  padding: 1px 5px;
  font-size: 14px;
  font-weight: 800;
  border-radius: 3px;
  background: rgba(15, 17, 21, 0.78);
  white-space: nowrap;
}

.es-align {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--color-text-muted);
  cursor: pointer;
  user-select: none;
}

.es-crosshair-label {
  position: absolute;
  top: 2px;
  transform: translateX(-50%);
  padding: 1px 4px;
  font-size: 10px;
  color: #fff;
  background: rgba(0, 0, 0, 0.7);
  border-radius: 3px;
  pointer-events: none;
  white-space: nowrap;
}

.es-triglog {
  display: flex;
  flex-direction: column;
  gap: 4px;
  border: 1px solid var(--color-border);
  border-radius: 6px;
  background: var(--color-bg, #0f1115);
  overflow: hidden;
}

.es-triglog-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  padding: 4px 8px;
  border-bottom: 1px solid var(--color-border);
}

.es-triglog-title {
  font-size: 12px;
  font-weight: 700;
  color: var(--color-fg, #e5e7eb);
}

.es-triglog-clear {
  padding: 1px 8px;
  font-size: 11px;
}

.es-triglog-hint {
  font-size: 11px;
  color: var(--color-text-muted);
}

.es-triglog-body {
  max-height: 180px;
  overflow-y: auto;
  padding: 4px 8px;
  font-family: var(--font-mono, monospace);
  font-size: 11px;
  line-height: 1.45;
  background: #fff;
  user-select: text;
  -webkit-user-select: text;
  cursor: text;
}

.es-triglog-line {
  white-space: pre-wrap;
  word-break: break-word;
  color: #000;
  user-select: text;
  -webkit-user-select: text;
}

.es-triglog-ts {
  margin-right: 8px;
  color: #555;
  font-variant-numeric: tabular-nums;
  user-select: text;
  -webkit-user-select: text;
}

.es-triglog-line--error {
  background: #fde2e2;
  color: #7f1d1d;
  font-weight: 700;
}

.es-triglog-line--error .es-triglog-ts {
  color: #b91c1c;
}

.es-triglog-resume {
  padding: 1px 8px;
  font-size: 11px;
}

.es-triglog-frozen {
  font-size: 11px;
  font-weight: 700;
  color: var(--color-warning, #d97706);
}

.es-triglog-empty {
  margin: 0;
  color: var(--color-text-muted);
}
</style>
