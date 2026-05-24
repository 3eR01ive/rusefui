<script setup lang="ts">
import {
  computed,
  onMounted,
  onUnmounted,
  ref,
  shallowRef,
  watch,
} from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { initOutputChannels, useOutputChannels } from "../../composables/useOutputChannels";
import { useOutputFields } from "../../composables/useOutputFields";
import {
  createTimeSeriesStore,
  type TimeSeries,
} from "../../composables/useTimeSeriesBuffer";
import { drawLogPanelsChart, type LogGraphPanelSpec, type LogTraceSpec } from "../../composables/drawTimeSeriesChart";

interface LogGraphGroup {
  id: string;
  fieldNames: string[];
}

const MAX_CHANNELS = 12;
const MAX_GRAPHS = 6;
let graphIdSeq = 1;

function nextGraphId(): string {
  graphIdSeq += 1;
  return `g${graphIdSeq}`;
}

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const windowSeconds = computed(() => {
  const w = Number(props.props.windowSeconds ?? 30);
  return w > 0 ? w : 30;
});

const chartHeight = computed(() => {
  const h = Number(props.props.height ?? 280);
  return h > 120 ? h : 280;
});

const defaultFields = computed(() => {
  const raw = props.props.fields;
  if (Array.isArray(raw)) {
    return raw.map(String).filter(Boolean);
  }
  return ["RPMValue", "coolant"];
});

const canvasRef = ref<HTMLCanvasElement | null>(null);
const containerRef = ref<HTMLDivElement | null>(null);
const searchInputRef = ref<HTMLInputElement | null>(null);
const canvasWidth = ref(640);

const { snapshot } = useOutputChannels();
const { fields: allFields, reload: reloadOutputFields } = useOutputFields();

const graphGroups = ref<LogGraphGroup[]>([
  { id: "g1", fieldNames: [...defaultFields.value] },
]);
const activeGraphId = ref("g1");

function allSelectedFields(): string[] {
  return graphGroups.value.flatMap((g) => g.fieldNames);
}

function findGraphWithField(name: string): LogGraphGroup | undefined {
  return graphGroups.value.find((g) => g.fieldNames.includes(name));
}

function isFieldSelected(name: string): boolean {
  return allSelectedFields().includes(name);
}

function syncGraphStore(): void {
  syncRangeInputs();
  store.value.setFields(allSelectedFields());
}

async function refreshFieldCatalog(): Promise<void> {
  await reloadOutputFields();
  const defaults = defaultFields.value.filter((f) =>
    allFields.value.length === 0 ? true : allFields.value.some((x) => x.name === f),
  );
  const names =
    defaults.length > 0
      ? defaults
      : allFields.value.length > 0
        ? [allFields.value[0]!.name]
        : [];
  graphGroups.value = [{ id: "g1", fieldNames: names }];
  activeGraphId.value = "g1";
  graphIdSeq = 1;
  syncGraphStore();
}
const fieldFilter = ref("");
const showSuggest = ref(false);
const suggestStyle = ref({ top: "0px", left: "0px", width: "0px" });
const store = shallowRef(createTimeSeriesStore(windowSeconds.value));

function updateSuggestPosition(): void {
  const el = searchInputRef.value;
  if (!el) return;
  const r = el.getBoundingClientRect();
  suggestStyle.value = {
    top: `${r.bottom + 2}px`,
    left: `${r.left}px`,
    width: `${r.width}px`,
  };
}

function openSuggest(): void {
  showSuggest.value = true;
  updateSuggestPosition();
}

function closeSuggestSoon(): void {
  window.setTimeout(() => {
    showSuggest.value = false;
  }, 160);
}

/** min/max для шкалы Y; пустая строка = авто по данным окна. */
const rangeInputs = ref<Record<string, { min: string; max: string }>>({});

function syncRangeInputs(): void {
  const next: Record<string, { min: string; max: string }> = {};
  for (const name of allSelectedFields()) {
    next[name] = rangeInputs.value[name] ?? { min: "", max: "" };
  }
  rangeInputs.value = next;
}

function parseRangeInput(raw: string): number | null {
  const t = raw.trim();
  if (!t) return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

function setRangeMin(name: string, value: string): void {
  const prev = rangeInputs.value[name] ?? { min: "", max: "" };
  rangeInputs.value = { ...rangeInputs.value, [name]: { ...prev, min: value } };
  redraw();
}

function setRangeMax(name: string, value: string): void {
  const prev = rangeInputs.value[name] ?? { min: "", max: "" };
  rangeInputs.value = { ...rangeInputs.value, [name]: { ...prev, max: value } };
  redraw();
}

watch(
  () => snapshot.value.iniFieldCount ?? 0,
  (count, prev) => {
    if (count > 0 && count !== prev) {
      void refreshFieldCatalog();
    }
  },
);

watch(windowSeconds, (sec) => {
  store.value = createTimeSeriesStore(sec);
  store.value.setFields(allSelectedFields());
});

watch(graphGroups, () => syncGraphStore(), { deep: true });

const channelRows = computed(() => {
  const rows: {
    name: string;
    graphId: string;
    graphLabel: string;
    color: string;
    units: string;
    value: number | null;
    min: string;
    max: string;
  }[] = [];
  graphGroups.value.forEach((g, gi) => {
    for (const name of g.fieldNames) {
      const s = store.value.seriesMap.get(name);
      const meta = allFields.value.find((f) => f.name === name);
      const pts = s?.points;
      const last = pts && pts.length > 0 ? pts[pts.length - 1]!.v : null;
      const ranges = rangeInputs.value[name] ?? { min: "", max: "" };
      rows.push({
        name,
        graphId: g.id,
        graphLabel: `Граф ${gi + 1}`,
        color: s?.color ?? "#888",
        units: meta?.units ?? "",
        value: last,
        min: ranges.min,
        max: ranges.max,
      });
    }
  });
  return rows;
});

const PANEL_GAP_UI = 6;

const canvasHeight = computed(() => {
  const n = Math.max(1, graphGroups.value.filter((g) => g.fieldNames.length > 0).length);
  const perPanel = Math.max(140, chartHeight.value);
  return perPanel * n + PANEL_GAP_UI * Math.max(0, n - 1) + 12;
});

const hasAnyChannel = computed(() => allSelectedFields().length > 0);

const filteredFields = computed(() => {
  const q = fieldFilter.value.trim().toLowerCase();
  const list = allFields.value;
  if (!list.length) return [];
  if (!q) return list.slice(0, 80);
  return list.filter((f) => f.name.toLowerCase().includes(q)).slice(0, 80);
});

const suggestEmptyHint = computed(() => {
  if (allFields.value.length === 0) {
    return snapshot.value.connected
      ? "INI без output channels — переподключите ECU"
      : "Подключите ECU — список полей из signature INI";
  }
  if (fieldFilter.value.trim() && filteredFields.value.length === 0) {
    return "Нет совпадений";
  }
  return null;
});

const activeSeries = computed((): TimeSeries[] => {
  const out: TimeSeries[] = [];
  for (const f of allSelectedFields()) {
    const s = store.value.seriesMap.get(f);
    if (s) out.push(s);
  }
  return out;
});

const legendItems = computed(() =>
  channelRows.value.map((row) => ({
    name: row.name,
    graphLabel: row.graphLabel,
    color: row.color,
    units: row.units,
    value: row.value,
  })),
);

function addGraph(): void {
  if (graphGroups.value.length >= MAX_GRAPHS) return;
  const id = nextGraphId();
  graphGroups.value = [...graphGroups.value, { id, fieldNames: [] }];
  activeGraphId.value = id;
}

function removeGraph(id: string): void {
  if (graphGroups.value.length <= 1) return;
  const g = graphGroups.value.find((x) => x.id === id);
  if (g) {
    for (const name of g.fieldNames) {
      const { [name]: _, ...rest } = rangeInputs.value;
      rangeInputs.value = rest;
    }
  }
  graphGroups.value = graphGroups.value.filter((x) => x.id !== id);
  if (activeGraphId.value === id) {
    activeGraphId.value = graphGroups.value[0]!.id;
  }
  syncGraphStore();
}

function moveFieldToGraph(name: string, graphId: string): void {
  for (const g of graphGroups.value) {
    g.fieldNames = g.fieldNames.filter((f) => f !== name);
  }
  const target = graphGroups.value.find((g) => g.id === graphId);
  if (target && !target.fieldNames.includes(name)) {
    target.fieldNames = [...target.fieldNames, name];
  }
  graphGroups.value = [...graphGroups.value];
  syncGraphStore();
  redraw();
}

function toggleField(name: string): void {
  const existing = findGraphWithField(name);
  if (existing) {
    existing.fieldNames = existing.fieldNames.filter((f) => f !== name);
    const { [name]: _, ...rest } = rangeInputs.value;
    rangeInputs.value = rest;
    graphGroups.value = [...graphGroups.value];
    syncGraphStore();
    return;
  }
  if (allSelectedFields().length >= MAX_CHANNELS) return;
  const g =
    graphGroups.value.find((x) => x.id === activeGraphId.value) ?? graphGroups.value[0];
  if (!g) return;
  g.fieldNames = [...g.fieldNames, name];
  rangeInputs.value[name] = { min: "", max: "" };
  graphGroups.value = [...graphGroups.value];
  syncGraphStore();
}

function removeField(name: string): void {
  const g = findGraphWithField(name);
  if (g) {
    g.fieldNames = g.fieldNames.filter((f) => f !== name);
    graphGroups.value = [...graphGroups.value];
  }
  const { [name]: _, ...rest } = rangeInputs.value;
  rangeInputs.value = rest;
  syncGraphStore();
}

function clearHistory(): void {
  store.value.resetTimeOrigin();
}

function redraw(): void {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const dpr = window.devicePixelRatio || 1;
  const w = canvasWidth.value;
  const h = canvasHeight.value;
  canvas.width = Math.floor(w * dpr);
  canvas.height = Math.floor(h * dpr);
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  const { tMin, tMax } = store.value.visibleRange();
  const panels: LogGraphPanelSpec[] = [];
  graphGroups.value.forEach((group, gi) => {
    const traces: LogTraceSpec[] = [];
    for (const name of group.fieldNames) {
      const s = store.value.seriesMap.get(name);
      if (!s) continue;
      const inp = rangeInputs.value[name] ?? { min: "", max: "" };
      const { vMin, vMax } = store.value.valueRangeForSeries(
        s,
        tMin,
        tMax,
        parseRangeInput(inp.min),
        parseRangeInput(inp.max),
      );
      const meta = allFields.value.find((f) => f.name === name);
      traces.push({
        series: s,
        vMin,
        vMax,
        name,
        units: meta?.units ?? "",
        color: s.color,
      });
    }
    if (traces.length > 0) {
      panels.push({ traces, title: `Граф ${gi + 1}` });
    }
  });
  drawLogPanelsChart(ctx, w, h, panels, tMin, tMax);
}

let resizeObserver: ResizeObserver | null = null;
let unlistenEcu: UnlistenFn | null = null;

onMounted(async () => {
  await initOutputChannels();
  await refreshFieldCatalog();

  unlistenEcu = await listen("ecu-connection", () => {
    void refreshFieldCatalog();
  });

  if (containerRef.value) {
    resizeObserver = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) {
        canvasWidth.value = Math.max(200, entry.contentRect.width);
        redraw();
      }
    });
    resizeObserver.observe(containerRef.value);
  }
  redraw();
});

onUnmounted(() => {
  resizeObserver?.disconnect();
  unlistenEcu?.();
  window.removeEventListener("scroll", updateSuggestPosition, true);
  window.removeEventListener("resize", updateSuggestPosition);
});

watch(showSuggest, (open) => {
  if (open) {
    updateSuggestPosition();
    window.addEventListener("scroll", updateSuggestPosition, true);
    window.addEventListener("resize", updateSuggestPosition);
  } else {
    window.removeEventListener("scroll", updateSuggestPosition, true);
    window.removeEventListener("resize", updateSuggestPosition);
  }
});

watch(
  () => snapshot.value.values,
  (values) => {
    if (!snapshot.value.connected) return;
    for (const f of allSelectedFields()) {
      const v = values[f];
      if (v !== undefined) store.value.addSample(f, v);
    }
    redraw();
  },
);

watch(activeSeries, () => redraw(), { deep: true });
watch(canvasHeight, () => redraw());
watch(rangeInputs, () => redraw(), { deep: true });
watch(graphGroups, () => redraw(), { deep: true });
</script>

<template>
  <div class="output-chart log-chart" ref="containerRef">
    <div class="graph-tabs">
      <button
        v-for="(g, i) in graphGroups"
        :key="g.id"
        type="button"
        class="graph-tab"
        :class="{ active: activeGraphId === g.id }"
        @click="activeGraphId = g.id"
      >
        Граф {{ i + 1 }}
        <span v-if="g.fieldNames.length" class="graph-tab-count">{{ g.fieldNames.length }}</span>
      </button>
      <button
        type="button"
        class="graph-tab graph-tab-add"
        :disabled="graphGroups.length >= MAX_GRAPHS"
        title="Добавить график"
        @click="addGraph"
      >
        +
      </button>
      <button
        v-if="graphGroups.length > 1"
        type="button"
        class="graph-tab graph-tab-remove"
        title="Удалить активный график"
        @click="removeGraph(activeGraphId)"
      >
        −
      </button>
      <span class="graph-tabs-hint">Новые каналы добавляются на активный граф</span>
    </div>

    <div class="toolbar">
      <div class="field-picker">
        <label class="picker-label" for="chart-field-filter">Каналы log</label>
        <input
          id="chart-field-filter"
          ref="searchInputRef"
          v-model="fieldFilter"
          type="search"
          class="field-search"
          placeholder="Поиск по имени…"
          autocomplete="off"
          @focus="openSuggest"
          @blur="closeSuggestSoon"
          @input="updateSuggestPosition"
        />
        <Teleport to="body">
          <ul
            v-if="showSuggest"
            class="field-suggest field-suggest-portal"
            :style="suggestStyle"
          >
            <li v-if="suggestEmptyHint" class="field-suggest-empty">
              {{ suggestEmptyHint }}
            </li>
            <li
              v-for="f in filteredFields"
              :key="f.name"
              :class="{ active: isFieldSelected(f.name) }"
            >
              <button type="button" @mousedown.prevent="toggleField(f.name)">
                {{ f.name }}
                <span v-if="f.units" class="units">{{ f.units }}</span>
              </button>
            </li>
          </ul>
        </Teleport>
      </div>

      <div class="selected-fields">
        <span
          v-for="item in legendItems"
          :key="item.name"
          class="chip"
          :style="{ borderColor: item.color }"
        >
          <span class="chip-dot" :style="{ background: item.color }" />
          <span class="chip-graph">{{ item.graphLabel }}</span>
          <span class="chip-name">{{ item.name }}</span>
          <span v-if="item.value !== null" class="chip-val">
            {{ Number.isInteger(item.value) ? item.value : item.value.toFixed(2) }}
            <span v-if="item.units" class="chip-units">{{ item.units }}</span>
          </span>
          <button type="button" class="chip-remove" title="Убрать" @click="removeField(item.name)">
            ×
          </button>
        </span>
      </div>

      <div class="toolbar-actions">
        <span class="window-hint">окно {{ windowSeconds }} с · автопромотка</span>
        <button type="button" class="btn-clear" @click="clearHistory">Сброс</button>
      </div>
    </div>

    <div v-if="channelRows.length" class="channel-ranges">
      <p class="ranges-title">Диапазон Y · min / max (пусто = авто по окну)</p>
      <div class="ranges-grid">
        <div v-for="row in channelRows" :key="row.name" class="range-row">
          <span class="range-dot" :style="{ background: row.color }" />
          <span class="range-name">{{ row.name }}</span>
          <label class="range-graph">
            <span>граф</span>
            <select
              class="range-select"
              :value="row.graphId"
              @change="moveFieldToGraph(row.name, ($event.target as HTMLSelectElement).value)"
            >
              <option v-for="(g, i) in graphGroups" :key="g.id" :value="g.id">
                {{ i + 1 }}
              </option>
            </select>
          </label>
          <label class="range-field">
            <span>min</span>
            <input
              type="number"
              class="range-input"
              :value="row.min"
              placeholder="авто"
              step="any"
              @input="setRangeMin(row.name, ($event.target as HTMLInputElement).value)"
            />
          </label>
          <label class="range-field">
            <span>max</span>
            <input
              type="number"
              class="range-input"
              :value="row.max"
              placeholder="авто"
              step="any"
              @input="setRangeMax(row.name, ($event.target as HTMLInputElement).value)"
            />
          </label>
        </div>
      </div>
    </div>

    <div class="canvas-wrap">
      <canvas ref="canvasRef" class="chart-canvas" />
      <p v-if="!snapshot.connected" class="overlay-hint">Подключите ECU для записи log</p>
      <p v-else-if="!hasAnyChannel" class="overlay-hint">
        Выберите параметры через поиск — они попадут на активный граф
      </p>
    </div>

    <p v-if="snapshot.lastError" class="error">{{ snapshot.lastError }}</p>
  </div>
</template>

<style scoped>
.output-chart {
  display: flex;
  flex-direction: column;
  gap: 0.65rem;
  width: 100%;
}

.graph-tabs {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.35rem;
}

.graph-tab {
  display: inline-flex;
  align-items: center;
  gap: 0.3rem;
  padding: 0.28rem 0.55rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-text-muted);
  font-size: 0.78rem;
  cursor: pointer;
}

.graph-tab.active {
  border-color: var(--color-accent);
  color: var(--color-text);
  background: var(--color-bg-accent-soft);
}

.graph-tab-count {
  font-size: 0.68rem;
  opacity: 0.75;
}

.graph-tab-add,
.graph-tab-remove {
  min-width: 1.75rem;
  justify-content: center;
  font-weight: 600;
}

.graph-tab:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.graph-tabs-hint {
  font-size: 0.72rem;
  color: var(--color-text-subtle);
  margin-left: 0.25rem;
}

.toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 0.65rem;
  align-items: flex-start;
}

.field-picker {
  position: relative;
  flex: 1 1 14rem;
  min-width: 12rem;
}

.picker-label {
  display: block;
  font-size: 0.72rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-gray);
  margin-bottom: 0.3rem;
  font-weight: 500;
}

.field-search {
  width: 100%;
  padding: 0.45rem 0.6rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-text);
}

.field-suggest-portal {
  position: fixed;
  z-index: 10050;
  max-height: min(280px, 40vh);
  overflow: auto;
  margin: 0;
  padding: 0.25rem 0;
  list-style: none;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-card);
}

.field-suggest-empty {
  padding: 0.5rem 0.65rem;
  font-size: 0.82rem;
  color: var(--color-text-subtle);
}

.field-suggest-portal li.active button {
  background: var(--color-bg-accent-soft);
}

.field-suggest-portal button {
  display: flex;
  width: 100%;
  gap: 0.5rem;
  justify-content: space-between;
  padding: 0.35rem 0.65rem;
  border: none;
  background: transparent;
  color: var(--color-text);
  text-align: left;
  font-size: 0.85rem;
  cursor: pointer;
}

.field-suggest-portal button:hover {
  background: var(--color-bg-muted);
}

.field-suggest-portal .units {
  color: var(--color-text-subtle);
  font-size: 0.78rem;
}

.selected-fields {
  display: flex;
  flex-wrap: wrap;
  gap: 0.35rem;
  flex: 2 1 20rem;
  align-content: flex-start;
  padding-top: 1.15rem;
}

.channel-ranges {
  padding: 0.65rem 0.75rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: var(--color-bg-muted);
}

.ranges-title {
  margin: 0 0 0.5rem;
  font-size: 0.72rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-gray);
  font-weight: 500;
}

.ranges-grid {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}

.range-row {
  display: grid;
  grid-template-columns: auto 1fr auto auto auto auto;
  gap: 0.5rem 0.75rem;
  align-items: center;
}

.range-graph {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  font-size: 0.72rem;
  color: var(--color-text-subtle);
}

.range-select {
  width: 3rem;
  padding: 0.25rem 0.3rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-text);
  font-size: 0.82rem;
}

.chip-graph {
  font-size: 0.68rem;
  color: var(--color-text-subtle);
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.range-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

.range-name {
  font-size: 0.82rem;
  font-weight: 500;
  font-family: ui-monospace, monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.range-field {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  font-size: 0.72rem;
  color: var(--color-text-subtle);
}

.range-input {
  width: 5.5rem;
  padding: 0.25rem 0.4rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-elevated);
  color: var(--color-text);
  font-size: 0.82rem;
}

.range-input::placeholder {
  color: var(--color-text-subtle);
  opacity: 0.7;
}

@media (max-width: 640px) {
  .range-row {
    grid-template-columns: auto 1fr;
    grid-template-rows: auto auto;
  }

  .range-name {
    grid-column: 2;
  }

  .range-field {
    grid-column: 2;
  }
}

.chip {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  padding: 0.2rem 0.45rem 0.2rem 0.35rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border);
  background: var(--color-bg-muted);
  font-size: 0.78rem;
}

.chip-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}

.chip-name {
  font-weight: 500;
  color: var(--color-text);
}

.chip-val {
  font-variant-numeric: tabular-nums;
  color: var(--color-text-muted);
}

.chip-units {
  opacity: 0.85;
}

.chip-remove {
  border: none;
  background: transparent;
  color: var(--color-text-subtle);
  cursor: pointer;
  padding: 0 0.15rem;
  font-size: 1rem;
  line-height: 1;
}

.chip-remove:hover {
  color: var(--color-error);
}

.toolbar-actions {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 0.25rem;
  padding-top: 1.15rem;
}

.window-hint {
  font-size: 0.72rem;
  color: var(--color-text-subtle);
  white-space: nowrap;
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

.canvas-wrap {
  position: relative;
  width: 100%;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  background: var(--color-bg-elevated);
  overflow: hidden;
}

.chart-canvas {
  display: block;
  width: 100%;
}

.overlay-hint {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  margin: 0;
  font-size: 0.9rem;
  color: var(--color-text-subtle);
  pointer-events: none;
  background: rgba(250, 247, 242, 0.55);
}

.error {
  margin: 0;
  font-size: 0.82rem;
  color: var(--color-error);
}
</style>
