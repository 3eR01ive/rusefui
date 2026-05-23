<script setup lang="ts">
import { computed, onMounted } from "vue";
import type { ComponentInstance, ComponentMeta, DataBinding } from "../../core/types";
import { initOutputChannels, useOutputChannels } from "../../composables/useOutputChannels";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const bind = computed(() => props.instance.bind as DataBinding | undefined);
const fieldName = computed(() => bind.value?.field ?? "");
const label = computed(
  () => String(props.props.label ?? (fieldName.value || "—")),
);
const unit = computed(() => String(props.props.unit ?? ""));
const decimals = computed(() => Number(props.props.decimals ?? 1));

const { snapshot, getField } = useOutputChannels();

onMounted(() => {
  void initOutputChannels();
});

const rawValue = computed(() => {
  if (!fieldName.value) return null;
  return getField(fieldName.value);
});

const displayValue = computed(() => {
  if (!snapshot.value.connected) return "—";
  const v = rawValue.value;
  if (v === null) return "—";
  if (Number.isInteger(v) && decimals.value === 0) return String(v);
  return v.toFixed(decimals.value);
});

const stale = computed(
  () => snapshot.value.connected && rawValue.value === null && !!fieldName.value,
);
</script>

<template>
  <div class="output-value" :class="{ stale }">
    <span class="ov-label">{{ label }}</span>
    <span class="ov-value">{{ displayValue }}</span>
    <span v-if="unit" class="ov-unit">{{ unit }}</span>
    <span v-if="snapshot.lastError" class="ov-error">{{ snapshot.lastError }}</span>
    <span v-else class="ov-meta">output · {{ fieldName || "?" }}</span>
  </div>
</template>

<style scoped>
.output-value {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  padding: 0.65rem 0.75rem;
  background: var(--color-bg-muted);
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border);
  min-width: 120px;
}

.output-value.stale {
  border-style: dashed;
  opacity: 0.85;
}

.ov-label {
  font-size: 0.75rem;
  color: var(--color-gray);
  font-weight: 500;
}

.ov-value {
  font-size: 1.25rem;
  font-weight: 600;
  color: var(--color-text);
  font-variant-numeric: tabular-nums;
}

.ov-unit {
  font-size: 0.8rem;
  color: var(--color-text-muted);
}

.ov-meta,
.ov-error {
  font-size: 0.68rem;
  color: var(--color-text-subtle);
}

.ov-error {
  color: var(--color-error);
}
</style>
