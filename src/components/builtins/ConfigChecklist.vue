<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, shallowRef, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import ComponentHost from "../ComponentHost.vue";
import {
  activateComponent,
  focusComponent,
  isNavActivatablePath,
  selectedPath,
  selectComponent,
  setNavExtension,
  setNavMenuPaths,
  syncNavSelectionVisual,
} from "../../composables/useWorkspaceNav";
import {
  configCanView,
  initConfig,
  useConfig,
  type ChecklistEditor,
  type ChecklistLevelStatus,
  type ChecklistSnapshot,
} from "../../composables/useConfig";
import { initChecklist } from "../../composables/useChecklist";
import { resolveChecklistEditors } from "../../composables/useChecklistEditor";
import { useTabActivity } from "../../composables/useTabActivity";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const { isActive: tabActive } = useTabActivity();

void initConfig();
void initChecklist();

const { snapshot } = useConfig();

const checklist = computed(() => snapshot.value.checklist);
const canShow = computed(() => configCanView(snapshot.value));

const selectedId = ref<string>("");
const showOnlyIncomplete = ref(false);
const collapsedLevels = ref<Set<string>>(new Set());
const editorInstances = shallowRef<ComponentInstance[]>([]);
const editorLoading = ref(false);
const editorError = ref<string | null>(null);

/** Корень редактора справа: один leaf или composite — пути nav как у IniPanelsBrowser. */
const editorRoot = computed((): ComponentInstance | null => {
  const insts = editorInstances.value;
  if (insts.length === 0) return null;
  if (insts.length === 1) return insts[0]!;
  return {
    id: "checklist-conflict-editors",
    type: "composite",
    children: insts,
  };
});

type ChecklistItemView = Readonly<ChecklistSnapshot["items"][number]>;

interface GroupBlock {
  id: string;
  title: string;
  order: number;
  items: ChecklistItemView[];
}

function groupsForLevel(levelId: string): GroupBlock[] {
  let items = checklist.value?.items.filter((i) => i.level === levelId) ?? [];
  if (showOnlyIncomplete.value) {
    items = items.filter((i) => !i.ok);
  }
  const map = new Map<string, GroupBlock>();
  for (const item of items) {
    let block = map.get(item.group);
    if (!block) {
      block = {
        id: item.group,
        title: item.groupTitle,
        order: item.groupOrder,
        items: [],
      };
      map.set(item.group, block);
    }
    block.items.push(item as ChecklistItemView);
  }
  return [...map.values()].sort((a, b) => a.order - b.order || a.title.localeCompare(b.title));
}

const levels = computed(() => checklist.value?.levels ?? []);

const visibleLevels = computed(() =>
  levels.value.filter((level) => groupsForLevel(level.id).length > 0),
);

const flatItems = computed(() => checklist.value?.items ?? []);

const visibleMenuItems = computed((): ChecklistItemView[] => {
  const result: ChecklistItemView[] = [];
  for (const level of visibleLevels.value) {
    for (const group of groupsForLevel(level.id)) {
      result.push(...group.items);
    }
  }
  return result;
});

function menuItemPath(id: string): string {
  return `${props.path}/menu/${id}`;
}

/** Редакторы справа — только из текущего пункта checklist. */
function editorTargets(item: {
  editor?: ChecklistEditor;
  editors?: readonly ChecklistEditor[];
}): ChecklistEditor[] {
  if (item.editors?.length) return [...item.editors];
  if (item.editor) return [item.editor];
  return [];
}

const menuNavPaths = computed(() =>
  visibleMenuItems.value.map((item) => menuItemPath(item.id)),
);

watch(
  flatItems,
  (items) => {
    if (!items.length) {
      selectedId.value = "";
      return;
    }
    if (!items.some((i) => i.id === selectedId.value)) {
      const firstConflictFail = items.find((i) => i.level === "conflicts" && !i.ok);
      const firstFail = firstConflictFail ?? items.find((i) => !i.ok);
      selectedId.value = (firstFail ?? items[0])!.id;
    }
  },
  { immediate: true },
);

watch(showOnlyIncomplete, (on) => {
  if (!on) return;
  const current = flatItems.value.find((i) => i.id === selectedId.value);
  if (current?.ok) {
    const firstFail = flatItems.value.find((i) => !i.ok);
    if (firstFail) selectedId.value = firstFail.id;
  }
});

async function loadEditorForSelection(id: string): Promise<void> {
  const item = flatItems.value.find((i) => i.id === id);
  if (item) expandLevel(item.level);
  if (!item) {
    editorInstances.value = [];
    return;
  }
  editorLoading.value = true;
  editorError.value = null;
  try {
    editorInstances.value = await resolveChecklistEditors(editorTargets(item));
  } catch (e) {
    editorInstances.value = [];
    editorError.value = e instanceof Error ? e.message : String(e);
  } finally {
    editorLoading.value = false;
  }
}

watch(
  selectedId,
  (id) => {
    void loadEditorForSelection(id);
  },
  { immediate: true },
);

watch(
  menuNavPaths,
  (paths) => {
    if (!tabActive.value) return;
    setNavMenuPaths(props.path, paths);
    void nextTick(() => syncNavSelectionVisual(selectedPath.value));
  },
  { immediate: true },
);

watch(
  editorRoot,
  (root) => {
    if (!tabActive.value) return;
    setNavExtension(`${props.path}/editor`, root);
  },
  { immediate: true },
);

watch(tabActive, (active) => {
  if (!active) return;
  setNavMenuPaths(props.path, menuNavPaths.value);
  if (editorRoot.value) {
    setNavExtension(`${props.path}/editor`, editorRoot.value);
  } else if (selectedId.value) {
    void loadEditorForSelection(selectedId.value);
  }
});

watch(selectedPath, (path) => {
  const prefix = `${props.path}/menu/`;
  if (!path.startsWith(prefix)) return;
  const id = path.slice(prefix.length);
  if (visibleMenuItems.value.some((i) => i.id === id) && selectedId.value !== id) {
    selectedId.value = id;
  }
});

onUnmounted(() => {
  setNavExtension(`${props.path}/editor`, null);
  setNavMenuPaths(props.path, []);
});

function selectItem(id: string): void {
  selectedId.value = id;
  selectComponent(menuItemPath(id));
}

function levelSummary(levelId: string) {
  const list = checklist.value?.items.filter((i) => i.level === levelId) ?? [];
  const done = list.filter((i) => i.ok).length;
  return { done, total: list.length };
}

function isLevelCollapsed(levelId: string): boolean {
  return collapsedLevels.value.has(levelId);
}

function setLevelCollapsed(levelId: string, collapsed: boolean): void {
  const next = new Set(collapsedLevels.value);
  if (collapsed) next.add(levelId);
  else next.delete(levelId);
  collapsedLevels.value = next;
}

function toggleLevel(levelId: string): void {
  setLevelCollapsed(levelId, !isLevelCollapsed(levelId));
}

function expandLevel(levelId: string): void {
  if (isLevelCollapsed(levelId)) setLevelCollapsed(levelId, false);
}

function levelLedClass(level: ChecklistLevelStatus): string {
  if (level.ok) return "level-led--ok";
  switch (level.severity) {
    case "critical":
      return "level-led--critical";
    case "warning":
      return "level-led--warning";
    default:
      return "level-led--error";
  }
}
</script>

<template>
  <section class="checklist-shell">
    <header class="checklist-head">
      <div class="checklist-head-row">
        <h2 class="checklist-title">CHKLST</h2>
        <label v-if="canShow && checklist?.evaluated" class="checklist-filter">
          <input v-model="showOnlyIncomplete" type="checkbox" />
          <span>Только невыполненные</span>
        </label>
      </div>
      <p class="checklist-sub">
        Готовность к запуску и конфликты настроек. Правила —
        <code>config/checklist.yaml</code>.
      </p>
    </header>

    <p v-if="!canShow" class="checklist-empty">Загрузите конфигурацию ECU или проекта.</p>
    <p v-else-if="checklist && !checklist.rulesLoaded" class="checklist-empty">
      Правила checklist не загружены.
    </p>
    <p v-else-if="checklist && !checklist.evaluated" class="checklist-empty">
      Ожидание снимка конфигурации…
    </p>

    <div v-else class="checklist-split">
      <aside class="checklist-sidebar">
        <p v-if="showOnlyIncomplete && visibleLevels.length === 0" class="checklist-filter-empty">
          Все пункты выполнены.
        </p>
        <article v-for="level in visibleLevels" :key="level.id" class="level-block">
          <button
            type="button"
            class="level-head"
            :aria-expanded="!isLevelCollapsed(level.id)"
            @click="toggleLevel(level.id)"
          >
            <span
              class="level-chevron"
              :class="{ expanded: !isLevelCollapsed(level.id) }"
              aria-hidden="true"
            >›</span>
            <span
              class="level-led"
              :class="levelLedClass(level)"
              :title="level.severity"
              aria-hidden="true"
            />
            <h3 class="level-title">{{ level.title }}</h3>
            <span class="level-count">
              <template v-if="!level.ok && level.issueCount">
                {{ level.issueCount }} ·
              </template>
              {{ levelSummary(level.id).done }}/{{ levelSummary(level.id).total }}
            </span>
          </button>
          <p v-if="level.description && !isLevelCollapsed(level.id)" class="level-desc">
            {{ level.description }}
          </p>

          <div v-show="!isLevelCollapsed(level.id)" class="level-body">
            <div v-for="group in groupsForLevel(level.id)" :key="group.id" class="group-block">
              <h4 class="group-title">{{ group.title }}</h4>
              <ul class="checklist">
                <li v-for="item in group.items" :key="item.id">
                  <button
                    type="button"
                    class="check-row nav-node"
                    :class="{
                      'check-row--ok': item.ok,
                      'check-row--fail': !item.ok,
                    }"
                    data-nav-node="1"
                    :data-nav-path="menuItemPath(item.id)"
                    @mousedown.prevent="selectItem(item.id)"
                  >
                    <span class="check-icon" aria-hidden="true">{{ item.ok ? "✓" : "✗" }}</span>
                    <span class="check-label">{{ item.label }}</span>
                    <span class="check-value">{{ item.valueDisplay }}</span>
                  </button>
                </li>
              </ul>
            </div>
          </div>
        </article>
      </aside>

      <main class="checklist-editor">
        <p v-if="editorLoading" class="editor-state">Загрузка редактора…</p>
        <p v-else-if="editorError" class="editor-err">{{ editorError }}</p>
        <p v-else-if="!editorRoot" class="editor-state">Выберите пункт checklist</p>
        <ComponentHost
          v-else
          :instance="editorRoot"
          :path="`${path}/editor`"
          @select-path="selectComponent"
          @activate-path="(p) => { if (isNavActivatablePath(p)) { activateComponent(p); focusComponent(p); } }"
        />
      </main>
    </div>
  </section>
</template>

<style scoped>
.checklist-shell {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  height: 100%;
  min-height: 28rem;
}

.checklist-head {
  flex-shrink: 0;
}

.checklist-head-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  flex-wrap: wrap;
}

.checklist-filter {
  display: inline-flex;
  align-items: center;
  gap: 0.4rem;
  font-size: 0.82rem;
  color: var(--color-text-muted);
  cursor: pointer;
  user-select: none;
}

.checklist-filter input {
  margin: 0;
  accent-color: var(--color-accent);
}

.checklist-filter-empty {
  margin: 0;
  padding: 0.5rem 0.25rem;
  font-size: 0.85rem;
  color: var(--color-text-muted);
}

.checklist-title {
  margin: 0;
  font-size: 1.1rem;
  font-weight: 600;
}

.checklist-sub {
  margin: 0.25rem 0 0;
  font-size: 0.85rem;
  color: var(--color-text-muted);
}

.checklist-empty {
  margin: 0;
  padding: 0.75rem 1rem;
  border-radius: var(--radius-md, 6px);
  color: var(--color-text-muted);
  background: var(--color-surface-2, rgba(255, 255, 255, 0.04));
}

.checklist-split {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(16rem, 26rem) 1fr;
  gap: 1rem;
  align-items: stretch;
}

.checklist-sidebar {
  overflow: auto;
  border: 1px solid var(--color-border, rgba(255, 255, 255, 0.08));
  border-radius: var(--radius-md, 6px);
  padding: 0.65rem;
  background: var(--color-surface-1, rgba(255, 255, 255, 0.02));
}

.checklist-editor {
  overflow: auto;
  border: 1px solid var(--color-border, rgba(255, 255, 255, 0.08));
  border-radius: var(--radius-md, 6px);
  padding: 0.75rem 1rem;
  background: var(--color-surface-1, rgba(255, 255, 255, 0.02));
  min-width: 0;
}

.level-block + .level-block {
  margin-top: 0.65rem;
  padding-top: 0.65rem;
  border-top: 1px solid var(--color-border, rgba(255, 255, 255, 0.06));
}

.level-head {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  width: 100%;
  margin: 0;
  padding: 0.3rem 0.35rem;
  border: none;
  border-radius: var(--radius-sm, 4px);
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.level-head:hover {
  background: var(--color-surface-2, rgba(255, 255, 255, 0.04));
}

.level-chevron {
  flex-shrink: 0;
  display: inline-block;
  width: 0.65rem;
  font-size: 0.85rem;
  line-height: 1;
  color: var(--color-text-muted);
  transform: rotate(0deg);
  transition: transform 0.15s ease;
}

.level-chevron.expanded {
  transform: rotate(90deg);
}

.level-led {
  flex-shrink: 0;
  width: 0.45rem;
  height: 0.45rem;
  border-radius: 50%;
  background: currentColor;
  box-shadow: 0 0 5px currentColor;
}

.level-led--ok {
  color: var(--color-success, #6ecf8a);
  opacity: 0.75;
  box-shadow: none;
}

.level-led--error {
  color: var(--color-danger, #f08080);
}

.level-led--critical {
  color: #ff5c5c;
  animation: checklist-led-pulse 1.8s ease-in-out infinite;
}

.level-led--warning {
  color: #e6a817;
  box-shadow: 0 0 4px currentColor;
}

@keyframes checklist-led-pulse {
  0%,
  100% {
    opacity: 1;
    box-shadow: 0 0 6px currentColor;
  }
  50% {
    opacity: 0.45;
    box-shadow: 0 0 2px currentColor;
  }
}

.level-title {
  margin: 0;
  flex: 1;
  min-width: 0;
  font-size: 0.9rem;
  font-weight: 600;
}

.level-count {
  flex-shrink: 0;
  font-size: 0.75rem;
  color: var(--color-text-muted);
  font-variant-numeric: tabular-nums;
}

.level-desc {
  margin: 0.15rem 0 0.45rem 1.5rem;
  font-size: 0.76rem;
  color: var(--color-text-muted);
  line-height: 1.4;
}

.level-body {
  margin-top: 0.35rem;
  padding-left: 0.15rem;
}

.group-block + .group-block {
  margin-top: 0.65rem;
}

.group-title {
  margin: 0 0 0.35rem;
  font-size: 0.78rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-text-muted);
}

.checklist {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.check-row {
  width: 100%;
  display: grid;
  grid-template-columns: 1.1rem 1fr auto;
  gap: 0.45rem;
  align-items: center;
  padding: 0.35rem 0.45rem;
  border: 1px solid transparent;
  border-radius: var(--radius-sm, 4px);
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.check-row--ok .check-icon {
  color: var(--color-success, #6ecf8a);
}

.check-row--fail .check-icon {
  color: var(--color-danger, #f08080);
}

.check-label {
  font-size: 0.85rem;
  font-weight: 500;
  min-width: 0;
}

.check-value {
  font-size: 0.78rem;
  color: var(--color-text-muted);
  font-variant-numeric: tabular-nums;
  max-width: 9rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.editor-state,
.editor-err {
  margin: 0;
  font-size: 0.9rem;
  color: var(--color-text-muted);
}

.editor-err {
  color: var(--color-danger, #f08080);
}
</style>
