import { onMounted, onUnmounted } from "vue";
import { activeTabId } from "./useTabState";

/** Список tab id в порядке навигации — заполняется при монтировании TabWorkspace. */
export const tabOrder = { value: [] as string[] };

export function useGlobalHotkeys() {
  function onKeydown(e: KeyboardEvent) {
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

  onMounted(() => window.addEventListener("keydown", onKeydown));
  onUnmounted(() => window.removeEventListener("keydown", onKeydown));
}
