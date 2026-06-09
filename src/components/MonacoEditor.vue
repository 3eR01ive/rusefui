<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import type * as Monaco from "monaco-editor";
import { defineRusefuiTheme, ensureMonacoEnvironment } from "../monaco/environment";
import { useKeyboardSink } from "../composables/useKeyboardSink";

const props = withDefaults(
  defineProps<{
    modelValue: string;
    language?: string;
    readOnly?: boolean;
  }>(),
  {
    language: "plaintext",
    readOnly: false,
  },
);

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
  (e: "change", value: string): void;
}>();

const hostEl = ref<HTMLElement | null>(null);
const rootEl = ref<HTMLElement | null>(null);
useKeyboardSink(hostEl);
const ready = ref(false);
const editor = shallowRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
let monacoApi: typeof Monaco | null = null;
let resizeObserver: ResizeObserver | null = null;
let applyingExternalValue = false;

async function initEditor(): Promise<void> {
  if (!rootEl.value || editor.value) {
    return;
  }

  ensureMonacoEnvironment();
  monacoApi = await import("monaco-editor");
  defineRusefuiTheme(monacoApi);

  const instance = monacoApi.editor.create(rootEl.value, {
    value: props.modelValue,
    language: props.language,
    theme: "rusefui",
    readOnly: props.readOnly,
    automaticLayout: true,
    minimap: { enabled: false },
    scrollBeyondLastLine: false,
    wordWrap: "on",
    tabSize: 2,
    fontSize: 13,
    fontFamily: 'ui-monospace, "Cascadia Code", "Fira Code", monospace',
    lineNumbers: "on",
    renderLineHighlight: "line",
    padding: { top: 8, bottom: 8 },
    scrollbar: {
      verticalScrollbarSize: 10,
      horizontalScrollbarSize: 10,
    },
  });

  instance.onDidChangeModelContent(() => {
    if (applyingExternalValue) {
      return;
    }
    const next = instance.getValue();
    emit("update:modelValue", next);
    emit("change", next);
  });

  editor.value = instance;
  instance.updateOptions({ readOnly: props.readOnly });
  ready.value = true;

  resizeObserver = new ResizeObserver(() => {
    instance.layout();
  });
  resizeObserver.observe(rootEl.value);
  requestAnimationFrame(() => instance.layout());
}

function setEditorValue(value: string): void {
  const instance = editor.value;
  if (!instance || instance.getValue() === value) {
    return;
  }
  applyingExternalValue = true;
  const position = instance.getPosition();
  instance.setValue(value);
  if (position) {
    instance.setPosition(position);
  }
  applyingExternalValue = false;
}

watch(
  () => props.modelValue,
  (value) => {
    if (editor.value) {
      setEditorValue(value);
    }
  },
);

watch(
  () => [props.readOnly, editor.value] as const,
  ([readOnly, instance]) => {
    instance?.updateOptions({ readOnly });
  },
);

watch(
  () => props.language,
  (language) => {
    if (!editor.value || !monacoApi) {
      return;
    }
    const model = editor.value.getModel();
    if (model) {
      monacoApi.editor.setModelLanguage(model, language);
    }
  },
);

onMounted(() => {
  void initEditor();
});

onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  resizeObserver = null;
  editor.value?.dispose();
  editor.value = null;
});
</script>

<template>
  <div ref="hostEl" class="monaco-editor-host">
    <div ref="rootEl" class="monaco-editor-root" />
    <p v-if="!ready" class="monaco-editor-loading">Загрузка редактора…</p>
  </div>
</template>

<style scoped>
.monaco-editor-host {
  position: relative;
  flex: 1;
  min-height: 0;
  width: 100%;
}

.monaco-editor-root {
  position: absolute;
  inset: 0;
}

.monaco-editor-loading {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  margin: 0;
  font-size: 0.8rem;
  color: var(--color-text-muted);
  pointer-events: none;
}
</style>
