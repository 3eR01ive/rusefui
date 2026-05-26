import { onMounted, onUnmounted } from "vue";
import { activeTabId } from "./useTabState";

/** Список tab id в порядке навигации — заполняется при монтировании TabWorkspace. */
export const tabOrder = { value: [] as string[] };

/** Коллбэк сохранения проекта — регистрируется из AppShell. */
export const saveProjectCallback = { value: null as (() => void) | null };
export const openProjectCallback = { value: null as (() => void) | null };
export const burnCallback = { value: null as (() => void) | null };

export function useGlobalHotkeys() {
  function onKeydown(e: KeyboardEvent) {
    if (e.ctrlKey || e.metaKey) {
      // e.code не зависит от раскладки клавиатуры
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
