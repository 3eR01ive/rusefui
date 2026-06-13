<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import type { DataBinding } from "../../core/types";
import { configCanEdit, configCanView, initConfig, useConfig } from "../../composables/useConfig";
import {
  isIniPlaceholderLabel,
  type PinOptionAllocation,
} from "../../composables/pinAllocation";
import { usePinAllocation } from "../../composables/usePinAllocation";

interface EnumOptionProp {
  value: number;
  label: string;
}

function normalizeOptions(raw: EnumOptionProp[]): EnumOptionProp[] {
  return raw.filter((o) => !isIniPlaceholderLabel(o.label));
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
const { describeOption, usageIndex } = usePinAllocation();

const bind = computed(() => props.instance.bind as DataBinding | undefined);
const fieldName = computed(() => bind.value?.field ?? "");
const label = computed(
  () => String(props.props.label ?? (fieldName.value || "—")),
);

const pinPool = computed(() => {
  const info = fieldName.value ? getFieldInfo(fieldName.value) : null;
  return info?.pinPool ?? null;
});

const allOptions = computed((): EnumOptionProp[] => {
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

const visibleOptions = computed(() => normalizeOptions(allOptions.value));

const currentValue = computed(() => {
  if (!fieldName.value) return null;
  return getField(fieldName.value);
});

const selectOptions = computed((): EnumOptionProp[] => {
  const vis = visibleOptions.value;
  const cur = currentValue.value;
  if (cur === null || vis.some((o) => o.value === cur)) {
    return vis;
  }
  const stored = allOptions.value.find((o) => o.value === cur);
  const orphanLabel = stored ? `${stored.label} (${cur})` : `#${cur}`;
  return [{ value: cur, label: orphanLabel }, ...vis];
});

const optionMeta = computed(() => {
  void usageIndex.value;
  const map = new Map<number, PinOptionAllocation>();
  if (!pinPool.value) return map;
  for (const opt of selectOptions.value) {
    map.set(
      opt.value,
      describeOption(pinPool.value, fieldName.value, opt.value, opt.label),
    );
  }
  return map;
});

const currentPinConflict = computed(() => {
  if (currentValue.value === null || !pinPool.value) return null;
  return optionMeta.value.get(currentValue.value) ?? null;
});

function metaFor(opt: EnumOptionProp): PinOptionAllocation {
  return (
    optionMeta.value.get(opt.value) ?? {
      selectable: true,
      suffix: "",
      title: "",
      cssClass: "",
    }
  );
}

function optionDisabled(opt: EnumOptionProp): boolean {
  const m = metaFor(opt);
  if (m.selectable) return false;
  return selected.value !== opt.value;
}

function optionText(opt: EnumOptionProp): string {
  const m = metaFor(opt);
  return opt.label + m.suffix;
}

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
  const conflict = currentPinConflict.value;
  if (conflict && !conflict.selectable) {
    return conflict.title || "конфликт пинов";
  }
  if (saving.value) return "сохранение…";
  if (snapshot.value.loading) return "загрузка конфига…";
  if (snapshot.value.lastError) return snapshot.value.lastError;
  if (snapshot.value.readOnly && snapshot.value.loaded) return "проект (offline)";
  if (!snapshot.value.connected && !configCanView(snapshot.value)) return "нет подключения";
  if (!snapshot.value.loaded) return "ожидание данных…";
  return pinPool.value ? "enum · пины" : "enum";
});

const disabled = computed(
  () =>
    !fieldName.value ||
    selectOptions.value.length === 0 ||
    !configCanEdit(snapshot.value) ||
    saving.value,
);

const selectClass = computed(() => ({
  "field-select--error": !!localError.value,
  "field-select--pin-conflict":
    !!currentPinConflict.value && !currentPinConflict.value.selectable,
}));

async function commit() {
  if (disabled.value || !fieldName.value || selected.value === "") return;
  const value = Number(selected.value);
  if (!Number.isFinite(value)) {
    localError.value = "некорректное значение";
    return;
  }
  const opt = selectOptions.value.find((o) => o.value === value);
  if (opt) {
    const m = metaFor(opt);
    if (!m.selectable && currentValue.value !== value) {
      localError.value = m.title || "Пин уже занят другим полем";
      selected.value = currentValue.value === null ? "" : currentValue.value;
      return;
    }
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
      :class="selectClass"
      :disabled="disabled"
      @change="commit"
    >
      <option v-if="selected === ''" disabled value="">—</option>
      <option
        v-for="opt in selectOptions"
        :key="`${opt.value}-${opt.label}`"
        :value="opt.value"
        :disabled="optionDisabled(opt)"
        :class="metaFor(opt).cssClass"
        :title="metaFor(opt).title"
      >
        {{ optionText(opt) }}
      </option>
    </select>
    <span
      class="field-badge"
      :class="{
        'field-badge--error':
          !!localError || !!snapshot.lastError || !!currentPinConflict?.title,
      }"
    >
      {{ statusText }}
    </span>
  </div>
</template>

<style scoped>
.enum-field {
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

.field-select {
  flex: 1 1 80px;
  min-width: 0;
  padding: 0.2rem 0.45rem;
  font-size: 0.85rem;
  border-radius: var(--radius-sm, var(--radius-md));
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg);
  color: var(--color-text);
}

.field-select:disabled {
  background: var(--color-bg-muted);
  color: var(--color-text-muted);
}

.field-select--error,
.field-select--pin-conflict {
  border-color: var(--color-danger, #c0392b);
}

.field-select option.pin-option--conflict {
  color: var(--color-danger, #c0392b);
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
