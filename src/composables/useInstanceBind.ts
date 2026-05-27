import { computed, type ComputedRef, type MaybeRefOrGetter, toValue } from "vue";
import type { ComponentInstance, DataBinding } from "../core/types";

function asStringArray(v: unknown): string[] {
  if (!Array.isArray(v)) return [];
  return v.map(String).filter(Boolean);
}

/**
 * Чтение `bind` из инстанса YAML: имена полей/каналов только отсюда, не из SFC.
 */
export function useInstanceBind(
  instance: MaybeRefOrGetter<ComponentInstance>,
) {
  const bind = computed(() => toValue(instance).bind);

  const source = computed(() => bind.value?.source);

  const field = computed(() => bind.value?.field);

  const fields = computed((): string[] => {
    const b = bind.value;
    if (!b) return [];
    if (b.fields?.length) return [...b.fields];
    const fromParams = asStringArray(b.params?.fields);
    if (fromParams.length) return fromParams;
    if (b.field) return [b.field];
    return [];
  });

  function paramString(key: string): string | undefined {
    const v = bind.value?.params?.[key];
    if (v === undefined || v === null) return undefined;
    const s = String(v).trim();
    return s || undefined;
  }

  function paramStringOr(key: string, fallback: string): string {
    return paramString(key) ?? fallback;
  }

  return {
    bind,
    source,
    field,
    fields,
    paramString,
    paramStringOr,
  };
}

export function requireBindSource(
  bind: ComputedRef<DataBinding | undefined>,
  expected: string,
): ComputedRef<boolean> {
  return computed(() => bind.value?.source === expected);
}
