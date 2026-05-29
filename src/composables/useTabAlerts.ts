import {
  onUnmounted,
  readonly,
  shallowRef,
  toValue,
  watch,
  type MaybeRefOrGetter,
} from "vue";

/** Визуальный вариант подсветки таба (CSS: `.tab-alert--{variant}`). */
export type TabAlertVariant = "spin-border";

export type TabAlertSeverity = "error" | "warn" | "info";

export interface TabAlert {
  variant: TabAlertVariant;
  severity: TabAlertSeverity;
}

const alerts = shallowRef(new Map<string, TabAlert>());

export function setTabAlert(tabId: string, alert: TabAlert | null | undefined): void {
  const next = new Map(alerts.value);
  if (alert) {
    next.set(tabId, alert);
  } else {
    next.delete(tabId);
  }
  alerts.value = next;
}

export function getTabAlert(tabId: string): TabAlert | undefined {
  return alerts.value.get(tabId);
}

/** CSS-классы для обёртки таба (`.header-tab-slot`). */
export function tabAlertClasses(tabId: string): Record<string, boolean> {
  const alert = alerts.value.get(tabId);
  if (!alert) return {};
  return {
    "tab-alert": true,
    [`tab-alert--${alert.variant}`]: true,
    [`tab-alert--${alert.severity}`]: true,
  };
}

export function useTabAlerts() {
  return {
    alerts: readonly(alerts),
    setTabAlert,
    getTabAlert,
    tabAlertClasses,
  };
}

/** Реактивная привязка alert к табу; снимается при unmount. */
export function useTabAlertBinding(
  tabId: string,
  source: MaybeRefOrGetter<TabAlert | null | undefined>,
): void {
  watch(
    () => toValue(source),
    (alert) => setTabAlert(tabId, alert ?? null),
    { immediate: true },
  );
  onUnmounted(() => setTabAlert(tabId, null));
}
