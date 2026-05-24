<script setup lang="ts">
import { toRef } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useConfigGrid } from "../../composables/useConfigGrid";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const propsRef = toRef(props, "props");

const {
  title,
  xLabel,
  yLabel,
  colIndices,
  rowIndices,
  disabled,
  fmt,
  cellValue,
  commitCell,
  statusText,
  localError,
  xValues,
  yAxisValues,
} = useConfigGrid({ kind: "table", props: propsRef });
</script>

<template>
  <div class="config-table">
    <header v-if="title" class="grid-head">
      <h4 class="grid-title">{{ title }}</h4>
      <span class="grid-badge" :class="{ 'grid-badge--error': !!localError }">
        {{ statusText }}
      </span>
    </header>

    <div class="grid-scroll">
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

.grid-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 0.75rem;
}

.grid-title {
  margin: 0;
  font-size: 0.92rem;
  font-weight: 600;
}

.grid-badge {
  font-size: 0.7rem;
  color: var(--color-text-subtle);
}

.grid-badge--error {
  color: var(--color-danger, #c0392b);
}

.grid-scroll {
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
