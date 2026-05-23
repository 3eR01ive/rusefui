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

const title = computed(() => String(props.props.title ?? ""));
</script>

<template>
  <section class="section">
    <h3 v-if="title" class="section-title">{{ title }}</h3>
    <div class="section-body">
      <slot />
    </div>
  </section>
</template>

<style scoped>
.section {
  width: 100%;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  padding: 1.1rem 1.35rem;
  box-shadow: var(--shadow-card);
  box-sizing: border-box;
}

.section-title {
  margin: 0 0 1rem;
  font-size: 1rem;
  font-weight: 600;
  color: var(--color-text);
}

.section-body {
  display: flex;
  flex-direction: column;
  gap: 0.85rem;
}
</style>
