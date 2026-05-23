<script setup lang="ts">
import { computed } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const text = computed(() => String(props.props.text ?? ""));
const variant = computed(() => String(props.props.variant ?? "body"));
</script>

<template>
  <p class="text-block" :class="variant">{{ text }}</p>
</template>

<style scoped>
.text-block {
  margin: 0;
  color: var(--color-text-muted);
  font-size: 0.9rem;
  line-height: 1.5;
}

.text-block.hint {
  font-size: 0.8rem;
  color: var(--color-text-subtle);
}
</style>
