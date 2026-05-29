<script setup lang="ts">
import { computed, onUnmounted, ref, shallowRef, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import ComponentHost from "../ComponentHost.vue";
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
import {
  configCanView,
  initConfig,
  useConfig,
  type ChecklistSnapshot,
} from "../../composables/useConfig";
import { initChecklist } from "../../composables/useChecklist";
import { resolveChecklistEditor } from "../../composables/useChecklistEditor";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

void initConfig();
void initChecklist();

const { snapshot } = useConfig();

const checklist = computed(() => snapshot.value.checklist);
const canShow = computed(() => configCanView(snapshot.value));

const selectedId = ref<string>("");
const showOnlyIncomplete = ref(false);
const editorInstance = shallowRef<ComponentInstance | null>(null);
const editorLoading = ref(false);
const editorError = ref<string | null>(null);

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
      const firstFail = items.find((i) => !i.ok);
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

watch(
  selectedId,
  (id) => {
    const item = flatItems.value.find((i) => i.id === id);
    if (!item) {
      editorInstance.value = null;
      return;
    }
    editorLoading.value = true;
    editorError.value = null;
    void resolveChecklistEditor({ ...item.editor })
      .then((inst) => {
        editorInstance.value = inst;
      })
      .catch((e) => {
        editorInstance.value = null;
        editorError.value = e instanceof Error ? e.message : String(e);
      })
      .finally(() => {
        editorLoading.value = false;
      });
  },
  { immediate: true },
);

watch(
  menuNavPaths,
  (paths) => {
    setNavMenuPaths(props.path, paths);
  },
  { immediate: true },
);

watch(
  editorInstance,
  (inst) => {
    setNavExtension(`${props.path}/editor`, inst);
  },
  { immediate: true },
);

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
        Минимальная готовность к запуску. Правила — <code>config/checklist.yaml</code>.
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
          <header class="level-head">
            <h3 class="level-title">{{ level.title }}</h3>
            <span class="level-count">{{ levelSummary(level.id).done }}/{{ levelSummary(level.id).total }}</span>
          </header>
          <p v-if="level.description" class="level-desc">{{ level.description }}</p>

          <div v-for="group in groupsForLevel(level.id)" :key="group.id" class="group-block">
            <h4 class="group-title">{{ group.title }}</h4>
            <ul class="checklist">
              <li v-for="item in group.items" :key="item.id">
                <button
                  type="button"
                  class="check-row"
                  :class="{
                    'check-row--ok': item.ok,
                    'check-row--fail': !item.ok,
                  }"
                  data-nav-node="1"
                  :data-nav-path="menuItemPath(item.id)"
                  v-bind="navPresentation(menuItemPath(item.id))"
                  @mousedown.prevent="selectItem(item.id)"
                >
                  <span class="check-icon" aria-hidden="true">{{ item.ok ? "✓" : "✗" }}</span>
                  <span class="check-label">{{ item.label }}</span>
                  <span class="check-value">{{ item.valueDisplay }}</span>
                </button>
              </li>
            </ul>
          </div>
        </article>
      </aside>

      <main class="checklist-editor">
        <p v-if="editorLoading" class="editor-state">Загрузка редактора…</p>
        <p v-else-if="editorError" class="editor-err">{{ editorError }}</p>
        <p v-else-if="!editorInstance" class="editor-state">Выберите пункт checklist</p>
        <ComponentHost
          v-else
          :instance="editorInstance"
          :path="`${path}/editor`"
          :selected-path="selectedPath"
          :active-path="activePath"
          :nav-mode="navMode"
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
  margin-top: 1rem;
  padding-top: 0.75rem;
  border-top: 1px solid var(--color-border, rgba(255, 255, 255, 0.06));
}

.level-head {
  display: flex;
  justify-content: space-between;
  gap: 0.5rem;
  align-items: baseline;
}

.level-title {
  margin: 0;
  font-size: 0.95rem;
  font-weight: 600;
}

.level-count {
  font-size: 0.78rem;
  color: var(--color-text-muted);
  font-variant-numeric: tabular-nums;
}

.level-desc {
  margin: 0.25rem 0 0.5rem;
  font-size: 0.78rem;
  color: var(--color-text-muted);
  line-height: 1.4;
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
