import { computed, type ComputedRef, type MaybeRefOrGetter, toValue } from "vue";
import type { ComponentInstance, DataBinding, DataSourceId } from "../core/types";

/** Объект из `resolveBinding()` (проп `binding` в ComponentHost). */
type ResolvedBinding = {
  source?: string;
  field?: string;
  fields?: string[];
  params?: Record<string, unknown>;
};

function asStringArray(v: unknown): string[] {
  if (!Array.isArray(v)) return [];
  return v.map(String).filter(Boolean);
}

/**
 * Чтение `bind` из инстанса YAML (+ опционально `binding` из ComponentHost).
 */
export function useInstanceBind(
  instance: MaybeRefOrGetter<ComponentInstance>,
  resolvedBinding?: MaybeRefOrGetter<unknown>,
) {
  const bind = computed((): DataBinding | undefined => {
    const inst = toValue(instance).bind;
    const res = resolvedBinding
      ? (toValue(resolvedBinding) as ResolvedBinding | undefined)
      : undefined;
    if (!inst && !res) return undefined;
    const source = (inst?.source ?? res?.source) as DataSourceId | string | undefined;
    if (!source) return undefined;
    return {
      source,
      field: inst?.field ?? res?.field,
      fields: inst?.fields ?? res?.fields,
      params: { ...res?.params, ...inst?.params },
    };
  });

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
