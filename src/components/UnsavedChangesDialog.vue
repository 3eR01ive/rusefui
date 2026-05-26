<script setup lang="ts">
import { computed } from "vue";
import type { UnsavedDialogState } from "../composables/useUnsavedChangesGuard";

const props = defineProps<{
  state: UnsavedDialogState | null;
}>();

const emit = defineEmits<{
  primary: [];
  skip: [];
  cancel: [];
}>();

const visible = computed(() => props.state !== null);

const title = computed(() => {
  if (!props.state) return "";
  return props.state.kind === "project"
    ? "Несохранённый проект"
    : "Несохранённые изменения";
});

const body = computed(() => {
  if (!props.state) return "";
  const { kind, context } = props.state;
  if (kind === "project") {
    return context === "quit"
      ? "Файл проекта изменён, но не сохранён на диск.<br>Сохранить перед выходом?"
      : "Файл проекта изменён, но не сохранён на диск.<br>Сохранить перед сменой проекта?";
  }
  return context === "quit"
    ? "Конфигурация изменена, но не записана во flash ECU.<br>Записать перед выходом?"
    : "Конфигурация изменена, но не записана во flash ECU.<br>Записать перед сменой проекта?";
});

const primaryLabel = computed(() => labelFor(props.state, "primary"));
const skipLabel = computed(() => labelFor(props.state, "skip"));

function labelFor(
  state: UnsavedDialogState | null,
  slot: "primary" | "skip",
): string {
  if (!state) return "";
  const { kind, context } = state;
  if (kind === "project") {
    if (slot === "primary") {
      return context === "quit" ? "Сохранить и выйти" : "Сохранить и продолжить";
    }
    return context === "quit" ? "Выйти без сохранения" : "Не сохранять";
  }
  if (slot === "primary") {
    return context === "quit" ? "Burn и выйти" : "Burn и продолжить";
  }
  return context === "quit" ? "Выйти без Burn" : "Продолжить без Burn";
}

const primaryClass = computed(() =>
  props.state?.kind === "burn" ? "burn" : "save",
);
</script>

<template>
  <div
    v-if="visible"
    class="unsaved-dialog-overlay"
    @click.self="emit('cancel')"
  >
    <div class="unsaved-dialog" role="alertdialog" aria-modal="true">
      <h3 class="unsaved-dialog-title">{{ title }}</h3>
      <p class="unsaved-dialog-body" v-html="body" />
      <div class="unsaved-dialog-actions">
        <button
          type="button"
          class="unsaved-dialog-btn primary"
          :class="primaryClass"
          @click="emit('primary')"
        >
          {{ primaryLabel }}
        </button>
        <button type="button" class="unsaved-dialog-btn skip" @click="emit('skip')">
          {{ skipLabel }}
        </button>
        <button type="button" class="unsaved-dialog-btn cancel" @click="emit('cancel')">
          Отмена
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.unsaved-dialog-overlay {
  position: fixed;
  inset: 0;
  z-index: 9100;
  background: rgba(0, 0, 0, 0.6);
  display: flex;
  align-items: center;
  justify-content: center;
}

.unsaved-dialog {
  background: var(--color-bg-elevated, #1e1e2e);
  border: 1px solid var(--color-border-strong, #4b5563);
  border-radius: 10px;
  padding: 1.75rem 2rem;
  max-width: 420px;
  width: 90%;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
}

.unsaved-dialog-title {
  margin: 0 0 0.75rem;
  font-size: 1rem;
  font-weight: 700;
  color: var(--color-fg);
}

.unsaved-dialog-body {
  margin: 0 0 1.5rem;
  font-size: 0.875rem;
  color: var(--color-fg-muted, #9ca3af);
  line-height: 1.5;
}

.unsaved-dialog-actions {
  display: flex;
  gap: 0.5rem;
  justify-content: flex-end;
  flex-wrap: wrap;
}

.unsaved-dialog-btn {
  padding: 0.45rem 1rem;
  border-radius: var(--radius-sm);
  font-size: 0.82rem;
  font-weight: 600;
  cursor: pointer;
  border: 1px solid transparent;
  transition: filter 0.15s;
}

.unsaved-dialog-btn:hover {
  filter: brightness(1.1);
}

.unsaved-dialog-btn.primary.save {
  background: var(--color-accent, #b45309);
  color: #fff;
  border-color: var(--color-accent, #b45309);
}

.unsaved-dialog-btn.primary.burn {
  background: #ea580c;
  color: #fff;
  border-color: #ea580c;
}

.unsaved-dialog-btn.skip {
  background: var(--color-bg-muted, #374151);
  color: var(--color-fg);
  border-color: var(--color-border-strong);
}

.unsaved-dialog-btn.cancel {
  background: transparent;
  color: var(--color-fg-muted);
  border-color: var(--color-border);
}
</style>
