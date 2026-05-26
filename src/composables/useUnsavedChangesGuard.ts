import { readonly, ref } from "vue";

export type UnsavedDialogKind = "project" | "burn";
export type UnsavedDialogContext = "quit" | "switch";

export interface UnsavedDialogState {
  kind: UnsavedDialogKind;
  context: UnsavedDialogContext;
}

export interface UnsavedChangesCheck {
  context: UnsavedDialogContext;
  projectDirty: boolean;
  projectPath: string | null;
  burnPending: boolean;
  ecuConnected: boolean;
  canBurn: boolean;
  saveProject: () => Promise<string | null>;
  saveProjectAs: () => Promise<string | null>;
  burnConfig: () => Promise<void>;
}

type DialogResult = "primary" | "skip" | "cancel";

const dialogState = ref<UnsavedDialogState | null>(null);
let pendingResolve: ((result: DialogResult) => void) | null = null;

function needsProjectPrompt(check: UnsavedChangesCheck): boolean {
  return check.projectDirty;
}

function needsBurnPrompt(check: UnsavedChangesCheck): boolean {
  return check.burnPending && check.ecuConnected;
}

function showDialog(state: UnsavedDialogState): Promise<DialogResult> {
  return new Promise((resolve) => {
    pendingResolve = resolve;
    dialogState.value = state;
  });
}

function resolveDialog(result: DialogResult): void {
  dialogState.value = null;
  pendingResolve?.(result);
  pendingResolve = null;
}

/**
 * Последовательно спрашивает про несохранённый проект и burn (если нужно).
 * @returns true — можно продолжать (выход / смена проекта)
 */
export async function confirmUnsavedChanges(check: UnsavedChangesCheck): Promise<boolean> {
  if (needsProjectPrompt(check)) {
    const result = await showDialog({ kind: "project", context: check.context });
    if (result === "cancel") return false;
    if (result === "primary") {
      const saved = check.projectPath
        ? await check.saveProject()
        : await check.saveProjectAs();
      if (saved === null) return false;
    }
  }

  if (needsBurnPrompt(check)) {
    const result = await showDialog({ kind: "burn", context: check.context });
    if (result === "cancel") return false;
    if (result === "primary" && check.canBurn) {
      await check.burnConfig();
    }
  }

  return true;
}

export function useUnsavedChangesGuard() {
  return {
    dialogState: readonly(dialogState),
    onDialogPrimary: () => resolveDialog("primary"),
    onDialogSkip: () => resolveDialog("skip"),
    onDialogCancel: () => resolveDialog("cancel"),
    confirmUnsavedChanges,
  };
}
