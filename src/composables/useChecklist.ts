import { computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { readProjectUiConfig } from "../core/config-loader";
import { useFooterSlot } from "./useAppFooter";
import { useTabAlertBinding } from "./useTabAlerts";
import { configCanView, initConfig, useConfig } from "./useConfig";
import { useWorkspaceState } from "./useWorkspaceState";

let initPromise: Promise<void> | null = null;

export async function initChecklist(): Promise<void> {
  if (initPromise) return initPromise;

  initPromise = (async () => {
    await initConfig();
    try {
      const yaml = await readProjectUiConfig("checklist.yaml");
      await invoke("checklist_load_rules", { yaml });
    } catch (e) {
      console.warn("checklist rules load failed:", e);
    }
  })();

  return initPromise;
}

export function useChecklistFooter(): void {
  const { snapshot } = useConfig();
  const { showMainUi } = useWorkspaceState();

  const footerText = computed(() => {
    const snap = snapshot.value;
    const c = snap.checklist;
    if (!showMainUi.value || !configCanView(snap) || !c?.evaluated || c.ok) {
      return null;
    }
    const count = c.issues.length;
    const first = c.issues[0]?.message;
    if (count === 1 && first) {
      return `Checklist: ${first}`;
    }
    return `Checklist: ${count} нарушений — откройте CHKLST`;
  });

  const footerOpts = computed(() => {
    const snap = snapshot.value;
    const c = snap.checklist;
    if (!showMainUi.value || !configCanView(snap) || !c?.evaluated || c.ok) {
      return undefined;
    }
    const hasError = c.issues.some(
      (i) => i.severity === "error" || i.severity === "critical",
    );
    return {
      warn: !hasError,
      error: hasError,
      priority: hasError ? 25 : 20,
    };
  });

  useFooterSlot("config:checklist", footerText, footerOpts);
}

/** Подсветка таба checklist при невыполненном уровня severity: error. */
export function useChecklistTabAlert(): void {
  const { snapshot } = useConfig();
  const { showMainUi } = useWorkspaceState();

  useTabAlertBinding("checklist", () => {
    const snap = snapshot.value;
    const c = snap.checklist;
    if (!showMainUi.value || !configCanView(snap) || !c?.evaluated) {
      return null;
    }
    const hasErrorLevelFail = c.levels.some(
      (level) =>
        (level.severity === "error" || level.severity === "critical") && !level.ok,
    );
    if (!hasErrorLevelFail) return null;
    return { variant: "spin-border", severity: "error" };
  });
}
