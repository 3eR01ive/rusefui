<script setup lang="ts">
import { computed } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import type { DataBinding } from "../../core/types";

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
</script>

<template>
  <div class="scalar-field">
    <label class="field-label">{{ label }}</label>
    <input
      type="text"
      class="field-input"
      disabled
      :placeholder="`config.${field} (скоро)`"
    />
    <span class="field-badge">edit · config</span>
  </div>
</template>

<style scoped>
.scalar-field {
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
  max-width: 280px;
}

.field-label {
  font-size: 0.78rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-gray);
  font-weight: 500;
}

.field-input {
  padding: 0.5rem 0.65rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-muted);
  color: var(--color-text-muted);
}

.field-badge {
  font-size: 0.7rem;
  color: var(--color-text-subtle);
}
</style>
