<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
import ConfigDiffFieldRow from "../components/builtins/ConfigDiffFieldRow.vue";
import {
  initConfigDiff,
  useConfigDiff,
  type DiffSide,
} from "../composables/useConfigDiff";

void initConfigDiff();

const { active, count, snapshot, choiceFor, setChoice, setAll, apply } = useConfigDiff();

const busy = ref(false);
const error = ref<string | null>(null);

const entries = computed(() => snapshot.value.entries);

async function run(fn: () => Promise<void>): Promise<void> {
  busy.value = true;
  error.value = null;
  try {
    await fn();
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = false;
  }
}

function onPick(field: string, side: DiffSide): void {
  void run(async () => {
    await setChoice(field, side);
  });
}

function onAllProject(): void {
  void run(async () => {
    await setAll("project");
  });
}

function onAllEcu(): void {
  void run(async () => {
    await setAll("ecu");
  });
}

function onApply(): void {
  void run(() => apply());
}

watch(
  active,
  (on) => {
    document.body.classList.toggle("config-merge-blocking", on);
    if (on) {
      document.body.style.overflow = "hidden";
    } else {
      document.body.style.overflow = "";
    }
  },
  { immediate: true },
);

onUnmounted(() => {
  document.body.classList.remove("config-merge-blocking");
  document.body.style.overflow = "";
});
</script>

<template>
  <Teleport to="body">
    <div
      v-if="active"
      class="config-diff-modal-root"
      role="presentation"
    >
      <div class="config-diff-backdrop" aria-hidden="true" />
      <div
        class="config-diff-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="config-diff-title"
      >
        <header class="dialog-header">
          <div>
            <h2 id="config-diff-title">Слияние config: проект и ECU</h2>
            <p class="dialog-sub">
              Обнаружено <strong>{{ count }}</strong> отличий. Для каждого поля выберите
              значение слева (файл проекта) или справа (ECU), затем нажмите «Применить».
            </p>
          </div>
        </header>

        <div class="dialog-toolbar">
          <button type="button" class="btn" :disabled="busy" @click="onAllProject">
            Все значения → ECU
          </button>
          <button type="button" class="btn" :disabled="busy" @click="onAllEcu">
            Все значения → проект
          </button>
        </div>

        <div class="dialog-body">
          <ConfigDiffFieldRow
            v-for="e in entries"
            :key="e.field"
            :entry="e"
            :choice="choiceFor(e.field)"
            @pick="onPick(e.field, $event)"
          />
        </div>

        <footer class="dialog-footer">
          <p v-if="error" class="error" role="alert">{{ error }}</p>
          <div class="footer-actions">
            <button
              type="button"
              class="btn primary"
              :disabled="busy || count === 0"
              @click="onApply"
            >
              {{ busy ? "Применение…" : "Применить" }}
            </button>
          </div>
          <p class="footer-hint">Остальной интерфейс недоступен, пока не выполнено слияние.</p>
        </footer>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.config-diff-modal-root {
  position: fixed;
  inset: 0;
  z-index: 10000;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 1.25rem;
  pointer-events: auto;
}

.config-diff-backdrop {
  position: absolute;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  backdrop-filter: blur(2px);
}

.config-diff-dialog {
  position: relative;
  z-index: 1;
  display: flex;
  flex-direction: column;
  width: min(720px, 100%);
  max-height: min(88vh, 900px);
  border-radius: var(--radius-lg, 12px);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg);
  box-shadow: 0 24px 64px rgba(0, 0, 0, 0.35);
}

.dialog-header {
  padding: 1rem 1.15rem 0.5rem;
  border-bottom: 1px solid var(--color-border);
}

.dialog-header h2 {
  margin: 0;
  font-size: 1.1rem;
  font-weight: 600;
}

.dialog-sub {
  margin: 0.35rem 0 0;
  font-size: 0.85rem;
  color: var(--color-text-muted);
  line-height: 1.4;
}

.dialog-toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 0.45rem;
  padding: 0.65rem 1.15rem;
  border-bottom: 1px solid var(--color-border);
  background: var(--color-bg-muted);
}

.dialog-body {
  flex: 1;
  overflow: auto;
  padding: 0.25rem 1.15rem;
  min-height: 120px;
}

.dialog-footer {
  padding: 0.75rem 1.15rem 1rem;
  border-top: 1px solid var(--color-border);
  background: var(--color-bg-muted);
}

.footer-actions {
  display: flex;
  justify-content: flex-end;
  gap: 0.5rem;
}

.footer-hint {
  margin: 0.5rem 0 0;
  font-size: 0.75rem;
  color: var(--color-text-muted);
  text-align: right;
}

.btn {
  font-size: 0.85rem;
  padding: 0.45rem 0.85rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg);
  cursor: pointer;
}

.btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.btn.primary {
  background: var(--color-accent, #2a7de1);
  border-color: transparent;
  color: #fff;
  font-weight: 600;
  min-width: 9rem;
}

.error {
  margin: 0 0 0.5rem;
  font-size: 0.82rem;
  color: var(--color-danger, #c0392b);
}
</style>

<style>
body.config-merge-blocking .app-shell {
  pointer-events: none;
  user-select: none;
  filter: saturate(0.85);
}
</style>
