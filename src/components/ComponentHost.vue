<script setup lang="ts">
import { computed } from "vue";
import type { ComponentInstance } from "../core/types";
import { requireRegisteredComponent } from "../core/registry";
import { resolveBinding } from "../core/data-context";
import { childPath } from "../core/instance";
import { resolveNavActivatable, resolveNavSelectable } from "../core/navFlags";
import ComponentHost from "./ComponentHost.vue";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
}>();
const emit = defineEmits<{
  (e: "select-path", path: string): void;
  (e: "activate-path", path: string): void;
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
const hasChildren = computed(() => childInstances.value.length > 0);
const isLeaf = computed(() => !!entry.value && !("error" in entry.value) && !hasChildren.value);
const isNavLeaf = computed(() => {
  if (!isLeaf.value || !entry.value || "error" in entry.value) return false;
  return resolveNavSelectable(props.instance);
});
const navActivatable = computed(() => resolveNavActivatable(props.instance));

function onNodeMouseDown(): void {
  emit("select-path", props.path);
}
</script>

<template>
  <p v-if="entry && 'error' in entry" class="host-error">{{ entry.error }}</p>
  <p v-else-if="!instance.type" class="host-error">
    Компонент без type (проверьте $component в YAML — config-loader должен разрешить ссылку).
  </p>
  <div
    v-else-if="entry && isNavLeaf"
    class="host-node nav-node"
    data-nav-node="1"
    :data-nav-path="path"
    :data-nav-activatable="navActivatable ? undefined : 'false'"
    tabindex="-1"
    @mousedown.stop="onNodeMouseDown"
  >
    <component
      :is="entry.component"
      :instance="instance"
      :path="path"
      :props="instance.props ?? {}"
      :binding="binding"
      :meta="entry.meta"
    />
  </div>
  <component
    v-else-if="entry && isLeaf"
    :is="entry.component"
    :instance="instance"
    :path="path"
    :props="instance.props ?? {}"
    :binding="binding"
    :meta="entry.meta"
  />
  <component
    v-else-if="entry"
    :is="entry.component"
    :instance="instance"
    :path="path"
    :props="instance.props ?? {}"
    :binding="binding"
    :meta="entry.meta"
  >
    <template v-if="hasChildren">
      <ComponentHost
        v-for="(child, index) in childInstances"
        :key="child.id ?? `${path}-${index}`"
        :instance="child"
        :path="childPath(path, index, child)"
        @select-path="emit('select-path', $event)"
        @activate-path="emit('activate-path', $event)"
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

.host-node {
  border-radius: var(--radius-sm);
  width: 100%;
  max-width: 100%;
  min-width: 0;
  align-self: stretch;
  box-sizing: border-box;
}
</style>
