import { computed, readonly, shallowRef } from "vue";
import { clearConfigCommandHistory } from "./configCommands";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type DiffSide = "project" | "ecu";

export interface ConfigDiffEntry {
  field: string;
  ty: string;
  project: number;
  ecu: number;
}

export interface ConfigDiffSnapshot {
  active: boolean;
  entries: ConfigDiffEntry[];
  choices: Record<string, DiffSide>;
}

const snapshot = shallowRef<ConfigDiffSnapshot>({
  active: false,
  entries: [],
  choices: {},
});

const diffFields = computed(() => new Set(snapshot.value.entries.map((e) => e.field)));

let initPromise: Promise<void> | null = null;
let unlisten: UnlistenFn | null = null;

export async function initConfigDiff(): Promise<void> {
  if (initPromise) return initPromise;
  initPromise = (async () => {
    try {
      snapshot.value = await invoke<ConfigDiffSnapshot>("config_diff_get");
    } catch {
      /* browser */
    }
    if (!unlisten) {
      unlisten = await listen<ConfigDiffSnapshot>("config-diff", (ev) => {
        snapshot.value = ev.payload;
      });
    }
  })();
  return initPromise;
}

/** Пока true — UI заблокирован до merge (см. ConfigDiffModal). */
export function isConfigMergeBlocking(): boolean {
  return snapshot.value.active;
}

export function useConfigDiff() {
  const active = computed(() => snapshot.value.active);
  const count = computed(() => snapshot.value.entries.length);

  function entryFor(field: string): ConfigDiffEntry | null {
    if (!snapshot.value.active) return null;
    return snapshot.value.entries.find((e) => e.field === field) ?? null;
  }

  function choiceFor(field: string): DiffSide {
    return snapshot.value.choices[field] ?? "ecu";
  }

  function isDiffField(field: string): boolean {
    return diffFields.value.has(field);
  }

  async function setChoice(field: string, side: DiffSide): Promise<void> {
    snapshot.value = await invoke<ConfigDiffSnapshot>("config_diff_set_choice", {
      field,
      side,
    });
  }

  async function setAll(side: DiffSide): Promise<void> {
    snapshot.value = await invoke<ConfigDiffSnapshot>("config_diff_set_all", { side });
  }

  async function apply(): Promise<void> {
    await invoke("config_diff_apply");
    snapshot.value = await invoke<ConfigDiffSnapshot>("config_diff_get");
    clearConfigCommandHistory();
  }

  async function dismiss(): Promise<void> {
    await invoke("config_diff_dismiss");
    snapshot.value = { active: false, entries: [], choices: {} };
    clearConfigCommandHistory();
  }

  return {
    snapshot: readonly(snapshot),
    active,
    count,
    entryFor,
    choiceFor,
    isDiffField,
    setChoice,
    setAll,
    apply,
    dismiss,
  };
}

export function formatDiffValue(v: number, ty: string): string {
  if (!Number.isFinite(v)) return "—";
  if (ty === "enum" || Number.isInteger(v)) return String(Math.round(v));
  const s = v.toFixed(4);
  return s.replace(/\.?0+$/, "");
}
