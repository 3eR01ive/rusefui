<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, shallowRef, watch } from "vue";
import { parse as parseYaml } from "yaml";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { childPath } from "../../core/instance";
import ComponentHost from "../ComponentHost.vue";
import {
  loadGeneratedPanelYaml,
  normalizeGeneratedPanelFile,
  panelsEpoch,
} from "../../composables/useIniPanels";
import { setNavExtension } from "../../composables/useWorkspaceNav";

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

onMounted(() => { void loadPanel(); });
onUnmounted(() => { setNavExtension(props.path, null); });

watch([panelFile, panelsEpoch], () => { void loadPanel(); });

// Регистрируем детей в nav-дереве: basePath = props.path, collectNavPathsFromTree
// генерирует те же пути что childPath(props.path, index, child) в шаблоне.
watch(children, (c) => {
  setNavExtension(props.path,
    c.length ? { type: 'composite', id: props.instance.id ?? 'gen', children: c } : null
  );
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
      :path="childPath(path, index, child)"
    />
  </div>
</template>

<style scoped>
.generated-panel {
  /* Секции INI узкие по контенту, но каждая тянется на всю ширину и оставляет
     справа пустоту. Раскладываем их адаптивной сеткой: на широком экране —
     несколько колонок (секции + график рядом), на узком — один столбец. */
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 26rem), 1fr));
  align-items: start;
  gap: 0.75rem;
  width: 100%;
  min-width: 0;
}

/* Загрузка/ошибка не должны участвовать в сетке как колонка. */
.generated-panel-hint,
.generated-panel-error {
  grid-column: 1 / -1;
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
