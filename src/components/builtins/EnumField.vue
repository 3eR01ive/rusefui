<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import type { DataBinding } from "../../core/types";
import { initConfig, useConfig } from "../../composables/useConfig";

interface EnumOptionProp {
  value: number;
  label: string;
}

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

const options = computed((): EnumOptionProp[] => {
  const fromYaml = props.props.options;
  if (Array.isArray(fromYaml) && fromYaml.length > 0) {
    return fromYaml.map((o) => {
      const item = o as Record<string, unknown>;
      return {
        value: Number(item.value),
        label: String(item.label ?? item.value),
      };
    });
  }
  const info = fieldName.value ? getFieldInfo(fieldName.value) : null;
  if (info?.options?.length) {
    return info.options.map((o) => ({
      value: o.value,
      label: o.label,
    }));
  }
  return [];
});

const currentValue = computed(() => {
  if (!fieldName.value) return null;
  return getField(fieldName.value);
});

const selected = ref<number | "">("");
const saving = ref(false);
const localError = ref<string | null>(null);

watch(
  currentValue,
  (v) => {
    selected.value = v === null ? "" : v;
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
  return "enum";
});

const disabled = computed(
  () =>
    !fieldName.value ||
    options.value.length === 0 ||
    !snapshot.value.connected ||
    !snapshot.value.loaded ||
    snapshot.value.loading ||
    saving.value,
);

async function commit() {
  if (disabled.value || !fieldName.value || selected.value === "") return;
  const value = Number(selected.value);
  if (!Number.isFinite(value)) {
    localError.value = "некорректное значение";
    return;
  }
  const current = getField(fieldName.value);
  if (current !== null && current === value) return;

  saving.value = true;
  localError.value = null;
  try {
    await setField(fieldName.value, value);
  } catch (e) {
    localError.value = e instanceof Error ? e.message : String(e);
    selected.value = currentValue.value === null ? "" : currentValue.value;
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div class="enum-field">
    <label class="field-label">{{ label }}</label>
    <select
      v-model="selected"
      class="field-select"
      :class="{ 'field-select--error': !!localError }"
      :disabled="disabled"
      @change="commit"
    >
      <option v-if="selected === ''" disabled value="">—</option>
      <option
        v-for="opt in options"
        :key="`${opt.value}-${opt.label}`"
        :value="opt.value"
      >
        {{ opt.label }}
      </option>
    </select>
    <span
      class="field-badge"
      :class="{ 'field-badge--error': !!localError || !!snapshot.lastError }"
    >
      {{ statusText }}
    </span>
  </div>
</template>

<style scoped>
.enum-field {
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

.field-select {
  padding: 0.5rem 0.65rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg);
  color: var(--color-text);
}

.field-select:disabled {
  background: var(--color-bg-muted);
  color: var(--color-text-muted);
}

.field-select--error {
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
