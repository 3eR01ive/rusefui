<script setup lang="ts">
import { computed, onMounted, ref, shallowRef, watch } from "vue";
import { parse as parseYaml } from "yaml";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import ComponentHost from "../ComponentHost.vue";
import {
  loadGeneratedPanelYaml,
  normalizeGeneratedPanelFile,
  panelsEpoch,
} from "../../composables/useIniPanels";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const children = shallowRef<ComponentInstance[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);

const panelFile = computed(() =>
  normalizeGeneratedPanelFile(String(props.props.file ?? "")),
);

async function loadPanel(): Promise<void> {
  if (!panelFile.value) {
    error.value = "Не задан props.file для generated-panel";
    children.value = [];
    return;
  }
  loading.value = true;
  error.value = null;
  try {
    const text = await loadGeneratedPanelYaml(panelFile.value);
    const doc = parseYaml(text) as { children?: ComponentInstance[] };
    children.value = doc.children ?? [];
  } catch (e) {
    children.value = [];
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  void loadPanel();
});

watch([panelFile, panelsEpoch], () => {
  void loadPanel();
});
</script>

<template>
  <div class="generated-panel">
    <p v-if="loading" class="generated-panel-hint">Загрузка панели из INI…</p>
    <p v-else-if="error" class="generated-panel-error">{{ error }}</p>
    <ComponentHost
      v-for="(child, index) in children"
      :key="child.id ?? `${path}-${index}`"
      :instance="child"
      :path="`${path}/gen/${index}`"
    />
  </div>
</template>

<style scoped>
.generated-panel {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  width: 100%;
  min-width: 0;
}

.generated-panel-hint {
  margin: 0;
  font-size: 0.82rem;
  color: var(--color-text-subtle);
}

.generated-panel-error {
  margin: 0;
  font-size: 0.82rem;
  color: var(--color-error);
}
</style>
