import type { ComponentViewState } from "./useRustComponent";
import {
  configTableActionMayWrite,
  configTableCellValueMap,
  diffConfigTableAxis,
  diffConfigTableCells,
  recordConfigCommand,
} from "./configCommands";

export async function dispatchConfigTableWithHistory(
  dispatch: (action: string, payload?: Record<string, unknown>) => Promise<void>,
  getState: () => ComponentViewState,
  zField: string,
  title: string,
  action: string,
  payload: Record<string, unknown> = {},
  axisFields?: { xField?: string; yField?: string },
): Promise<void> {
  const track = configTableActionMayWrite(action, payload);
  const stateBefore = track ? getState() : null;
  const focusBefore = editFocusFromState(stateBefore ?? {});
  const beforeGrid = stateBefore ? configTableCellValueMap(stateBefore) : null;
  const beforeX = stateBefore ? axisValuesFromState(stateBefore, "x") : null;
  const beforeY = stateBefore ? axisValuesFromState(stateBefore, "y") : null;
  const gridBefore = stateBefore?.grid as { rows?: number; cols?: number } | undefined;

  await dispatch(action, payload);

  if (!track || !stateBefore) return;

  const stateAfter = getState();

  if (focusBefore === "x" && axisFields?.xField && beforeX) {
    const afterX = axisValuesFromState(stateAfter, "x");
    const updates = diffConfigTableAxis(beforeX, afterX);
    if (updates.length) {
      await recordConfigCommand({
        type: "array",
        field: axisFields.xField,
        updates,
        label: title ? `ось X · ${title}` : axisFields.xField,
      });
      return;
    }
  }

  if (focusBefore === "y" && axisFields?.yField && beforeY) {
    const afterY = axisValuesFromState(stateAfter, "y");
    const updates = diffConfigTableAxis(beforeY, afterY);
    if (updates.length) {
      await recordConfigCommand({
        type: "array",
        field: axisFields.yField,
        updates,
        label: title ? `ось Y · ${title}` : axisFields.yField,
      });
      return;
    }
  }

  if (!beforeGrid || !zField) return;

  const after = configTableCellValueMap(stateAfter);
  const grid = stateAfter.grid as { rows?: number; cols?: number } | undefined;
  const rows = grid?.rows ?? gridBefore?.rows ?? 0;
  const cols = grid?.cols ?? gridBefore?.cols ?? 0;
  if (!rows || !cols) return;

  const updates = diffConfigTableCells(beforeGrid, after, rows, cols);
  if (!updates.length) return;

  await recordConfigCommand({
    type: "array",
    field: zField,
    updates,
    label: title ? `таблица ${title}` : zField,
  });
}

function editFocusFromState(state: ComponentViewState): "grid" | "x" | "y" {
  const raw = state.editFocus;
  if (raw === "grid" || raw === "x" || raw === "y") return raw;
  const s = String(raw ?? "grid").toLowerCase();
  if (s === "x") return "x";
  if (s === "y") return "y";
  return "grid";
}

function axisValuesFromState(
  state: ComponentViewState,
  axis: "x" | "y",
): number[] {
  const bar = state[`${axis}Axis`] as { cells?: { value: number }[] } | undefined;
  if (bar?.cells?.length) {
    return bar.cells.map((c) => c.value);
  }
  const key = axis === "x" ? "xValues" : "yValues";
  const raw = state[key];
  return Array.isArray(raw) ? raw.map(Number) : [];
}
