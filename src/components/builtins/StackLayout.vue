<script setup lang="ts">
import type { ComponentInstance } from "../../core/types";
import type { ComponentMeta } from "../../core/types";

defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();
</script>

<template>
  <div class="layout-stack">
    <slot />
  </div>
</template>

<style scoped>
.layout-stack {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  width: 100%;
  --field-label-w: 9rem;
}
</style>
