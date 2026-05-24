import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { onMounted, onUnmounted, ref } from "vue";

export type LogLevel = "error" | "warn" | "info" | "debug" | "trace";

export interface ProtocolLogFilterSettings {
  error: boolean;
  warn: boolean;
  info: boolean;
  debug: boolean;
  trace: boolean;
}

export interface ProtocolLogEntry {
  id: number;
  timestampMs: number;
  level: LogLevel;
  direction: "tx" | "rx" | "err" | "info";
  command: string | null;
  summary: string;
  payloadHex: string;
  frameHex: string;
  responseCode: number | null;
}

export interface ProtocolLogInfo {
  path: string;
  entries: ProtocolLogEntry[];
  filters: ProtocolLogFilterSettings;
}

export const DEFAULT_LOG_FILTERS: ProtocolLogFilterSettings = {
  error: true,
  warn: true,
  info: true,
  debug: false,
  trace: false,
};

const entries = ref<ProtocolLogEntry[]>([]);
const logPath = ref("");
const filters = ref<ProtocolLogFilterSettings>({ ...DEFAULT_LOG_FILTERS });
const open = ref(false);
let unlisten: UnlistenFn | null = null;
let loaded = false;

function allowsUi(entry: ProtocolLogEntry, f: ProtocolLogFilterSettings): boolean {
  if (entry.level === "trace") return false;
  return f[entry.level];
}

export function useProtocolLog() {
  async function load(limit = 200) {
    const info = await invoke<ProtocolLogInfo>("protocol_log_get", { limit });
    logPath.value = info.path;
    filters.value = info.filters;
    entries.value = info.entries;
    loaded = true;
  }

  async function setFilters(next: ProtocolLogFilterSettings) {
    await invoke("protocol_log_set_filters", { filters: next });
    filters.value = { ...next };
    const info = await invoke<ProtocolLogInfo>("protocol_log_get", { limit: 500 });
    entries.value = info.entries;
  }

  async function clear() {
    await invoke("protocol_log_clear");
    entries.value = [];
  }

  function togglePanel() {
    open.value = !open.value;
    if (open.value && !loaded) {
      void load();
    }
  }

  function formatTime(ms: number): string {
    const d = new Date(ms);
    const base = d.toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
    const frac = String(d.getMilliseconds()).padStart(3, "0");
    return `${base}.${frac}`;
  }

  return {
    entries,
    logPath,
    filters,
    open,
    load,
    setFilters,
    clear,
    togglePanel,
    formatTime,
  };
}

export async function initProtocolLogListener(): Promise<void> {
  if (unlisten) return;
  unlisten = await listen<ProtocolLogEntry>("protocol-log", (event) => {
    const entry = event.payload;
    if (!allowsUi(entry, filters.value)) return;
    entries.value = [...entries.value, entry].slice(-500);
  });
}

export function teardownProtocolLogListener(): void {
  unlisten?.();
  unlisten = null;
}

export function useProtocolLogLifecycle() {
  onMounted(() => {
    void initProtocolLogListener();
  });
  onUnmounted(() => {
    teardownProtocolLogListener();
  });
}
