<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import type { DataBinding } from "../../core/types";
import { initConfig, useConfig } from "../../composables/useConfig";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

void initConfig();

const { snapshot, getField, getFieldInfo, setField } = useConfig();

const bind = computed(() => props.instance.bind as DataBinding | undefined);
const fieldName = computed(() => bind.value?.field ?? "");
const label = computed(
  () => String(props.props.label ?? (fieldName.value || "—")),
);

const fieldInfo = computed(() =>
  fieldName.value ? getFieldInfo(fieldName.value) : null,
);
const units = computed(() => fieldInfo.value?.units ?? "");

const displayValue = computed(() => {
  if (!fieldName.value) return "";
  const v = getField(fieldName.value);
  if (v === null) return "";
  if (Number.isInteger(v)) return String(v);
  const s = v.toFixed(4);
  return s.replace(/\.?0+$/, "");
});

const draft = ref("");
const saving = ref(false);
const localError = ref<string | null>(null);

watch(
  displayValue,
  (v) => {
    draft.value = v;
  },
  { immediate: true },
);

const statusText = computed(() => {
  if (localError.value) return localError.value;
  if (saving.value) return "сохранение…";
  if (snapshot.value.loading) return "загрузка конфига…";
  if (snapshot.value.lastError) return snapshot.value.lastError;
  if (!snapshot.value.connected) return "нет подключения";
  if (!snapshot.value.loaded) return "ожидание данных…";
  return units.value ? units.value : "config";
});

const disabled = computed(
  () =>
    !fieldName.value ||
    !snapshot.value.connected ||
    !snapshot.value.loaded ||
    snapshot.value.loading ||
    saving.value,
);

async function commit() {
  if (disabled.value || !fieldName.value) return;
  const parsed = Number(draft.value.trim().replace(",", "."));
  if (!Number.isFinite(parsed)) {
    localError.value = "некорректное число";
    draft.value = displayValue.value;
    return;
  }
  const current = getField(fieldName.value);
  if (current !== null && Math.abs(current - parsed) < 1e-9) return;

  saving.value = true;
  localError.value = null;
  try {
    await setField(fieldName.value, parsed);
  } catch (e) {
    localError.value = e instanceof Error ? e.message : String(e);
    draft.value = displayValue.value;
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div class="scalar-field">
    <label class="field-label">{{ label }}</label>
    <input
      v-model="draft"
      type="text"
      class="field-input"
      :class="{ 'field-input--error': !!localError }"
      :disabled="disabled"
      :placeholder="fieldName || '—'"
      @change="commit"
      @keydown.enter="($event.target as HTMLInputElement)?.blur()"
    />
    <span class="field-badge" :class="{ 'field-badge--error': !!localError || !!snapshot.lastError }">
      {{ statusText }}
    </span>
  </div>
</template>

<style scoped>
.scalar-field {
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
  max-width: none;
  width: 100%;
}

.field-label {
  font-size: 0.78rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-gray);
  font-weight: 500;
}

.field-input {
  padding: 0.5rem 0.65rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg);
  color: var(--color-text);
}

.field-input:disabled {
  background: var(--color-bg-muted);
  color: var(--color-text-muted);
}

.field-input--error {
  border-color: var(--color-danger, #c0392b);
}

.field-badge {
  font-size: 0.7rem;
  color: var(--color-text-subtle);
}

.field-badge--error {
  color: var(--color-danger, #c0392b);
}
</style>
