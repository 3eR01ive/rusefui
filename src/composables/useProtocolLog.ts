import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { onMounted, onUnmounted, ref } from "vue";

export type LogLevel = "error" | "warn" | "info" | "debug" | "trace";

export type ProtocolLogSource =
  | "command"
  | "output"
  | "trigger"
  | "spectrogram"
  | "config";

export interface ProtocolLogFilterSettings {
  error: boolean;
  warn: boolean;
  info: boolean;
  debug: boolean;
  trace: boolean;
  commands: boolean;
  output: boolean;
  trigger: boolean;
  spectrogram: boolean;
  config: boolean;
}

export interface ProtocolLogEntry {
  id: number;
  timestampMs: number;
  level: LogLevel;
  source: ProtocolLogSource;
  direction: "tx" | "rx" | "err" | "info" | "link";
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
  commands: true,
  output: false,
  trigger: false,
  spectrogram: false,
  config: false,
};

const DATA_STREAM_SOURCES = new Set<ProtocolLogSource>([
  "output",
  "trigger",
  "spectrogram",
  "config",
]);

function isDataStreamSource(source: ProtocolLogSource): boolean {
  return DATA_STREAM_SOURCES.has(source);
}

function allowsSource(
  source: ProtocolLogSource,
  f: ProtocolLogFilterSettings,
): boolean {
  switch (source) {
    case "command":
      return f.commands;
    case "output":
      return f.output;
    case "trigger":
      return f.trigger;
    case "spectrogram":
      return f.spectrogram;
    case "config":
      return f.config;
    default:
      return false;
  }
}

function allowsUi(entry: ProtocolLogEntry, f: ProtocolLogFilterSettings): boolean {
  if (!allowsSource(entry.source, f)) {
    return false;
  }
  if (isDataStreamSource(entry.source)) {
    return true;
  }
  return f[entry.level];
}

const entries = ref<ProtocolLogEntry[]>([]);
const logPath = ref("");
const filters = ref<ProtocolLogFilterSettings>({ ...DEFAULT_LOG_FILTERS });
const open = ref(false);
let unlisten: UnlistenFn | null = null;
let loaded = false;

export function sourceLabel(source: ProtocolLogSource): string {
  switch (source) {
    case "command":
      return "CMD";
    case "output":
      return "OUT";
    case "trigger":
      return "TRG";
    case "spectrogram":
      return "SPG";
    case "config":
      return "CFG";
    default:
      return source;
  }
}

export function useProtocolLog() {
  async function load(limit = 200) {
    const info = await invoke<ProtocolLogInfo>("protocol_log_get", { limit });
    logPath.value = info.path;
    filters.value = { ...DEFAULT_LOG_FILTERS, ...info.filters };
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
    const next = entries.value;
    if (next.length >= 500) {
      entries.value = [...next.slice(-499), entry];
    } else {
      entries.value = [...next, entry];
    }
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
