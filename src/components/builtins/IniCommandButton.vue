<script setup lang="ts">
import { computed } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useRustComponent } from "../../composables/useRustComponent";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

function mountPayload(): Record<string, unknown> {
  const label = props.props.label;
  const command = props.props.command;
  return {
    label: typeof label === "string" ? label : "",
    command: typeof command === "string" ? command : "",
  };
}

const { state, dispatch, error, ready } = useRustComponent(
  props.instance,
  props.path,
  mountPayload,
);

const label = computed(() => String(state.value.label || props.props.label || "Run"));
const connected = computed(() => Boolean(state.value.connected));
const busy = computed(() => Boolean(state.value.busy));
const message = computed(() => (state.value.message as string) ?? null);
const messageIsError = computed(() => Boolean(state.value.messageIsError));
const canRun = computed(() => ready.value && connected.value && !busy.value);

async function run(): Promise<void> {
  if (!canRun.value) return;
  try {
    await dispatch("run");
  } catch {
    // error surfaced via composable / state.message
  }
}
</script>

<template>
  <div class="ini-cmd">
    <button type="button" class="ini-cmd-btn" :disabled="!canRun" @click="run">
      {{ busy ? "…" : label }}
    </button>
    <p v-if="message" class="ini-cmd-msg" :class="{ 'ini-cmd-msg--err': messageIsError }">
      {{ message }}
    </p>
    <p v-else-if="error" class="ini-cmd-msg ini-cmd-msg--err">{{ error }}</p>
  </div>
</template>

<style scoped>
.ini-cmd {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.35rem;
  margin: 0.15rem 0;
}

.ini-cmd-btn {
  padding: 0.5rem 0.85rem;
  border-radius: var(--radius-md, 8px);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg-accent-soft, rgba(255, 255, 255, 0.06));
  color: var(--color-text);
  font-weight: 600;
  font-size: 0.88rem;
  cursor: pointer;
}

.ini-cmd-btn:hover:not(:disabled) {
  border-color: var(--color-accent);
  color: var(--color-accent);
}

.ini-cmd-btn:disabled {
  opacity: 0.38;
  cursor: not-allowed;
}

.ini-cmd-msg {
  margin: 0;
  font-size: 0.78rem;
  color: var(--color-text-muted);
}

.ini-cmd-msg--err {
  color: var(--color-error);
}
</style>
