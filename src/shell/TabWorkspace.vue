<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { loadAppConfig, type LoadedAppConfig } from "../core/config-loader";
import type { ResolvedTab } from "../core/types";
import ComponentHost from "../components/ComponentHost.vue";

const config = ref<LoadedAppConfig | null>(null);
const loadError = ref<string | null>(null);
const activeTabId = ref<string>("");

const tabs = computed<ResolvedTab[]>(() => config.value?.tabs ?? []);
onMounted(async () => {
  try {
    config.value = await loadAppConfig();
    if (config.value.tabs.length) {
      activeTabId.value = config.value.tabs[0].id;
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
      <nav class="tab-bar" role="tablist">
        <button
          v-for="tab in tabs"
          :key="tab.id"
          type="button"
          class="tab-btn"
          :class="{ active: tab.id === activeTabId }"
          role="tab"
          :aria-selected="tab.id === activeTabId"
          @click="activeTabId = tab.id"
        >
          {{ tab.title }}
        </button>
      </nav>

      <div
        v-for="tab in tabs"
        v-show="tab.id === activeTabId"
        :key="tab.id"
        class="tab-panel"
        role="tabpanel"
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

.tab-bar {
  display: flex;
  gap: 0.35rem;
  margin-bottom: 1.25rem;
  padding-bottom: 0;
  border-bottom: 1px solid var(--color-border);
  width: 100%;
}

.tab-btn {
  flex: 1 1 0;
  min-width: 8rem;
  padding: 0.65rem 1.25rem;
  border: none;
  border-bottom: 2px solid transparent;
  margin-bottom: -1px;
  background: transparent;
  color: var(--color-text-muted);
  font-weight: 500;
  border-radius: var(--radius-sm) var(--radius-sm) 0 0;
  white-space: nowrap;
  text-align: center;
}

.tab-btn:hover {
  color: var(--color-text);
  background: var(--color-bg-muted);
}

.tab-btn.active {
  color: var(--color-accent-hover);
  border-bottom-color: var(--color-accent);
  background: var(--color-bg-elevated);
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
