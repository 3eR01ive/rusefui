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

const entry = computed(() => requireRegisteredComponent(props.instance.type));
const binding = computed(() => resolveBinding(props.instance.bind));
const resolved = entry.value;

const childInstances = computed(() => props.instance.children ?? []);
</script>

<template>
  <component
    :is="resolved.component"
    :instance="instance"
    :path="path"
    :props="instance.props ?? {}"
    :binding="binding"
    :meta="resolved.meta"
  >
    <template v-if="resolved.meta.isContainer && childInstances.length">
      <ComponentHost
        v-for="(child, index) in childInstances"
        :key="child.id ?? `${path}-${index}`"
        :instance="child"
        :path="childPath(path, index, child)"
      />
    </template>
  </component>
</template>
