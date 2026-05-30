<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";

defineProps<{
  projectName: string;
  projectPath: string | null;
  projectDirty: boolean;
  projectBusy: boolean;
  canCaptureConfig: boolean;
  hasOpenProject: boolean;
  timelineClipCount: number;
  iniSignature: string | null;
}>();

const emit = defineEmits<{
  newProject: [];
  openProject: [];
  closeProject: [];
  saveProject: [];
  saveProjectAs: [];
  changeIni: [];
  captureConfig: [];
  copyProjectWithoutTimeline: [];
  clearTimeline: [];
}>();

const open = ref(false);

function toggle() {
  open.value = !open.value;
}

function action(fn: () => void) {
  open.value = false;
  fn();
}

function onOutsideClick(e: MouseEvent) {
  const el = document.getElementById("project-menu-root");
  if (el && !el.contains(e.target as Node)) open.value = false;
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") open.value = false;
}

onMounted(() => {
  window.addEventListener("mousedown", onOutsideClick);
  window.addEventListener("keydown", onKeydown);
});
onUnmounted(() => {
  window.removeEventListener("mousedown", onOutsideClick);
  window.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <div id="project-menu-root" class="project-menu">
    <button
      type="button"
      class="project-menu-btn"
      :class="{ open }"
      :title="`Проект: ${projectName}${projectDirty ? ' (несохранён)' : ''}`"
      :aria-expanded="open"
      aria-haspopup="menu"
      @click="toggle"
    >
      <!-- folder icon -->
      <svg
        class="project-icon"
        viewBox="0 0 20 16"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        aria-hidden="true"
      >
        <path
          d="M1 3.5C1 2.67 1.67 2 2.5 2H7.38l1.5 1.5H17.5C18.33 3.5 19 4.17 19 5v8c0 .83-.67 1.5-1.5 1.5h-15C1.67 14.5 1 13.83 1 13V3.5Z"
          fill="currentColor"
          opacity="0.18"
        />
        <path
          d="M1 6C1 5.17 1.67 4.5 2.5 4.5H17.5C18.33 4.5 19 5.17 19 6v7c0 .83-.67 1.5-1.5 1.5h-15C1.67 14.5 1 13.83 1 13V6Z"
          fill="currentColor"
        />
        <path
          d="M1 3.5C1 2.67 1.67 2 2.5 2H7.38l2 2H1V3.5Z"
          fill="currentColor"
          opacity="0.65"
        />
        <circle
          v-if="projectDirty"
          cx="17"
          cy="3"
          r="2.5"
          fill="#f59e0b"
          class="dirty-dot"
        />
      </svg>
    </button>

    <div v-if="open" class="project-dropdown" role="menu">
      <div class="project-dropdown-header">
        <span class="project-dropdown-name" :title="projectPath ?? '(файл не сохранён)'">
          {{ projectName }}
        </span>
        <span v-if="projectDirty" class="project-dropdown-unsaved">несохранён</span>
        <span v-if="iniSignature" class="project-dropdown-ini" :title="iniSignature">
          INI: {{ iniSignature.split(".").pop() ?? iniSignature }}
        </span>
      </div>

      <div class="project-dropdown-sep" />

      <button
        type="button"
        role="menuitem"
        class="menu-item"
        :disabled="projectBusy"
        @click="action(() => emit('newProject'))"
      >
        <span class="menu-item-icon">
          <svg viewBox="0 0 16 16" fill="none"><rect x="3" y="2" width="10" height="12" rx="1.5" fill="currentColor" opacity=".18"/><rect x="3" y="2" width="10" height="12" rx="1.5" stroke="currentColor" stroke-width="1.2"/><path d="M8 5.5v5M5.5 8h5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>
        </span>
        Новый проект…
      </button>

      <button
        type="button"
        role="menuitem"
        class="menu-item"
        :disabled="projectBusy"
        @click="action(() => emit('openProject'))"
      >
        <span class="menu-item-icon">
          <svg viewBox="0 0 16 16" fill="none"><path d="M2 5.5C2 4.67 2.67 4 3.5 4H6.88l1.25 1.25H12.5C13.33 5.25 14 5.92 14 6.75v5.25C14 12.83 13.33 13.5 12.5 13.5h-9C2.67 13.5 2 12.83 2 12V5.5Z" fill="currentColor" opacity=".18"/><path d="M2 7C2 6.17 2.67 5.5 3.5 5.5H12.5c.83 0 1.5.67 1.5 1.5V12c0 .83-.67 1.5-1.5 1.5h-9C2.67 13.5 2 12.83 2 12V7Z" fill="currentColor"/><path d="M2 5.5C2 4.67 2.67 4 3.5 4H6.88l1.25 1.5H2V5.5Z" fill="currentColor" opacity=".65"/></svg>
        </span>
        Открыть проект…
      </button>

      <button
        type="button"
        role="menuitem"
        class="menu-item"
        :disabled="projectBusy"
        @click="action(() => emit('closeProject'))"
      >
        <span class="menu-item-icon">
          <svg viewBox="0 0 16 16" fill="none"><path d="M4 4l8 8M12 4L4 12" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>
        </span>
        Закрыть проект
      </button>

      <div class="project-dropdown-sep" />

      <button
        type="button"
        role="menuitem"
        class="menu-item"
        :disabled="projectBusy"
        @click="action(() => emit('saveProject'))"
      >
        <span class="menu-item-icon">
          <svg viewBox="0 0 16 16" fill="none"><rect x="2" y="2" width="12" height="12" rx="1.5" fill="currentColor" opacity=".15"/><rect x="2" y="2" width="12" height="12" rx="1.5" stroke="currentColor" stroke-width="1.2"/><rect x="5" y="2" width="6" height="4" rx=".5" fill="currentColor" opacity=".5"/><rect x="4" y="9" width="8" height="4" rx=".75" fill="currentColor" opacity=".4"/></svg>
        </span>
        Сохранить
        <span class="menu-item-shortcut">Ctrl+S</span>
      </button>

      <button
        type="button"
        role="menuitem"
        class="menu-item"
        :disabled="projectBusy"
        @click="action(() => emit('saveProjectAs'))"
      >
        <span class="menu-item-icon">
          <svg viewBox="0 0 16 16" fill="none"><rect x="2" y="2" width="12" height="12" rx="1.5" fill="currentColor" opacity=".15"/><rect x="2" y="2" width="12" height="12" rx="1.5" stroke="currentColor" stroke-width="1.2"/><rect x="5" y="2" width="6" height="4" rx=".5" fill="currentColor" opacity=".5"/><rect x="4" y="9" width="8" height="4" rx=".75" fill="currentColor" opacity=".4"/><path d="M11 11l2 2M11 13l2-2" stroke="white" stroke-width="1.1" stroke-linecap="round"/></svg>
        </span>
        Сохранить как…
      </button>

      <button
        type="button"
        role="menuitem"
        class="menu-item"
        :disabled="projectBusy || !hasOpenProject"
        @click="action(() => emit('changeIni'))"
      >
        <span class="menu-item-icon">
          <svg viewBox="0 0 16 16" fill="none"><path d="M3 4.5h10M3 8h10M3 11.5h6" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/><path d="M12.5 10.5l1.5 1.5-1.5 1.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>
        </span>
        Сменить INI…
      </button>

      <div class="project-dropdown-sep" />

      <button
        type="button"
        role="menuitem"
        class="menu-item"
        :disabled="projectBusy || !canCaptureConfig"
        :title="canCaptureConfig ? '' : 'ECU не подключена или config не загружен'"
        @click="action(() => emit('captureConfig'))"
      >
        <span class="menu-item-icon">
          <svg viewBox="0 0 16 16" fill="none"><path d="M8 2L8 10M8 10L5 7M8 10L11 7" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/><path d="M3 12h10" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>
        </span>
        Config → проект
      </button>

      <div class="project-dropdown-sep" />

      <button
        type="button"
        role="menuitem"
        class="menu-item"
        :disabled="projectBusy || !hasOpenProject"
        @click="action(() => emit('copyProjectWithoutTimeline'))"
      >
        <span class="menu-item-icon">
          <svg viewBox="0 0 16 16" fill="none"><rect x="3" y="2" width="9" height="11" rx="1.2" fill="currentColor" opacity=".15"/><rect x="3" y="2" width="9" height="11" rx="1.2" stroke="currentColor" stroke-width="1.1"/><rect x="6" y="2" width="9" height="11" rx="1.2" fill="currentColor" opacity=".35"/><rect x="6" y="2" width="9" height="11" rx="1.2" stroke="currentColor" stroke-width="1.1"/></svg>
        </span>
        Копировать проект…
        <span class="menu-item-hint">без таймлайна</span>
      </button>

      <button
        type="button"
        role="menuitem"
        class="menu-item"
        :disabled="projectBusy || !hasOpenProject || timelineClipCount === 0"
        @click="action(() => emit('clearTimeline'))"
      >
        <span class="menu-item-icon">
          <svg viewBox="0 0 16 16" fill="none"><path d="M3 4h10M5.5 4V3h5v1M6 7v4M10 7v4M4.5 4l.5 9h6l.5-9" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>
        </span>
        Очистить таймлайн
      </button>
    </div>
  </div>
</template>

<style scoped>
.project-menu {
  position: relative;
  flex-shrink: 0;
}

/* ---- trigger button ---- */
.project-menu-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 3.2rem;
  height: 3.2rem;
  background: none;
  border: 1px solid transparent;
  border-radius: var(--radius-md);
  cursor: pointer;
  color: var(--color-accent);
  transition: background 0.15s, border-color 0.15s, color 0.15s;
  padding: 0;
}

.project-menu-btn:hover,
.project-menu-btn.open {
  background: var(--color-bg-accent-soft);
  border-color: var(--color-success-border);
  color: var(--color-accent-hover);
}

.project-icon {
  width: 1.75rem;
  height: 1.75rem;
}

.dirty-dot {
  animation: blink-dot 2s ease-in-out infinite;
}

@keyframes blink-dot {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.35; }
}

/* ---- dropdown ---- */
.project-dropdown {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  z-index: 8000;
  min-width: 16rem;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card), 0 8px 24px rgba(58,53,48,0.12);
  padding: 0.4rem 0;
  overflow: hidden;
}

.project-dropdown-header {
  display: flex;
  align-items: baseline;
  gap: 0.5rem;
  padding: 0.45rem 0.85rem 0.35rem;
}

.project-dropdown-name {
  font-size: 0.82rem;
  font-weight: 600;
  color: var(--color-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 12rem;
}

.project-dropdown-unsaved {
  font-size: 0.68rem;
  color: #b45309;
  background: #fef3c7;
  padding: 0.1rem 0.35rem;
  border-radius: var(--radius-sm);
  flex-shrink: 0;
}

.project-dropdown-ini {
  display: block;
  width: 100%;
  margin-top: 0.25rem;
  font-size: 0.68rem;
  color: var(--color-text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.project-dropdown-sep {
  height: 1px;
  background: var(--color-border);
  margin: 0.3rem 0;
}

/* ---- menu items ---- */
.menu-item {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  width: 100%;
  padding: 0.42rem 0.85rem;
  background: none;
  border: none;
  text-align: left;
  font-size: 0.84rem;
  color: var(--color-text);
  cursor: pointer;
  border-radius: 0;
  transition: background 0.1s;
}

.menu-item:hover:not(:disabled) {
  background: var(--color-bg-muted);
}

.menu-item:disabled {
  opacity: 0.42;
  cursor: not-allowed;
}

.menu-item-icon {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  width: 1rem;
  height: 1rem;
  color: var(--color-accent);
}

.menu-item-icon svg {
  width: 100%;
  height: 100%;
}

.menu-item-shortcut {
  margin-left: auto;
  font-size: 0.68rem;
  color: var(--color-text-subtle);
}

.menu-item-hint {
  margin-left: auto;
  font-size: 0.65rem;
  color: var(--color-text-subtle);
  font-weight: 400;
}
</style>
