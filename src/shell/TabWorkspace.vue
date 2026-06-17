<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { listen } from "@tauri-apps/api/event";
import { loadAppConfig, type LoadedAppConfig } from "../core/config-loader";
import { panelsEpoch } from "../composables/useIniPanels";
import type { ResolvedTab } from "../core/types";
import { loadCustomTabs, useCustomTabs } from "../composables/useCustomTabs";
import TabActivityScope from "../components/TabActivityScope.vue";
import TabCanvasLayer from "../components/TabCanvasLayer.vue";
import { activeTabId } from "../composables/useTabState";
import { tabOrder } from "../composables/useHotkeys";
import {
  registerTabBinding,
  unregisterTabBinding,
} from "../composables/useKeyboardRouter";
import {
  activateComponent,
  buildSpatialData,
  collectAllNavPaths,
  ensureSelectedInNav,
  focusComponent,
  isFilterNavPath,
  isNavActivatablePath,
  moveNavSelection,
  navExtensions,
  navMenuPaths,
  resetWorkspaceNav,
  selectComponent,
  selectedPath,
  setNavPaths,
  syncNavSelectionVisual,
} from "../composables/useWorkspaceNav";

const config = ref<LoadedAppConfig | null>(null);
const loadError = ref<string | null>(null);
const { customTabDefs } = useCustomTabs();

const tabs = computed<ResolvedTab[]>(() => {
  const yamlTabs = config.value?.tabs ?? [];
  const extraTabs: ResolvedTab[] = customTabDefs.value.map((def) => ({
    id: def.id,
    title: def.title,
    isCustom: true,
    root: { type: "stack", id: "custom_root", children: [] },
  }));
  return [...yamlTabs, ...extraTabs];
});

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
  if (!tab) { setNavPaths([]); return; }
  const paths = collectAllNavPaths(tab.root, `tab/${tab.id}`);
  setNavPaths(paths);
  ensureSelectedInNav();
  void nextTick(() => {
    buildSpatialData(paths);
    syncNavSelectionVisual(selectedPath.value);
  });
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
  // Enter (без модификаторов) → активировать выбранный компонент
  if (e.key === "Enter" && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
    activateSelection();
    return true;
  }

  // Ctrl+Arrow (без Alt/Shift) → навигация со стрелками
  if (
    (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey &&
    (e.key === "ArrowUp" || e.key === "ArrowDown" || e.key === "ArrowLeft" || e.key === "ArrowRight")
  ) {
    moveNavSelection(e.key as Parameters<typeof moveNavSelection>[0]);
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

async function loadWorkspaceConfig(): Promise<void> {
  try {
    config.value = await loadAppConfig();
    await loadCustomTabs();
    syncActiveTab(tabs.value.map((t) => t.id));
    resetNavForTab();
    loadError.value = null;
  } catch (e) {
    loadError.value = String(e);
  }
}

let unlistenReset: (() => void) | null = null;

onMounted(async () => {
  registerTabBinding(onTabKeydown);
  await loadWorkspaceConfig();
  unlistenReset = await listen("workspace-reset", () => {
    void loadWorkspaceConfig();
  });
});

watch(panelsEpoch, () => {
  void loadWorkspaceConfig();
});

onUnmounted(() => {
  unregisterTabBinding();
  unlistenReset?.();
});

watch(activeTabId, () => {
  resetNavForTab();
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
        <TabActivityScope :tab-id="tab.id">
          <TabCanvasLayer
            :tab="tab"
            @select-path="selectComponent"
            @activate-path="(path) => { if (isNavActivatablePath(path)) { activateComponent(path); focusComponent(path); } }"
          />
        </TabActivityScope>
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
  overflow: hidden auto;
  scrollbar-gutter: stable;
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
