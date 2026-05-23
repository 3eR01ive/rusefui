<script setup lang="ts">
import { computed } from "vue";
import type { ComponentInstance, ComponentMeta, DataBinding } from "../../core/types";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const bind = computed(() => props.instance.bind as DataBinding | undefined);
const label = computed(
  () => String(props.props.label ?? bind.value?.field ?? "—"),
);
const field = computed(() => bind.value?.field ?? "—");
const unit = computed(() => String(props.props.unit ?? ""));
</script>

<template>
  <div class="output-value">
    <span class="ov-label">{{ label }}</span>
    <span class="ov-value">—</span>
    <span v-if="unit" class="ov-unit">{{ unit }}</span>
    <span class="ov-meta">output · {{ field }}</span>
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

.ov-label {
  font-size: 0.75rem;
  color: var(--color-gray);
  font-weight: 500;
}

.ov-value {
  font-size: 1.25rem;
  font-weight: 600;
  color: var(--color-text);
}

.ov-unit {
  font-size: 0.8rem;
  color: var(--color-text-muted);
}

.ov-meta {
  font-size: 0.68rem;
  color: var(--color-text-subtle);
}
</style>
