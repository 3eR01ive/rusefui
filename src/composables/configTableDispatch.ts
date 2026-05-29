import type { ComponentViewState } from "./useRustComponent";
import {
  configTableActionMayWrite,
  configTableCellValueMap,
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
): Promise<void> {
  const track = Boolean(zField) && configTableActionMayWrite(action, payload);
  const stateBefore = track ? getState() : null;
  const before = stateBefore ? configTableCellValueMap(stateBefore) : null;
  const gridBefore = stateBefore?.grid as { rows?: number; cols?: number } | undefined;

  await dispatch(action, payload);

  if (!track || !before || !zField) return;

  const stateAfter = getState();
  const after = configTableCellValueMap(stateAfter);
  const grid = stateAfter.grid as { rows?: number; cols?: number } | undefined;
  const rows = grid?.rows ?? gridBefore?.rows ?? 0;
  const cols = grid?.cols ?? gridBefore?.cols ?? 0;
  if (!rows || !cols) return;

  const updates = diffConfigTableCells(before, after, rows, cols);
  if (!updates.length) return;

  await recordConfigCommand({
    type: "array",
    field: zField,
    updates,
    label: title ? `таблица ${title}` : zField,
  });
}
