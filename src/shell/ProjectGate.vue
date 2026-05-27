<script setup lang="ts">
import { computed, nextTick, onMounted, ref, useTemplateRef } from "vue";
import {
  useProject,
  type RecentProjectEntry,
} from "../composables/useProject";
import { useFooterSlot } from "../composables/useAppFooter";

const { createNewProject, openProject, openProjectAtPath, listRecentProjects } =
  useProject();

const busy = ref(false);
const error = ref<string | null>(null);
const recent = ref<RecentProjectEntry[]>([]);
const selectedIndex = ref(-1);
const gateRef = useTemplateRef<HTMLElement>("gateRef");

async function loadRecent(): Promise<void> {
  recent.value = await listRecentProjects();
  if (recent.value.length === 0) {
    selectedIndex.value = -1;
    return;
  }
  if (
    selectedIndex.value < 0 ||
    selectedIndex.value >= recent.value.length
  ) {
    selectedIndex.value = 0;
  }
}

async function run(action: () => Promise<boolean>): Promise<void> {
  error.value = null;
  busy.value = true;
  try {
    const ok = await action();
    if (!ok) return;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = false;
  }
}

function onCreate(): void {
  void run(createNewProject);
}

function onOpen(): void {
  void run(openProject);
}

async function openRecentAt(index: number): Promise<void> {
  const item = recent.value[index];
  if (!item || busy.value) return;
  if (!item.exists) {
    error.value = "Файл проекта не найден";
    return;
  }
  await run(() => openProjectAtPath(item.path));
}

function onRecentClick(index: number): void {
  selectedIndex.value = index;
  void openRecentAt(index);
}

function onKeydown(ev: KeyboardEvent): void {
  if (busy.value || recent.value.length === 0) return;
  if (ev.key === "ArrowDown") {
    ev.preventDefault();
    const next =
      selectedIndex.value < 0
        ? 0
        : Math.min(selectedIndex.value + 1, recent.value.length - 1);
    selectedIndex.value = next;
    return;
  }
  if (ev.key === "ArrowUp") {
    ev.preventDefault();
    const next =
      selectedIndex.value < 0
        ? recent.value.length - 1
        : Math.max(selectedIndex.value - 1, 0);
    selectedIndex.value = next;
    return;
  }
  if (ev.key === "Enter" && selectedIndex.value >= 0) {
    ev.preventDefault();
    void openRecentAt(selectedIndex.value);
  }
}

function dirname(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i > 0 ? path.slice(0, i) : "";
}

const gateFooter = computed(() => {
  if (error.value) return error.value;
  if (busy.value) return "Загрузка…";
  if (recent.value.length > 0) {
    return "↑↓ — выбор в списке, Enter — открыть";
  }
  return "Создайте или откройте проект";
});

useFooterSlot("gate:main", gateFooter, computed(() => ({
  error: !!error.value,
  priority: 40,
})));

onMounted(async () => {
  await loadRecent();
  await nextTick();
  gateRef.value?.focus();
});
</script>

<template>
  <Teleport to="body">
    <div
      ref="gateRef"
      class="project-gate"
      role="dialog"
      aria-modal="true"
      aria-labelledby="project-gate-title"
      tabindex="-1"
      @keydown="onKeydown"
    >
      <div class="project-gate-panel">
        <h2 id="project-gate-title" class="project-gate-title">Проект rusefui</h2>
        <p class="project-gate-lead">
          Работа с ECU, настройками и логами ведётся внутри проекта. Создайте новый файл
          проекта или откройте существующий (<code>.json</code>).
        </p>
        <div class="project-gate-actions">
          <button type="button" class="btn primary" :disabled="busy" @click="onCreate">
            Создать проект…
          </button>
          <button type="button" class="btn secondary" :disabled="busy" @click="onOpen">
            Открыть проект…
          </button>
        </div>
        <section
          v-if="recent.length > 0"
          class="project-gate-recent"
          aria-label="Недавние проекты"
        >
          <h3 class="project-gate-recent-title">Недавние</h3>
          <ul
            class="project-gate-recent-list"
            role="listbox"
            aria-label="Список недавних проектов"
          >
            <li
              v-for="(item, index) in recent"
              :key="item.path"
              role="option"
              :aria-selected="index === selectedIndex"
              class="project-gate-recent-item"
              :class="{
                selected: index === selectedIndex,
                missing: !item.exists,
              }"
              :title="item.path"
              @click="onRecentClick(index)"
            >
              <span class="project-gate-recent-label">{{ item.label }}</span>
              <span class="project-gate-recent-dir">{{ dirname(item.path) }}</span>
            </li>
          </ul>
        </section>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.project-gate {
  position: fixed;
  inset: 0;
  z-index: 11000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--color-bg);
  padding: 1.5rem;
  box-sizing: border-box;
  outline: none;
}

.project-gate-panel {
  width: min(32rem, 100%);
  max-height: min(90vh, 40rem);
  padding: 2rem 2.25rem;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.project-gate-title {
  margin: 0 0 0.75rem;
  font-size: 1.35rem;
  font-weight: 600;
  letter-spacing: -0.02em;
  color: var(--color-text);
}

.project-gate-lead {
  margin: 0 0 1.5rem;
  font-size: 0.92rem;
  line-height: 1.5;
  color: var(--color-text-muted);
}

.project-gate-lead code {
  font-size: 0.85em;
  padding: 0.1em 0.35em;
  border-radius: var(--radius-sm);
  background: var(--color-bg-muted);
}

.project-gate-actions {
  display: flex;
  flex-direction: column;
  gap: 0.65rem;
  flex-shrink: 0;
}

.project-gate-recent {
  margin-top: 1.5rem;
  padding-top: 1.25rem;
  border-top: 1px solid var(--color-border);
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.project-gate-recent-title {
  margin: 0 0 0.65rem;
  font-size: 0.8rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-text-muted);
}

.project-gate-recent-list {
  list-style: none;
  margin: 0;
  padding: 0;
  overflow-y: auto;
  flex: 1;
  min-height: 0;
}

.project-gate-recent-item {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  padding: 0.55rem 0.65rem;
  margin-bottom: 0.25rem;
  border-radius: var(--radius-md);
  border: 1px solid transparent;
  cursor: pointer;
  user-select: none;
}

.project-gate-recent-item:hover {
  background: var(--color-bg-muted);
}

.project-gate-recent-item.selected {
  background: var(--color-bg-muted);
  border-color: var(--color-border-strong);
}

.project-gate-recent-item.missing {
  opacity: 0.55;
  cursor: not-allowed;
}

.project-gate-recent-item.missing:hover {
  background: transparent;
}

.project-gate-recent-label {
  font-size: 0.95rem;
  font-weight: 500;
  color: var(--color-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.project-gate-recent-dir {
  font-size: 0.78rem;
  color: var(--color-text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: ui-monospace, monospace;
}

.btn {
  padding: 0.65rem 1rem;
  font-size: 0.95rem;
  font-weight: 600;
  border-radius: var(--radius-md);
  border: 1px solid transparent;
  cursor: pointer;
}

.btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.btn.primary {
  color: var(--color-on-accent);
  background: var(--color-accent);
  border-color: var(--color-accent);
}

.btn.primary:hover:not(:disabled) {
  filter: brightness(1.05);
}

.btn.secondary {
  color: var(--color-text);
  background: var(--color-bg-muted);
  border-color: var(--color-border);
}

.btn.secondary:hover:not(:disabled) {
  border-color: var(--color-border-strong);
}
</style>
