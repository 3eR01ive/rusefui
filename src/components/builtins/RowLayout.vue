<script setup lang="ts">
import type { ComponentInstance, ComponentMeta } from "../../core/types";

defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();
</script>

<template>
  <div class="layout-row">
    <slot />
  </div>
</template>

<style scoped>
.layout-row {
  display: flex;
  flex-wrap: wrap;
  gap: 1rem;
  align-items: stretch;
  width: 100%;
}

.layout-row > :deep(*) {
  flex: 1 1 12rem;
  min-width: 10rem;
}
</style>
