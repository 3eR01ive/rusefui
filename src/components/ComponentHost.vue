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
  selectedPath?: string;
  activePath?: string;
  navMode?: "select" | "active";
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
const isLeaf = computed(() => !!entry.value && !("error" in entry.value) && !entry.value.meta.isContainer);
const navSelected = computed(
  () => props.navMode === "select" && props.selectedPath === props.path,
);
const navActive = computed(
  () => props.navMode === "active" && props.activePath === props.path,
);

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
    v-else-if="entry && isLeaf"
    class="host-node"
    :class="{ 'host-node--selected': navSelected, 'host-node--active': navActive }"
    data-nav-node="1"
    :data-nav-path="path"
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
        :selected-path="selectedPath"
        :active-path="activePath"
        :nav-mode="navMode"
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
}

.host-node--selected {
  outline: 2px solid rgba(59, 130, 246, 0.95);
  outline-offset: 2px;
}

.host-node--active {
  outline: 2px solid rgba(22, 163, 74, 0.95);
  outline-offset: 2px;
}
</style>
