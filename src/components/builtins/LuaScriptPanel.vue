<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useRustComponent } from "../../composables/useRustComponent";
import MonacoEditor from "../MonacoEditor.vue";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

function mountPayload(): Record<string, unknown> {
  const field = props.props.scriptField;
  return {
    scriptField: typeof field === "string" && field.length > 0 ? field : "",
  };
}

const { state, dispatch, ready, error, mounting } = useRustComponent(
  props.instance,
  props.path,
  mountPayload,
);

const scriptText = ref("");
const dirty = ref(false);

const connected = computed(() => Boolean(state.value.connected));
const busy = computed(() => Boolean(state.value.busy));
const maxBytes = computed(() => Number(state.value.maxBytes ?? 0));
const scriptField = computed(() => String(state.value.scriptField ?? ""));
const message = computed(() => String(state.value.message ?? ""));
const localError = computed(() => (error.value ? String(error.value) : ""));

const sizeLabel = computed(() => {
  const max = maxBytes.value;
  const len = scriptText.value.length;
  if (max > 0) return `${len} / ${max}`;
  return String(len);
});

const overLimit = computed(() => maxBytes.value > 0 && scriptText.value.length >= maxBytes.value);

watch(
  () => state.value.script,
  (v) => {
    if (typeof v === "string" && !dirty.value) {
      scriptText.value = v;
    }
  },
);

watch(ready, (isReady) => {
  if (isReady && typeof state.value.script === "string") {
    scriptText.value = state.value.script;
    dirty.value = false;
  }
});

function onInput(): void {
  dirty.value = true;
}

async function readFromEcu(): Promise<void> {
  await dispatch("read");
  scriptText.value = String(state.value.script ?? "");
  dirty.value = false;
}

async function writeToEcu(): Promise<void> {
  await dispatch("write", { text: scriptText.value });
  dirty.value = false;
}

async function burnToFlash(): Promise<void> {
  await dispatch("burn");
}

async function resetLua(): Promise<void> {
  await dispatch("reset_lua");
}
</script>

<template>
  <div class="lua-script-panel">
    <header class="lua-toolbar">
      <button type="button" class="lua-btn" :disabled="busy || mounting" @click="readFromEcu">
        Прочитать с ECU
      </button>
      <button
        type="button"
        class="lua-btn lua-btn--primary"
        :disabled="busy || mounting || !connected || overLimit"
        @click="writeToEcu"
      >
        Записать в ECU
      </button>
      <button type="button" class="lua-btn" :disabled="busy || mounting || !connected" @click="burnToFlash">
        Burn
      </button>
      <button type="button" class="lua-btn" :disabled="busy || mounting || !connected" @click="resetLua">
        luareset
      </button>
      <span v-if="scriptField" class="lua-field-tag">{{ scriptField }}</span>
      <a
        class="lua-wiki"
        href="https://wiki.rusefi.com/Lua-Scripting"
        target="_blank"
        rel="noopener noreferrer"
      >Lua Wiki</a>
    </header>

    <p v-if="localError || message" class="lua-status" :class="{ 'lua-status--error': !!localError }">
      {{ localError || message }}
    </p>

    <div class="lua-editor-wrap">
      <MonacoEditor
        v-model="scriptText"
        language="lua"
        class="lua-monaco"
        :read-only="busy"
        @change="onInput"
      />
      <footer class="lua-editor-footer">
        <span v-if="dirty" class="lua-dirty">изменено</span>
        <span class="lua-size" :class="{ 'lua-size--over': overLimit }">{{ sizeLabel }}</span>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.lua-script-panel {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  height: 100%;
  min-height: 0;
  padding: 0.5rem;
  box-sizing: border-box;
}

.lua-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
  flex-shrink: 0;
}

.lua-btn {
  padding: 0.35rem 0.75rem;
  border: 1px solid var(--color-border-strong);
  border-radius: 4px;
  background: var(--color-bg-elevated);
  color: var(--color-text);
  font-size: 0.85rem;
  cursor: pointer;
}

.lua-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.lua-btn--primary {
  background: var(--color-accent);
  border-color: var(--color-accent);
  color: var(--color-on-accent);
}

.lua-field-tag {
  font-size: 0.75rem;
  color: var(--color-text-muted);
  margin-left: auto;
}

.lua-wiki {
  font-size: 0.8rem;
  color: var(--color-accent);
}

.lua-status {
  margin: 0;
  font-size: 0.8rem;
  color: var(--color-text-muted);
  flex-shrink: 0;
}

.lua-status--error {
  color: var(--color-error);
}

.lua-editor-wrap {
  flex: 1 1 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--color-border);
  border-radius: 6px;
  background: var(--color-bg-elevated);
  overflow: hidden;
}

.lua-monaco {
  flex: 1;
  min-height: 0;
}

.lua-editor-footer {
  display: flex;
  justify-content: space-between;
  padding: 0.25rem 0.75rem;
  border-top: 1px solid var(--color-border);
  font-size: 0.75rem;
  color: var(--color-text-subtle);
  flex-shrink: 0;
}

.lua-dirty {
  color: var(--color-accent);
}

.lua-size--over {
  color: var(--color-error);
  font-weight: 600;
}
</style>
