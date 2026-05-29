<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { loadAppConfig, type LoadedAppConfig } from "../core/config-loader";
import type { ResolvedTab } from "../core/types";
import ComponentHost from "../components/ComponentHost.vue";
import { activeTabId } from "../composables/useTabState";
import { tabOrder } from "../composables/useHotkeys";
import {
  registerTabBinding,
  unregisterTabBinding,
} from "../composables/useKeyboardRouter";
import {
  activateComponent,
  activePath,
  collectAllNavPaths,
  ensureSelectedInNav,
  focusComponent,
  isFilterNavPath,
  isNavActivatablePath,
  moveNavSelection,
  navExtensions,
  navMenuPaths,
  navMode,
  refreshNavDimming,
  resetWorkspaceNav,
  selectedPath,
  selectComponent,
  setNavPaths,
} from "../composables/useWorkspaceNav";

const config = ref<LoadedAppConfig | null>(null);
const loadError = ref<string | null>(null);
const tabs = computed<ResolvedTab[]>(() => config.value?.tabs ?? []);
const workspaceRef = ref<HTMLElement | null>(null);

defineExpose({ tabs });

function syncActiveTab(ids: string[]): void {
  tabOrder.value = ids;
  if (!ids.length) {
    activeTabId.value = "";
    return;
  }
  if (!ids.includes(activeTabId.value)) {
    activeTabId.value = ids[0]!;
  }
}

function rebuildNavPaths(): void {
  const tab = tabs.value.find((t) => t.id === activeTabId.value);
  if (!tab) {
    setNavPaths([]);
    return;
  }
  setNavPaths(collectAllNavPaths(tab.root, `tab/${tab.id}`));
  ensureSelectedInNav();
  void nextTick(refreshNavDimming);
}

watch(navExtensions, () => {
  rebuildNavPaths();
}, { deep: true });

watch(navMenuPaths, () => {
  rebuildNavPaths();
}, { deep: true });

function activateSelection(): void {
  if (!selectedPath.value) return;
  if (!isNavActivatablePath(selectedPath.value)) {
    if (isFilterNavPath(selectedPath.value)) {
      focusComponent(selectedPath.value);
    }
    return;
  }
  activateComponent(selectedPath.value);
  focusComponent(selectedPath.value);
}

function onTabKeydown(e: KeyboardEvent): boolean {
  if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) {
    return false;
  }

  if (e.key === "Enter") {
    activateSelection();
    return true;
  }

  if (
    e.key === "ArrowUp" ||
    e.key === "ArrowDown" ||
    e.key === "ArrowLeft" ||
    e.key === "ArrowRight"
  ) {
    moveNavSelection(e.key);
    return true;
  }

  return false;
}

function resetNavForTab(): void {
  resetWorkspaceNav();
  void nextTick(() => {
    rebuildNavPaths();
  });
}

onMounted(async () => {
  registerTabBinding(onTabKeydown);
  try {
    config.value = await loadAppConfig();
    syncActiveTab((config.value?.tabs ?? []).map((t) => t.id));
    resetNavForTab();
  } catch (e) {
    loadError.value = String(e);
  }
});

onUnmounted(() => {
  unregisterTabBinding();
});

watch(activeTabId, () => {
  resetNavForTab();
});

watch([navMode, activePath], () => {
  void nextTick(refreshNavDimming);
});
</script>

<template>
  <div ref="workspaceRef" class="workspace">
    <p v-if="loadError" class="workspace-error">{{ loadError }}</p>

    <template v-else-if="config">
      <div
        v-for="tab in tabs"
        v-show="tab.id === activeTabId"
        :key="tab.id"
        class="tab-panel"
        :data-tab-id="tab.id"
        role="tabpanel"
        :aria-label="tab.title"
      >
        <ComponentHost
          :instance="tab.root"
          :path="`tab/${tab.id}`"
          :selected-path="selectedPath"
          :active-path="activePath"
          :nav-mode="navMode"
          @select-path="selectComponent"
          @activate-path="(path) => { if (isNavActivatablePath(path)) { activateComponent(path); focusComponent(path); } }"
        />
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
  height: 100%;
  overflow: hidden;
}

.tab-panel {
  flex: 1;
  min-height: 0;
  width: 100%;
  overflow: auto;
  box-sizing: border-box;
  padding-top: 0.35rem;
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
