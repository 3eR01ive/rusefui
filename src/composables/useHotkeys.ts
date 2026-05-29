import { onMounted, onUnmounted } from "vue";

/** Список tab id в порядке навигации — заполняется при монтировании TabWorkspace. */
export const tabOrder = { value: [] as string[] };

/** Коллбэк сохранения проекта — регистрируется из AppShell. */
export const saveProjectCallback = { value: null as (() => void) | null };
export const openProjectCallback = { value: null as (() => void) | null };
export const burnCallback = { value: null as (() => void) | null };
export const undoCallback = { value: null as (() => void | Promise<void>) | null };
export const redoCallback = { value: null as (() => void | Promise<void>) | null };

/** Enter без модификаторов на активной вкладке — по tab id. */
export const tabEnterHandlers = new Map<string, () => void>();

/** Регистрирует обработчик Enter для вкладки (снимается при unmount). */
export function useTabEnterHandler(tabId: string, handler: () => void): void {
  onMounted(() => {
    tabEnterHandlers.set(tabId, handler);
  });
  onUnmounted(() => {
    tabEnterHandlers.delete(tabId);
  });
}
