<script setup lang="ts">
import { computed } from "vue";
import type { ComponentInstance } from "../core/types";
import { requireRegisteredComponent } from "../core/registry";
import { resolveBinding } from "../core/data-context";
import { childPath } from "../core/instance";
import ComponentHost from "./ComponentHost.vue";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
}>();

const entry = computed(() => {
  const type = props.instance.type;
  if (!type) {
    return null;
  }
  try {
    return requireRegisteredComponent(type);
  } catch (e) {
    return { error: String(e) } as const;
  }
});

const binding = computed(() => resolveBinding(props.instance.bind));

const childInstances = computed(() => props.instance.children ?? []);
</script>

<template>
  <p v-if="entry && 'error' in entry" class="host-error">{{ entry.error }}</p>
  <p v-else-if="!instance.type" class="host-error">
    Компонент без type (проверьте $component в YAML — config-loader должен разрешить ссылку).
  </p>
  <component
    v-else-if="entry"
    :is="entry.component"
    :instance="instance"
    :path="path"
    :props="instance.props ?? {}"
    :binding="binding"
    :meta="entry.meta"
  >
    <template v-if="entry.meta.isContainer && childInstances.length">
      <ComponentHost
        v-for="(child, index) in childInstances"
        :key="child.id ?? `${path}-${index}`"
        :instance="child"
        :path="childPath(path, index, child)"
      />
    </template>
  </component>
</template>

<style scoped>
.host-error {
  margin: 0.5rem 0;
  padding: 0.5rem 0.65rem;
  font-size: 0.85rem;
  color: var(--color-error);
  background: var(--color-error-bg);
  border-radius: var(--radius-sm);
  border-left: 3px solid var(--color-accent);
}
</style>
