<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { loadAppConfig, type LoadedAppConfig } from "../core/config-loader";
import type { ResolvedTab } from "../core/types";
import ComponentHost from "../components/ComponentHost.vue";
import { activeTabId } from "../composables/useTabState";
import { tabOrder } from "../composables/useHotkeys";

const config = ref<LoadedAppConfig | null>(null);
const loadError = ref<string | null>(null);
const tabs = computed<ResolvedTab[]>(() => config.value?.tabs ?? []);

/** Expose resolved tabs so AppShell can render the icon bar. */
defineExpose({ tabs });

onMounted(async () => {
  try {
    config.value = await loadAppConfig();
    const ids = (config.value?.tabs ?? []).map((t) => t.id);
    tabOrder.value = ids;
    if (ids.length && !activeTabId.value) {
      activeTabId.value = ids[0]!;
    }
  } catch (e) {
    loadError.value = String(e);
  }
});
</script>

<template>
  <div class="workspace">
    <p v-if="loadError" class="workspace-error">{{ loadError }}</p>

    <template v-else-if="config">
      <div
        v-for="tab in tabs"
        v-show="tab.id === activeTabId"
        :key="tab.id"
        class="tab-panel"
        role="tabpanel"
        :aria-label="tab.title"
      >
        <ComponentHost :instance="tab.root" :path="`tab/${tab.id}`" />
      </div>
    </template>

    <p v-else class="workspace-loading">Загрузка конфигурации…</p>
  </div>
</template>

<style scoped>
.workspace {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.tab-panel {
  flex: 1;
  min-height: 0;
  width: 100%;
  overflow: auto;
}

.workspace-loading,
.workspace-error {
  margin: 0;
  padding: 1rem;
  color: var(--color-text-muted);
}

.workspace-error {
  color: var(--color-error);
  background: var(--color-error-bg);
  border-radius: var(--radius-md);
  border-left: 3px solid var(--color-accent);
}
</style>
