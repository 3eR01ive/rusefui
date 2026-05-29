import { computed, shallowRef } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ConfigSnapshot } from "./useConfig";
import { isConfigMergeBlocking } from "./useConfigDiff";

export interface ArrayCellUpdate {
  index: number;
  oldValue: number;
  newValue: number;
}

export type ConfigCommand =
  | {
      type: "scalar";
      field: string;
      oldValue: number;
      newValue: number;
      label?: string;
    }
  | {
      type: "string";
      field: string;
      oldValue: string;
      newValue: string;
      label?: string;
    }
  | {
      type: "array";
      field: string;
      updates: ArrayCellUpdate[];
      label?: string;
    };

const MAX_HISTORY = 100;

const undoStack: ConfigCommand[] = [];
const redoStack: ConfigCommand[] = [];
const revision = shallowRef(0);
let applyingHistory = false;
let initPromise: Promise<void> | null = null;

function bumpRevision(): void {
  revision.value += 1;
}

export function clearConfigCommandHistory(): void {
  undoStack.length = 0;
  redoStack.length = 0;
  bumpRevision();
}

function stackTail(stack: ConfigCommand[]): ConfigCommand | undefined {
  return stack.length > 0 ? stack[stack.length - 1] : undefined;
}

export function configCommandHistoryState() {
  void revision.value;
  return {
    canUndo: undoStack.length > 0,
    canRedo: redoStack.length > 0,
    undoLabel: stackTail(undoStack)?.label ?? stackTail(undoStack)?.field ?? null,
    redoLabel: stackTail(redoStack)?.label ?? stackTail(redoStack)?.field ?? null,
  };
}

export function useConfigCommandHistory() {
  const canUndo = computed(() => {
    void revision.value;
    return undoStack.length > 0;
  });
  const canRedo = computed(() => {
    void revision.value;
    return redoStack.length > 0;
  });
  const undoLabel = computed(() => {
    void revision.value;
    const cmd = stackTail(undoStack);
    return cmd?.label ?? cmd?.field ?? null;
  });
  const redoLabel = computed(() => {
    void revision.value;
    const cmd = stackTail(redoStack);
    return cmd?.label ?? cmd?.field ?? null;
  });
  return { canUndo, canRedo, undoLabel, redoLabel };
}

function commandLabel(cmd: ConfigCommand): string | undefined {
  if (cmd.label) return cmd.label;
  if (cmd.type === "array") return cmd.field;
  return cmd.field;
}

function withLabel(cmd: ConfigCommand): ConfigCommand {
  const label = commandLabel(cmd);
  return label ? { ...cmd, label } : cmd;
}

async function invokeScalar(field: string, value: number): Promise<ConfigSnapshot> {
  return invoke<ConfigSnapshot>("config_set_scalar", {
    params: { field, value },
  });
}

async function invokeString(field: string, value: string): Promise<ConfigSnapshot> {
  return invoke<ConfigSnapshot>("config_set_string", {
    params: { field, value },
  });
}

async function invokeArrayPatch(
  field: string,
  updates: { index: number; value: number }[],
): Promise<ConfigSnapshot> {
  return invoke<ConfigSnapshot>("config_set_array_values", {
    params: { field, updates },
  });
}

export async function applyConfigCommand(
  cmd: ConfigCommand,
  direction: "forward" | "inverse",
): Promise<ConfigSnapshot | null> {
  switch (cmd.type) {
    case "scalar": {
      const value = direction === "forward" ? cmd.newValue : cmd.oldValue;
      return invokeScalar(cmd.field, value);
    }
    case "string": {
      const value = direction === "forward" ? cmd.newValue : cmd.oldValue;
      return invokeString(cmd.field, value);
    }
    case "array": {
      const updates = cmd.updates.map((u) => ({
        index: u.index,
        value: direction === "forward" ? u.newValue : u.oldValue,
      }));
      if (!updates.length) return null;
      return invokeArrayPatch(cmd.field, updates);
    }
    default:
      return null;
  }
}

export async function executeConfigCommand(
  cmd: ConfigCommand,
  applySnapshot?: (snap: ConfigSnapshot) => void,
): Promise<void> {
  if (isConfigMergeBlocking()) {
    throw new Error("Редактирование заблокировано — завершите merge конфига");
  }

  const labeled = withLabel(cmd);
  const snap = await applyConfigCommand(labeled, "forward");
  if (snap && applySnapshot) {
    applySnapshot(snap);
  }

  if (!applyingHistory) {
    undoStack.push(labeled);
    if (undoStack.length > MAX_HISTORY) {
      undoStack.shift();
    }
    redoStack.length = 0;
    bumpRevision();
  }
}

export async function recordConfigCommand(
  cmd: ConfigCommand,
): Promise<void> {
  if (applyingHistory || isConfigMergeBlocking()) return;

  const hasChange =
    cmd.type === "array"
      ? cmd.updates.length > 0
      : cmd.type === "scalar"
        ? cmd.oldValue !== cmd.newValue
        : cmd.oldValue !== cmd.newValue;

  if (!hasChange) return;

  const labeled = withLabel(cmd);
  undoStack.push(labeled);
  if (undoStack.length > MAX_HISTORY) {
    undoStack.shift();
  }
  redoStack.length = 0;
  bumpRevision();
}

function notifyConfigHistoryChanged(): void {
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent("config-undo-redo"));
  }
}

export async function undoConfigChange(
  applySnapshot?: (snap: ConfigSnapshot) => void,
): Promise<boolean> {
  if (isConfigMergeBlocking() || undoStack.length === 0) return false;

  const cmd = undoStack.pop()!;
  applyingHistory = true;
  try {
    const snap = await applyConfigCommand(cmd, "inverse");
    if (snap && applySnapshot) {
      applySnapshot(snap);
    }
    redoStack.push(cmd);
    bumpRevision();
    notifyConfigHistoryChanged();
    return true;
  } finally {
    applyingHistory = false;
  }
}

export async function redoConfigChange(
  applySnapshot?: (snap: ConfigSnapshot) => void,
): Promise<boolean> {
  if (isConfigMergeBlocking() || redoStack.length === 0) return false;

  const cmd = redoStack.pop()!;
  applyingHistory = true;
  try {
    const snap = await applyConfigCommand(cmd, "forward");
    if (snap && applySnapshot) {
      applySnapshot(snap);
    }
    undoStack.push(cmd);
    bumpRevision();
    notifyConfigHistoryChanged();
    return true;
  } finally {
    applyingHistory = false;
  }
}

export function configTableActionMayWrite(
  action: string,
  payload: Record<string, unknown>,
): boolean {
  switch (action) {
    case "interpolate":
    case "commit_cell":
    case "set_selection_value":
      return true;
    case "type_key":
      return true;
    case "keydown": {
      const ctrl = Boolean(payload.ctrl);
      const key = String(payload.key ?? "");
      return ctrl && (key === "ArrowUp" || key === "ArrowDown");
    }
    default:
      return false;
  }
}

export function configTableCellValueMap(
  state: Record<string, unknown>,
): Map<string, number> {
  const grid = state.grid as { cells?: { row: number; col: number; value: number }[] } | undefined;
  const map = new Map<string, number>();
  for (const cell of grid?.cells ?? []) {
    map.set(`${cell.row},${cell.col}`, cell.value);
  }
  return map;
}

export function configTableLinearIndex(
  rows: number,
  cols: number,
  row: number,
  col: number,
  yReversed = true,
): number {
  const storageRow = yReversed ? Math.max(0, rows - 1 - row) : row;
  return storageRow * cols + col;
}

export function diffConfigTableCells(
  before: Map<string, number>,
  after: Map<string, number>,
  rows: number,
  cols: number,
): ArrayCellUpdate[] {
  const updates: ArrayCellUpdate[] = [];
  for (const [key, newValue] of after) {
    const oldValue = before.get(key);
    if (oldValue === undefined) continue;
    if (Math.abs(oldValue - newValue) < 1e-9) continue;
    const [row, col] = key.split(",").map(Number);
    updates.push({
      index: configTableLinearIndex(rows, cols, row!, col!),
      oldValue,
      newValue,
    });
  }
  return updates;
}

export async function initConfigCommandHistory(): Promise<void> {
  if (initPromise) return initPromise;

  const unlisteners: UnlistenFn[] = [];
  initPromise = (async () => {
    unlisteners.push(
      await listen("workspace-reset", () => {
        clearConfigCommandHistory();
      }),
    );
  })();

  return initPromise;
}
