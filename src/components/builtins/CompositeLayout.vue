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
  <div class="composite-root">
    <slot />
  </div>
</template>

<style scoped>
.composite-root {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  width: 100%;
  max-width: 100%;
  min-width: 0;
  align-items: stretch;
}
</style>
