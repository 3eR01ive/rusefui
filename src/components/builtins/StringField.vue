<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import type { DataBinding } from "../../core/types";
import { configCanEdit, configCanView, initConfig, useConfig } from "../../composables/useConfig";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

void initConfig();

const { snapshot, getStringField, getFieldInfo, setStringField } = useConfig();

const bind = computed(() => props.instance.bind as DataBinding | undefined);
const fieldName = computed(() => bind.value?.field ?? "");
const label = computed(
  () => String(props.props.label ?? (fieldName.value || "—")),
);

const maxLength = computed(() => {
  const fromYaml = Number(props.props.maxLength);
  if (Number.isFinite(fromYaml) && fromYaml > 0) return fromYaml;
  const info = fieldName.value ? getFieldInfo(fieldName.value) : null;
  if (info?.arrayLength && info.arrayLength > 0) return info.arrayLength;
  return undefined;
});

const displayValue = computed(() => {
  if (!fieldName.value) return "";
  return getStringField(fieldName.value) ?? "";
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
  if (snapshot.value.readOnly && snapshot.value.loaded) return "проект (offline)";
  if (!snapshot.value.connected && !configCanView(snapshot.value)) return "нет подключения";
  if (!snapshot.value.loaded) return "ожидание данных…";
  return maxLength.value ? `до ${maxLength.value} симв.` : "config";
});

const disabled = computed(
  () =>
    !fieldName.value ||
    !configCanEdit(snapshot.value) ||
    saving.value,
);

async function commit() {
  if (disabled.value || !fieldName.value) return;
  const trimmed = draft.value.trimEnd();
  const limit = maxLength.value;
  if (limit !== undefined && trimmed.length >= limit) {
    localError.value = `максимум ${limit - 1} символов`;
    draft.value = displayValue.value;
    return;
  }
  if (trimmed === displayValue.value) return;

  saving.value = true;
  localError.value = null;
  try {
    await setStringField(fieldName.value, trimmed);
  } catch (e) {
    localError.value = e instanceof Error ? e.message : String(e);
    draft.value = displayValue.value;
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div class="string-field">
    <label class="field-label">{{ label }}</label>
    <input
      v-model="draft"
      type="text"
      class="field-input"
      :class="{ 'field-input--error': !!localError }"
      :disabled="disabled"
      :maxlength="maxLength"
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
.string-field {
  display: flex;
  align-items: center;
  gap: 0.45rem;
  min-width: 0;
  width: 100%;
}

.field-label {
  flex: 0 1 auto;
  min-width: 0;
  font-size: 0.78rem;
  color: var(--color-gray);
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.field-input {
  flex: 1 1 60px;
  min-width: 0;
  padding: 0.2rem 0.45rem;
  font-size: 0.85rem;
  border-radius: var(--radius-sm, var(--radius-md));
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
  flex: 0 0 auto;
  font-size: 0.68rem;
  color: var(--color-text-subtle);
  white-space: nowrap;
}

.field-badge--error {
  color: var(--color-danger, #c0392b);
}
</style>
