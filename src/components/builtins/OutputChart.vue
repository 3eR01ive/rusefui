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
import { drawTimeSeriesChart } from "../../composables/drawTimeSeriesChart";

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

const selectedFields = ref<string[]>([...defaultFields.value]);
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

async function refreshFieldCatalog(): Promise<void> {
  await reloadOutputFields();
  selectedFields.value = defaultFields.value.filter((f) =>
    allFields.value.length === 0 ? true : allFields.value.some((x) => x.name === f),
  );
  if (selectedFields.value.length === 0 && allFields.value.length > 0) {
    selectedFields.value = [allFields.value[0]!.name];
  }
  store.value.setFields(selectedFields.value);
}

watch(
  () => snapshot.value.iniFieldCount,
  (count, prev) => {
    if (count > 0 && count !== prev) {
      void refreshFieldCatalog();
    }
  },
);

watch(windowSeconds, (sec) => {
  store.value = createTimeSeriesStore(sec);
  store.value.setFields(selectedFields.value);
});

watch(
  selectedFields,
  (list) => {
    store.value.setFields(list);
  },
  { deep: true },
);

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
  for (const f of selectedFields.value) {
    const s = store.value.seriesMap.get(f);
    if (s) out.push(s);
  }
  return out;
});

const legendItems = computed(() =>
  selectedFields.value.map((name) => {
    const s = store.value.seriesMap.get(name);
    const meta = allFields.value.find((f) => f.name === name);
    const pts = s?.points;
    const last = pts && pts.length > 0 ? pts[pts.length - 1]!.v : null;
    return {
      name,
      color: s?.color ?? "#888",
      units: meta?.units ?? "",
      value: last,
    };
  }),
);

function toggleField(name: string): void {
  const idx = selectedFields.value.indexOf(name);
  if (idx >= 0) {
    selectedFields.value = selectedFields.value.filter((f) => f !== name);
  } else if (selectedFields.value.length < 8) {
    selectedFields.value = [...selectedFields.value, name];
  }
}

function removeField(name: string): void {
  selectedFields.value = selectedFields.value.filter((f) => f !== name);
}

function clearHistory(): void {
  store.value.resetTimeOrigin();
}

function redraw(): void {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const dpr = window.devicePixelRatio || 1;
  const w = canvasWidth.value;
  const h = chartHeight.value;
  canvas.width = Math.floor(w * dpr);
  canvas.height = Math.floor(h * dpr);
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  const { tMin, tMax } = store.value.visibleRange();
  const { vMin, vMax } = store.value.valueRange(tMin, tMax);
  drawTimeSeriesChart(ctx, w, h, activeSeries.value, tMin, tMax, vMin, vMax);
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
    for (const f of selectedFields.value) {
      const v = values[f];
      if (v !== undefined) store.value.addSample(f, v);
    }
    redraw();
  },
);

watch(activeSeries, () => redraw(), { deep: true });
watch(chartHeight, () => redraw());
</script>

<template>
  <div class="output-chart" ref="containerRef">
    <div class="toolbar">
      <div class="field-picker">
        <label class="picker-label" for="chart-field-filter">Параметры Output</label>
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
              :class="{ active: selectedFields.includes(f.name) }"
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

    <div class="canvas-wrap">
      <canvas ref="canvasRef" class="chart-canvas" />
      <p v-if="!snapshot.connected" class="overlay-hint">Подключите ECU для записи кривых</p>
      <p v-else-if="selectedFields.length === 0" class="overlay-hint">
        Выберите параметры через поиск выше
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
