/**
 * Неактивность вкладки: источники данных (Rust, Tauri events) работают всегда,
 * а отрисовка и render-IPC на скрытой вкладке ставятся на паузу.
 *
 * Provide на корне панели вкладки → inject в любом потомке дерева YAML.
 */
import {
  computed,
  inject,
  provide,
  ref,
  watch,
  watchEffect,
  type ComputedRef,
  type InjectionKey,
  type Ref,
} from "vue";
import { activeTabId } from "./useTabState";

export interface TabActivityContext {
  tabId: string;
  /** Вкладка выбрана в header (видима пользователю). */
  isActive: ComputedRef<boolean>;
}

const TabActivityKey: InjectionKey<TabActivityContext> = Symbol("rusefui.tabActivity");

export function provideTabActivity(tabId: string): TabActivityContext {
  const ctx: TabActivityContext = {
    tabId,
    isActive: computed(() => activeTabId.value === tabId),
  };
  provide(TabActivityKey, ctx);
  return ctx;
}

/** Контекст вкладки; вне дерева tab/* — всегда active (shell, модалки). */
export function useTabActivity(): TabActivityContext {
  const ctx = inject(TabActivityKey, null);
  if (ctx) return ctx;
  return {
    tabId: "",
    isActive: computed(() => true),
  };
}

/**
 * Пауза side-effect отрисовки: `gate()` не выполняет fn на неактивной вкладке.
 * При активации вкладки один раз вызывается `onResume` (срез + redraw).
 */
export function useTabRenderGate(onResume?: () => void) {
  const { isActive } = useTabActivity();

  function gate(fn: () => void): void {
    if (!isActive.value) return;
    fn();
  }

  if (onResume) {
    watch(isActive, (active, wasActive) => {
      if (active && !wasActive) onResume();
    });
  }

  return { isActive, gate, shouldRender: () => isActive.value };
}

/** UI-значение: на неактивной вкладке — последнее отрисованное; при возврате — sync. */
export function useTabFrozenDisplay<T>(compute: () => T, initial: T): ComputedRef<T> {
  const { isActive } = useTabActivity();
  const held = ref(initial) as Ref<T>;

  watchEffect(() => {
    if (isActive.value) {
      held.value = compute();
    }
  });

  return computed(() => held.value);
}
