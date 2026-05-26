<script setup lang="ts">
import { computed, ref } from "vue";
import { useProject } from "../composables/useProject";
import { useFooterSlot } from "../composables/useAppFooter";

const { createNewProject, openProject } = useProject();

const busy = ref(false);
const error = ref<string | null>(null);

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

const gateFooter = computed(() => {
  if (error.value) return error.value;
  if (busy.value) return "Ожидание диалога файла…";
  return "Создайте или откройте проект";
});
useFooterSlot("gate:main", gateFooter, computed(() => ({
  error: !!error.value,
  priority: 40,
})));
</script>

<template>
  <Teleport to="body">
    <div
      class="project-gate"
      role="dialog"
      aria-modal="true"
      aria-labelledby="project-gate-title"
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
}

.project-gate-panel {
  width: min(26rem, 100%);
  padding: 2rem 2.25rem;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card);
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
