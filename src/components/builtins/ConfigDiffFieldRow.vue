<script setup lang="ts">
import { computed } from "vue";
import { initConfig, useConfig } from "../../composables/useConfig";
import {
  formatDiffValue,
  type ConfigDiffEntry,
  type DiffSide,
} from "../../composables/useConfigDiff";

const props = defineProps<{
  entry: ConfigDiffEntry;
  choice: DiffSide;
  label?: string;
}>();

const emit = defineEmits<{
  pick: [side: DiffSide];
}>();

void initConfig();

const { getFieldInfo } = useConfig();

const title = computed(() => props.label?.trim() || props.entry.field);

const units = computed(() => getFieldInfo(props.entry.field)?.units ?? "");

const enumOptions = computed(() => {
  const info = getFieldInfo(props.entry.field);
  return info?.options ?? [];
});

function enumLabel(v: number): string {
  const opt = enumOptions.value.find((o) => o.value === v);
  return opt?.label ?? formatDiffValue(v, "enum");
}

function scalarDisplay(v: number): string {
  const u = units.value;
  const s = formatDiffValue(v, props.entry.ty);
  return u ? `${s} ${u}` : s;
}

function pick(side: DiffSide): void {
  emit("pick", side);
}
</script>

<template>
  <div class="diff-field-row" :class="`diff-field-row--${entry.ty}`">
    <div class="diff-field-head">
      <span class="diff-field-name">{{ title }}</span>
      <span class="diff-field-meta">{{ entry.field }} · {{ entry.ty }}</span>
    </div>
    <div class="diff-columns">
      <button
        type="button"
        class="diff-col diff-col--project"
        :class="{ 'diff-col--selected': choice === 'project' }"
        @click="pick('project')"
      >
        <span class="diff-col-label">Проект</span>
        <span class="diff-col-value">
          {{
            entry.ty === "enum"
              ? enumLabel(entry.project)
              : scalarDisplay(entry.project)
          }}
        </span>
      </button>
      <button
        type="button"
        class="diff-col diff-col--ecu"
        :class="{ 'diff-col--selected': choice === 'ecu' }"
        @click="pick('ecu')"
      >
        <span class="diff-col-label">ECU</span>
        <span class="diff-col-value">
          {{ entry.ty === "enum" ? enumLabel(entry.ecu) : scalarDisplay(entry.ecu) }}
        </span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.diff-field-row {
  padding: 0.65rem 0;
  border-bottom: 1px solid var(--color-border);
}

.diff-field-row:last-child {
  border-bottom: none;
}

.diff-field-head {
  display: flex;
  flex-wrap: wrap;
  gap: 0.25rem 0.65rem;
  align-items: baseline;
  margin-bottom: 0.45rem;
}

.diff-field-name {
  font-size: 0.9rem;
  font-weight: 600;
  color: var(--color-text);
}

.diff-field-meta {
  font-size: 0.72rem;
  color: var(--color-text-muted);
}

.diff-columns {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 0.5rem;
}

.diff-col {
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
  padding: 0.5rem 0.6rem;
  border-radius: var(--radius-md);
  border: 2px solid var(--color-border-strong);
  background: var(--color-bg-muted);
  text-align: left;
  cursor: pointer;
}

.diff-col--project.diff-col--selected {
  border-color: var(--color-accent, #2a7de1);
  background: color-mix(in srgb, var(--color-accent, #2a7de1) 10%, var(--color-bg));
}

.diff-col--ecu.diff-col--selected {
  border-color: var(--color-warning, #c07020);
  background: color-mix(in srgb, var(--color-warning, #c07020) 12%, var(--color-bg));
}

.diff-col-label {
  font-size: 0.68rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-text-muted);
}

.diff-col-value {
  font-size: 0.95rem;
  font-variant-numeric: tabular-nums;
  line-height: 1.3;
}
</style>
