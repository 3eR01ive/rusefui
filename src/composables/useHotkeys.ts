import { onMounted, onUnmounted } from "vue";
import { activeTabId } from "./useTabState";

/** Список tab id в порядке навигации — заполняется при монтировании TabWorkspace. */
export const tabOrder = { value: [] as string[] };

/** Коллбэк сохранения проекта — регистрируется из AppShell. */
export const saveProjectCallback = { value: null as (() => void) | null };
export const openProjectCallback = { value: null as (() => void) | null };
export const burnCallback = { value: null as (() => void) | null };
export const undoCallback = { value: null as (() => void | Promise<void>) | null };
export const redoCallback = { value: null as (() => void | Promise<void>) | null };

/** Enter без модификаторов на активной вкладке — по tab id. */
const tabEnterHandlers = new Map<string, () => void>();

function isEditableTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  return el.isContentEditable;
}

/** Регистрирует обработчик Enter для вкладки (снимается при unmount). */
export function useTabEnterHandler(tabId: string, handler: () => void): void {
  onMounted(() => {
    tabEnterHandlers.set(tabId, handler);
  });
  onUnmounted(() => {
    tabEnterHandlers.delete(tabId);
  });
}

export function useGlobalHotkeys() {
  function onKeydown(e: KeyboardEvent) {
    if (e.defaultPrevented) return;

    if (e.ctrlKey || e.metaKey) {
      // e.code не зависит от раскладки клавиатуры
      if (e.code === "KeyZ" && !e.shiftKey) {
        if (!isEditableTarget(e.target)) {
          e.preventDefault();
          void undoCallback.value?.();
        }
        return;
      }
      if (e.code === "KeyZ" && e.shiftKey) {
        e.preventDefault();
        void redoCallback.value?.();
        return;
      }
      if (e.code === "KeyY") {
        e.preventDefault();
        void redoCallback.value?.();
        return;
      }
      if (e.code === "KeyS") {
        e.preventDefault();
        saveProjectCallback.value?.();
        return;
      }
      if (e.code === "KeyO") {
        e.preventDefault();
        openProjectCallback.value?.();
        return;
      }
      if (e.code === "Enter") {
        e.preventDefault();
        burnCallback.value?.();
        return;
      }
    }

    if (e.key === "Enter" && !e.altKey && !e.shiftKey) {
      const handler = tabEnterHandlers.get(activeTabId.value);
      if (handler && !isEditableTarget(e.target)) {
        e.preventDefault();
        handler();
        return;
      }
    }

    if (!e.altKey) return;

    const ids = tabOrder.value;
    if (!ids.length) return;

    const current = ids.indexOf(activeTabId.value);

    if (e.key === "ArrowRight") {
      e.preventDefault();
      if (current < ids.length - 1) activeTabId.value = ids[current + 1]!;
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      if (current > 0) activeTabId.value = ids[current - 1]!;
    }
  }

  onMounted(() => window.addEventListener("keydown", onKeydown, true));
  onUnmounted(() => window.removeEventListener("keydown", onKeydown, true));
}
