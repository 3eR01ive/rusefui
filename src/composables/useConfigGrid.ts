import { computed, ref, watch, type ComputedRef } from "vue";
import { initConfig, useConfig } from "./useConfig";

export type ConfigGridKind = "table" | "curve";

export interface UseConfigGridOptions {
  kind: ConfigGridKind;
  props: ComputedRef<Record<string, unknown>>;
}

export function useConfigGrid({ kind, props }: UseConfigGridOptions) {
  void initConfig();

  const { snapshot, getArray, setArrayValue } = useConfig();

  const isCurve = kind === "curve";

  const title = computed(() => String(props.value.title ?? ""));
  const xField = computed(() => String(props.value.xBins ?? ""));
  const yField = computed(() => String(props.value.yBins ?? ""));
  const zField = computed(() => String(props.value.zBins ?? ""));
  const xLabel = computed(() => String(props.value.xLabel ?? "X"));
  const yLabel = computed(() => String(props.value.yLabel ?? "Y"));

  const xValues = ref<number[]>([]);
  const yAxisValues = ref<number[]>([]);
  const zValues = ref<number[]>([]);
  const loading = ref(false);
  const localError = ref<string | null>(null);
  const saving = ref(false);

  const valueField = computed(() => (isCurve ? yField.value : zField.value));

  const cols = computed(() => {
    if (isCurve) return 1;
    const n = xValues.value.length;
    return n > 0 ? n : Math.max(1, Math.round(Math.sqrt(zValues.value.length)));
  });

  const rows = computed(() => {
    if (isCurve) {
      return Math.max(
        xValues.value.length,
        zValues.value.length,
        yAxisValues.value.length,
      );
    }
    const n = yAxisValues.value.length;
    if (n > 0) return n;
    const c = cols.value;
    return c > 0 ? Math.max(1, Math.ceil(zValues.value.length / c)) : 1;
  });

  const colIndices = computed(() => Array.from({ length: cols.value }, (_, i) => i));
  const rowIndices = computed(() => Array.from({ length: rows.value }, (_, i) => i));

  const disabled = computed(
    () =>
      !valueField.value ||
      !snapshot.value.connected ||
      !snapshot.value.loaded ||
      snapshot.value.loading ||
      loading.value ||
      saving.value,
  );

  function fmt(v: number): string {
    if (!Number.isFinite(v)) return "";
    if (Number.isInteger(v)) return String(v);
    const s = v.toFixed(3);
    return s.replace(/\.?0+$/, "");
  }

  function cellIndex(row: number, col: number): number {
    return row * cols.value + col;
  }

  function cellValue(row: number, col: number): number | null {
    const idx = cellIndex(row, col);
    const v = zValues.value[idx];
    return v === undefined ? null : v;
  }

  async function reload() {
    if (!snapshot.value.loaded) return;
    loading.value = true;
    localError.value = null;
    try {
      if (xField.value) {
        xValues.value = await getArray(xField.value);
      }
      if (yField.value && !isCurve) {
        yAxisValues.value = await getArray(yField.value);
      }
      if (isCurve && yField.value) {
        zValues.value = await getArray(yField.value);
        if (!xField.value) {
          xValues.value = zValues.value.map((_, i) => i);
        }
      } else if (valueField.value) {
        zValues.value = await getArray(valueField.value);
      }
      if (isCurve && xField.value && xValues.value.length === 0) {
        xValues.value = await getArray(xField.value);
      }
    } catch (e) {
      localError.value = e instanceof Error ? e.message : String(e);
    } finally {
      loading.value = false;
    }
  }

  watch(
    () => snapshot.value.loaded,
    (loaded) => {
      if (loaded) void reload();
    },
    { immediate: true },
  );

  async function commitCell(row: number, col: number, raw: string) {
    if (disabled.value || !valueField.value) return;
    const parsed = Number(raw.trim().replace(",", "."));
    if (!Number.isFinite(parsed)) {
      localError.value = "некорректное число";
      return;
    }
    const idx = cellIndex(row, col);
    const current = cellValue(row, col);
    if (current !== null && Math.abs(current - parsed) < 1e-9) return;

    saving.value = true;
    localError.value = null;
    try {
      await setArrayValue(valueField.value, idx, parsed);
      zValues.value[idx] = parsed;
    } catch (e) {
      localError.value = e instanceof Error ? e.message : String(e);
      await reload();
    } finally {
      saving.value = false;
    }
  }

  const statusText = computed(() => {
    if (localError.value) return localError.value;
    if (saving.value) return "сохранение…";
    if (loading.value || snapshot.value.loading) return "загрузка…";
    if (!snapshot.value.connected) return "нет подключения";
    if (!snapshot.value.loaded) return "ожидание config…";
    return isCurve ? "кривая" : "таблица";
  });

  return {
    title,
    xLabel,
    yLabel,
    colIndices,
    rowIndices,
    disabled,
    fmt,
    cellValue,
    commitCell,
    statusText,
    localError,
    xValues,
    yAxisValues,
    zValues,
  };
}
