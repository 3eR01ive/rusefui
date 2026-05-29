<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { loadAppConfig, type LoadedAppConfig } from "../core/config-loader";
import type { ResolvedTab } from "../core/types";
import ComponentHost from "../components/ComponentHost.vue";
import { activeTabId } from "../composables/useTabState";
import { tabOrder } from "../composables/useHotkeys";

const config = ref<LoadedAppConfig | null>(null);
const loadError = ref<string | null>(null);
const tabs = computed<ResolvedTab[]>(() => config.value?.tabs ?? []);
const navMode = ref<"select" | "active">("select");
const selectedPath = ref("");
const activePath = ref("");
const workspaceRef = ref<HTMLElement | null>(null);

/** Expose resolved tabs so AppShell can render the icon bar. */
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

function currentPanelEl(): HTMLElement | null {
  const root = workspaceRef.value;
  if (!root || !activeTabId.value) return null;
  return root.querySelector<HTMLElement>(`.tab-panel[data-tab-id="${activeTabId.value}"]`);
}

function navNodes(): HTMLElement[] {
  const panel = currentPanelEl();
  if (!panel) return [];
  return Array.from(panel.querySelectorAll<HTMLElement>('[data-nav-node="1"]'));
}

function setSelectedPath(path: string): void {
  if (navMode.value === "active") {
    deactivateActive();
  }
  selectedPath.value = path;
}

function ensureSelectedNode(): void {
  const nodes = navNodes();
  if (!nodes.length) {
    selectedPath.value = "";
    activePath.value = "";
    return;
  }
  const exists = nodes.some((n) => n.dataset.navPath === selectedPath.value);
  if (!exists) {
    selectedPath.value = nodes[0]!.dataset.navPath ?? "";
  }
}

function selectedIndex(nodes: HTMLElement[]): number {
  if (!selectedPath.value) return -1;
  return nodes.findIndex((n) => n.dataset.navPath === selectedPath.value);
}

function moveSelection(delta: -1 | 1): void {
  const nodes = navNodes();
  if (!nodes.length) return;
  const cur = selectedIndex(nodes);
  const next =
    cur < 0
      ? delta > 0
        ? 0
        : nodes.length - 1
      : Math.max(0, Math.min(cur + delta, nodes.length - 1));
  const node = nodes[next]!;
  selectedPath.value = node.dataset.navPath ?? "";
  node.scrollIntoView({ block: "nearest" });
}

function focusInsideNode(node: HTMLElement): void {
  const target =
    node.querySelector<HTMLElement>(
      '[data-nav-focus], .grid-scroll, button:not([disabled]), input:not([disabled]):not([tabindex="-1"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ) ?? node;
  target.focus({ preventScroll: true });
}

function activateSelection(): void {
  if (!selectedPath.value) return;
  const node = navNodes().find((n) => n.dataset.navPath === selectedPath.value);
  if (!node) return;
  activePath.value = selectedPath.value;
  navMode.value = "active";
  focusInsideNode(node);
}

function deactivateActive(): void {
  navMode.value = "select";
  activePath.value = "";
  const panel = currentPanelEl();
  if (panel?.contains(document.activeElement)) {
    (document.activeElement as HTMLElement | null)?.blur();
  }
}

function resetNavForTab(): void {
  navMode.value = "select";
  activePath.value = "";
  selectedPath.value = "";
  void nextTick(() => {
    ensureSelectedNode();
  });
}

function onGlobalKeydown(e: KeyboardEvent): void {
  if (e.defaultPrevented) return;
  if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
  if (!showingActiveTab.value) return;

  if (navMode.value === "active") {
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      deactivateActive();
    }
    return;
  }

  if (e.key === "Enter") {
    e.preventDefault();
    e.stopPropagation();
    activateSelection();
    return;
  }

  if (e.key === "ArrowDown" || e.key === "ArrowRight") {
    e.preventDefault();
    e.stopPropagation();
    moveSelection(1);
    return;
  }
  if (e.key === "ArrowUp" || e.key === "ArrowLeft") {
    e.preventDefault();
    e.stopPropagation();
    moveSelection(-1);
  }
}

const showingActiveTab = computed(() => !!activeTabId.value);

onMounted(async () => {
  try {
    config.value = await loadAppConfig();
    syncActiveTab((config.value?.tabs ?? []).map((t) => t.id));
    resetNavForTab();
  } catch (e) {
    loadError.value = String(e);
  }
  window.addEventListener("keydown", onGlobalKeydown, true);
});

onUnmounted(() => {
  window.removeEventListener("keydown", onGlobalKeydown, true);
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
        <ComponentHost
          :instance="tab.root"
          :path="`tab/${tab.id}`"
          :selected-path="selectedPath"
          :active-path="activePath"
          :nav-mode="navMode"
          @select-path="setSelectedPath"
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
