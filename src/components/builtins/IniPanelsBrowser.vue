<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, shallowRef, watch } from "vue";
import { parse as parseYaml } from "yaml";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import ComponentHost from "../ComponentHost.vue";
import { useTabEnterHandler } from "../../composables/useHotkeys";
import { useComponentBinding } from "../../composables/useKeyboardRouter";
import {
  activateComponent,
  activePath,
  focusComponent,
  isNavActivatablePath,
  navMode,
  navPresentation,
  selectedPath,
  selectComponent,
  setNavExtension,
  setNavMenuPaths,
} from "../../composables/useWorkspaceNav";

interface ManifestEntry {
  id: string;
  file: string;
  title: string;
  menuPath: string;
}

interface Manifest {
  iniSource: string;
  panelCount: number;
  panels: ManifestEntry[];
}

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const manifestPath = computed(
  () => String(props.props.manifestPath ?? "/config/components/generated/manifest.json"),
);

const manifest = shallowRef<Manifest | null>(null);
const loadError = ref<string | null>(null);
const filter = ref("");
const filterInputRef = ref<HTMLInputElement | null>(null);
const selectedId = ref<string>("");
const panelRoot = shallowRef<ComponentInstance | null>(null);
const panelLoading = ref(false);
const panelError = ref<string | null>(null);

function filterPath(): string {
  return `${props.path}/filter`;
}

function menuItemPath(id: string): string {
  return `${props.path}/menu/${id}`;
}

const groupedPanels = computed(() => {
  const list = manifest.value?.panels ?? [];
  const q = filter.value.trim().toLowerCase();
  const filtered = q
    ? list.filter(
        (p) =>
          p.title.toLowerCase().includes(q) ||
          p.id.toLowerCase().includes(q) ||
          p.menuPath.toLowerCase().includes(q),
      )
    : list;
  const groups = new Map<string, ManifestEntry[]>();
  for (const p of filtered) {
    const top = p.menuPath.split(" › ")[0] ?? "Other";
    const arr = groups.get(top) ?? [];
    arr.push(p);
    groups.set(top, arr);
  }
  return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b));
});

/** Плоский список панелей в порядке отображения sidebar (группы → пункты). */
const visibleMenuPanels = computed((): ManifestEntry[] => {
  const result: ManifestEntry[] = [];
  for (const [, items] of groupedPanels.value) {
    result.push(...items);
  }
  return result;
});

const menuNavPaths = computed((): string[] => {
  const paths = [filterPath()];
  for (const p of visibleMenuPanels.value) {
    paths.push(menuItemPath(p.id));
  }
  return paths;
});

function ensureGroupExpandedForEntry(entry: ManifestEntry): void {
  if (filter.value.trim()) return;
  const group = entry.menuPath.split(" › ")[0] ?? "Other";
  if (expandedGroups.value.has(group)) return;
  expandedGroups.value = new Set([...expandedGroups.value, group]);
}

function selectPanel(id: string): void {
  selectedId.value = id;
  selectComponent(menuItemPath(id));
}

function selectFilter(): void {
  selectComponent(filterPath());
}

function isFilterTypingKey(e: KeyboardEvent): boolean {
  if (e.ctrlKey || e.metaKey || e.altKey) return false;
  if (e.key === "Backspace" || e.key === "Delete") return true;
  return e.key.length === 1;
}

function applyFilterKey(input: HTMLInputElement, e: KeyboardEvent): void {
  const start = input.selectionStart ?? input.value.length;
  const end = input.selectionEnd ?? input.value.length;
  let next = input.value;
  let caret = start;

  if (e.key === "Backspace") {
    if (start === end && start > 0) {
      next = input.value.slice(0, start - 1) + input.value.slice(end);
      caret = start - 1;
    } else if (start !== end) {
      next = input.value.slice(0, start) + input.value.slice(end);
      caret = start;
    } else {
      return;
    }
  } else if (e.key === "Delete") {
    if (start === end && start < input.value.length) {
      next = input.value.slice(0, start) + input.value.slice(end + 1);
      caret = start;
    } else if (start !== end) {
      next = input.value.slice(0, start) + input.value.slice(end);
      caret = start;
    } else {
      return;
    }
  } else if (e.key.length === 1) {
    next = input.value.slice(0, start) + e.key + input.value.slice(end);
    caret = start + 1;
  } else {
    return;
  }

  filter.value = next;
  void nextTick(() => input.setSelectionRange(caret, caret));
}

function onFilterKeydown(e: KeyboardEvent): boolean {
  const input = filterInputRef.value;
  if (!input || !isFilterTypingKey(e)) return false;
  if (document.activeElement === input) return false;

  input.focus({ preventScroll: true });
  applyFilterKey(input, e);
  return true;
}

useComponentBinding(filterPath(), onFilterKeydown);

function focusFilterInput(): void {
  void nextTick(() => {
    const input = filterInputRef.value;
    if (!input) return;
    input.focus({ preventScroll: true });
  });
}

async function loadManifest(): Promise<void> {
  loadError.value = null;
  try {
    const res = await fetch(manifestPath.value);
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
    manifest.value = (await res.json()) as Manifest;
    if (manifest.value.panels.length && !selectedId.value) {
      const first = manifest.value.panels[0]!;
      selectedId.value = first.id;
      ensureGroupExpandedForEntry(first);
    }
  } catch (e) {
    loadError.value = String(e);
  }
}

async function loadPanel(id: string): Promise<void> {
  const entry = manifest.value?.panels.find((p) => p.id === id);
  if (!entry) {
    panelRoot.value = null;
    return;
  }
  panelLoading.value = true;
  panelError.value = null;
  try {
    const url = `/config/components/generated/${entry.file}`;
    const res = await fetch(url);
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
    const doc = parseYaml(await res.text()) as {
      id: string;
      children: ComponentInstance[];
    };
    panelRoot.value = {
      id: doc.id,
      type: "composite",
      children: doc.children ?? [],
    };
  } catch (e) {
    panelRoot.value = null;
    panelError.value = String(e);
  } finally {
    panelLoading.value = false;
  }
}

useTabEnterHandler("ini-preview", () => {
  selectFilter();
  focusComponent(filterPath());
});

onMounted(() => {
  void loadManifest();
});

watch(selectedId, (id) => {
  if (id) {
    const entry = manifest.value?.panels.find((p) => p.id === id);
    if (entry) ensureGroupExpandedForEntry(entry);
    void loadPanel(id);
  }
});

watch(selectedPath, (path) => {
  const menuPrefix = `${props.path}/menu/`;
  if (path.startsWith(menuPrefix)) {
    const id = path.slice(menuPrefix.length);
    const entry = manifest.value?.panels.find((p) => p.id === id);
    if (entry && visibleMenuPanels.value.some((p) => p.id === id) && selectedId.value !== id) {
      ensureGroupExpandedForEntry(entry);
      selectedId.value = id;
    }
    return;
  }
  if (path === filterPath()) {
    focusFilterInput();
  }
}, { immediate: true });

watch(
  menuNavPaths,
  (paths) => {
    setNavMenuPaths(props.path, paths);
  },
  { immediate: true },
);

watch(manifest, (m) => {
  if (m?.panels.length && selectedId.value) {
    void loadPanel(selectedId.value);
  }
});

watch(
  panelRoot,
  (root) => {
    setNavExtension(`${props.path}/preview`, root);
  },
  { immediate: true },
);

onUnmounted(() => {
  setNavExtension(`${props.path}/preview`, null);
  setNavMenuPaths(props.path, []);
});

const selectedEntry = computed(() =>
  manifest.value?.panels.find((p) => p.id === selectedId.value),
);

const expandedGroups = ref(new Set<string>());

function isGroupExpanded(group: string): boolean {
  if (filter.value.trim()) return true;
  return expandedGroups.value.has(group);
}

function toggleGroup(group: string): void {
  if (filter.value.trim()) return;
  const next = new Set(expandedGroups.value);
  if (next.has(group)) next.delete(group);
  else next.add(group);
  expandedGroups.value = next;
}
</script>

<template>
  <div class="ini-panels-browser">
    <aside class="sidebar">
      <div class="sidebar-head">
        <h3 class="sidebar-title">INI панели</h3>
        <p v-if="manifest" class="sidebar-meta">
          {{ manifest.panelCount }} из {{ manifest.iniSource.split("/").pop() }}
        </p>
      </div>
      <input
        ref="filterInputRef"
        v-model="filter"
        type="search"
        class="filter"
        data-nav-node="1"
        :data-nav-path="filterPath()"
        v-bind="navPresentation(filterPath())"
        placeholder="Поиск панели…"
        autocomplete="off"
        @mousedown.stop="selectFilter()"
      />
      <p v-if="loadError" class="err">{{ loadError }}</p>
      <div v-else class="panel-list" role="listbox" aria-label="INI панели">
        <div v-for="[group, items] in groupedPanels" :key="group" class="group">
          <button
            type="button"
            class="group-title"
            :aria-expanded="isGroupExpanded(group)"
            @click="toggleGroup(group)"
          >
            <span class="group-chevron" :class="{ expanded: isGroupExpanded(group) }">›</span>
            <span class="group-title-text">{{ group }}</span>
            <span class="group-count">{{ items.length }}</span>
          </button>
          <div v-show="isGroupExpanded(group)" class="group-items">
            <button
              v-for="p in items"
              :key="p.id"
              type="button"
              class="panel-btn"
              data-nav-node="1"
              :data-nav-path="menuItemPath(p.id)"
              v-bind="navPresentation(menuItemPath(p.id))"
              role="option"
              :aria-selected="p.id === selectedId"
              @mousedown.prevent="selectPanel(p.id)"
            >
              <span class="panel-btn-title">{{ p.title }}</span>
              <span class="panel-btn-path">{{ p.menuPath }}</span>
            </button>
          </div>
        </div>
      </div>
    </aside>

    <main class="preview">
      <header v-if="selectedEntry" class="preview-head">
        <h2 class="preview-title">{{ selectedEntry.title }}</h2>
        <p class="preview-path">{{ selectedEntry.menuPath }}</p>
        <p class="preview-id">{{ selectedEntry.id }} · {{ selectedEntry.file }}</p>
      </header>

      <p v-if="panelLoading" class="state">Загрузка панели…</p>
      <p v-else-if="panelError" class="err">{{ panelError }}</p>
      <ComponentHost
        v-else-if="panelRoot"
        :instance="panelRoot"
        :path="`${path}/preview`"
        :selected-path="selectedPath"
        :active-path="activePath"
        :nav-mode="navMode"
        @select-path="selectComponent"
        @activate-path="(p) => { if (isNavActivatablePath(p)) { activateComponent(p); focusComponent(p); } }"
      />
      <p v-else class="state">Выберите панель слева</p>
    </main>
  </div>
</template>

<style scoped>
.ini-panels-browser {
  display: grid;
  grid-template-columns: minmax(14rem, 22rem) 1fr;
  gap: 1rem;
  width: 100%;
  min-height: 28rem;
  align-items: start;
}

.sidebar {
  position: sticky;
  top: 0;
  max-height: calc(100vh - 8rem);
  overflow: auto;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  background: var(--color-bg-elevated);
  padding: 0.75rem;
}

.sidebar-head {
  margin-bottom: 0.5rem;
}

.sidebar-title {
  margin: 0;
  font-size: 0.95rem;
  font-weight: 600;
}

.sidebar-meta {
  margin: 0.25rem 0 0;
  font-size: 0.72rem;
  color: var(--color-text-subtle);
}

.filter {
  width: 100%;
  margin-bottom: 0.65rem;
  padding: 0.4rem 0.55rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-muted);
}

.panel-list {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.group-title {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  width: 100%;
  margin: 0;
  padding: 0.35rem 0.25rem;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  font-size: 0.68rem;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-gray);
  font-weight: 600;
  cursor: pointer;
  text-align: left;
}

.group-title:hover {
  background: var(--color-bg-muted);
}

.group-chevron {
  display: inline-block;
  font-size: 0.85rem;
  line-height: 1;
  transition: transform 0.15s ease;
  transform: rotate(0deg);
}

.group-chevron.expanded {
  transform: rotate(90deg);
}

.group-title-text {
  flex: 1;
  min-width: 0;
}

.group-count {
  font-size: 0.62rem;
  font-weight: 500;
  color: var(--color-text-subtle);
  text-transform: none;
  letter-spacing: normal;
}

.group-items {
  padding-left: 0.5rem;
}

.panel-btn {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  width: 100%;
  padding: 0.4rem 0.5rem;
  margin-bottom: 0.15rem;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  text-align: left;
  cursor: pointer;
}

.panel-btn:hover {
  background: var(--color-bg-muted);
}

.panel-btn-title {
  font-size: 0.82rem;
  font-weight: 500;
  color: var(--color-text);
}

.panel-btn-path {
  font-size: 0.68rem;
  color: var(--color-text-subtle);
  line-height: 1.3;
}

.preview {
  min-width: 0;
}

.preview-head {
  margin-bottom: 1rem;
}

.preview-title {
  margin: 0;
  font-size: 1.15rem;
  font-weight: 600;
}

.preview-path {
  margin: 0.25rem 0 0;
  font-size: 0.82rem;
  color: var(--color-text-muted);
}

.preview-id {
  margin: 0.15rem 0 0;
  font-size: 0.72rem;
  color: var(--color-text-subtle);
  font-family: ui-monospace, monospace;
}

.state {
  margin: 0;
  color: var(--color-text-subtle);
}

.err {
  margin: 0;
  color: var(--color-error);
  font-size: 0.85rem;
}

@media (max-width: 900px) {
  .ini-panels-browser {
    grid-template-columns: 1fr;
  }

  .sidebar {
    position: static;
    max-height: 16rem;
  }
}
</style>
