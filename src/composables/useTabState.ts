import { ref } from "vue";

export const activeTabId = ref("");

export function useTabState() {
  function setTab(id: string) {
    activeTabId.value = id;
  }
  return { activeTabId, setTab };
}
