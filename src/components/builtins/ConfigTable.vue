<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { initConfig, useConfig } from "../../composables/useConfig";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

void initConfig();

const { snapshot, getArray, setArrayValue } = useConfig();

const title = computed(() => String(props.props.title ?? ""));
const variant = computed(() =>
  String(props.props.variant ?? "table") === "curve" ? "curve" : "table",
);
const xField = computed(() => String(props.props.xBins ?? ""));
const yField = computed(() => String(props.props.yBins ?? ""));
const zField = computed(() => String(props.props.zBins ?? ""));
const xLabel = computed(() => String(props.props.xLabel ?? "X"));
const yLabel = computed(() => String(props.props.yLabel ?? "Y"));

const xValues = ref<number[]>([]);
const yAxisValues = ref<number[]>([]);
const zValues = ref<number[]>([]);
const loading = ref(false);
const localError = ref<string | null>(null);
const saving = ref(false);

const isCurve = computed(() => variant.value === "curve");
const valueField = computed(() => (isCurve.value ? yField.value : zField.value));

const cols = computed(() => {
  if (isCurve.value) return 1;
  const n = xValues.value.length;
  return n > 0 ? n : Math.max(1, Math.round(Math.sqrt(zValues.value.length)));
});

const rows = computed(() => {
  if (isCurve.value) {
    return Math.max(
      xValues.value.length,
      zValues.value.length,
      yAxisValues.value.length,
    );
  }
  const n = yAxisValues.value.length;
  if (n > 0) return n;
  const c = cols.value;
  return c > 0 ? Math.max(1, Math.ceil(zValues.value.length / c)) : 1;
});

const colIndices = computed(() => Array.from({ length: cols.value }, (_, i) => i));
const rowIndices = computed(() => Array.from({ length: rows.value }, (_, i) => i));

const disabled = computed(
  () =>
    !valueField.value ||
    !snapshot.value.connected ||
    !snapshot.value.loaded ||
    snapshot.value.loading ||
    loading.value ||
    saving.value,
);

function fmt(v: number): string {
  if (!Number.isFinite(v)) return "";
  if (Number.isInteger(v)) return String(v);
  const s = v.toFixed(3);
  return s.replace(/\.?0+$/, "");
}

function cellIndex(row: number, col: number): number {
  return row * cols.value + col;
}

function cellValue(row: number, col: number): number | null {
  const idx = cellIndex(row, col);
  const v = zValues.value[idx];
  return v === undefined ? null : v;
}

async function reload() {
  if (!snapshot.value.loaded) return;
  loading.value = true;
  localError.value = null;
  try {
    if (xField.value) {
      xValues.value = await getArray(xField.value);
    }
    if (yField.value && !isCurve.value) {
      yAxisValues.value = await getArray(yField.value);
    }
    if (isCurve.value && yField.value) {
      zValues.value = await getArray(yField.value);
      if (!xField.value) {
        xValues.value = zValues.value.map((_, i) => i);
      }
    } else if (valueField.value) {
      zValues.value = await getArray(valueField.value);
    }
    if (isCurve.value && xField.value && xValues.value.length === 0) {
      xValues.value = await getArray(xField.value);
    }
  } catch (e) {
    localError.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

watch(
  () => snapshot.value.loaded,
  (loaded) => {
    if (loaded) void reload();
  },
  { immediate: true },
);

async function commitCell(row: number, col: number, raw: string) {
  if (disabled.value || !valueField.value) return;
  const parsed = Number(raw.trim().replace(",", "."));
  if (!Number.isFinite(parsed)) {
    localError.value = "некорректное число";
    return;
  }
  const idx = cellIndex(row, col);
  const current = cellValue(row, col);
  if (current !== null && Math.abs(current - parsed) < 1e-9) return;

  saving.value = true;
  localError.value = null;
  try {
    await setArrayValue(valueField.value, idx, parsed);
    zValues.value[idx] = parsed;
  } catch (e) {
    localError.value = e instanceof Error ? e.message : String(e);
    await reload();
  } finally {
    saving.value = false;
  }
}

const statusText = computed(() => {
  if (localError.value) return localError.value;
  if (saving.value) return "сохранение…";
  if (loading.value || snapshot.value.loading) return "загрузка…";
  if (!snapshot.value.connected) return "нет подключения";
  if (!snapshot.value.loaded) return "ожидание config…";
  return isCurve.value ? "кривая" : "таблица";
});
</script>

<template>
  <div class="config-table">
    <header v-if="title" class="table-head">
      <h4 class="table-title">{{ title }}</h4>
      <span class="table-badge" :class="{ 'table-badge--error': !!localError }">
        {{ statusText }}
      </span>
    </header>

    <div v-if="isCurve" class="table-scroll">
      <table class="grid">
        <thead>
          <tr>
            <th>{{ xLabel }}</th>
            <th>{{ yLabel }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in rowIndices" :key="row">
            <td class="axis-cell">{{ fmt(xValues[row] ?? row) }}</td>
            <td>
              <input
                type="text"
                class="cell-input"
                :disabled="disabled"
                :value="fmt(cellValue(row, 0) ?? 0)"
                @change="
                  commitCell(row, 0, ($event.target as HTMLInputElement).value)
                "
              />
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div v-else class="table-scroll">
      <table class="grid">
        <thead>
          <tr>
            <th class="corner">{{ yLabel }} \ {{ xLabel }}</th>
            <th v-for="col in colIndices" :key="`x-${col}`" class="axis-head">
              {{ fmt(xValues[col] ?? col) }}
            </th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in rowIndices" :key="`row-${row}`">
            <th class="axis-head">{{ fmt(yAxisValues[row] ?? row) }}</th>
            <td v-for="col in colIndices" :key="`c-${row}-${col}`">
              <input
                type="text"
                class="cell-input"
                :disabled="disabled"
                :value="fmt(cellValue(row, col) ?? 0)"
                @change="
                  commitCell(row, col, ($event.target as HTMLInputElement).value)
                "
              />
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<style scoped>
.config-table {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  width: 100%;
  min-width: 0;
}

.table-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 0.75rem;
}

.table-title {
  margin: 0;
  font-size: 0.92rem;
  font-weight: 600;
}

.table-badge {
  font-size: 0.7rem;
  color: var(--color-text-subtle);
}

.table-badge--error {
  color: var(--color-danger, #c0392b);
}

.table-scroll {
  overflow: auto;
  max-width: 100%;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
}

.grid {
  border-collapse: collapse;
  font-size: 0.78rem;
  min-width: 100%;
}

.grid th,
.grid td {
  border: 1px solid var(--color-border);
  padding: 0;
}

.axis-head,
.axis-cell,
.corner {
  background: var(--color-bg-muted);
  color: var(--color-text-muted);
  font-weight: 500;
  padding: 0.35rem 0.5rem;
  white-space: nowrap;
}

.corner {
  position: sticky;
  left: 0;
  z-index: 1;
}

.cell-input {
  width: 4.5rem;
  padding: 0.35rem 0.45rem;
  border: none;
  background: var(--color-bg);
  color: var(--color-text);
  text-align: right;
}

.cell-input:disabled {
  background: var(--color-bg-muted);
  color: var(--color-text-muted);
}
</style>
