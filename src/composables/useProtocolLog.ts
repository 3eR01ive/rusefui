import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { onMounted, onUnmounted, ref } from "vue";

export interface ProtocolLogEntry {
  id: number;
  timestampMs: number;
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
}

const entries = ref<ProtocolLogEntry[]>([]);
const logPath = ref("");
const open = ref(false);
let unlisten: UnlistenFn | null = null;
let loaded = false;

export function useProtocolLog() {
  async function load(limit = 200) {
    const info = await invoke<ProtocolLogInfo>("protocol_log_get", { limit });
    logPath.value = info.path;
    entries.value = info.entries;
    loaded = true;
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
    return d.toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      fractionalSecondDigits: 3,
    });
  }

  return {
    entries,
    logPath,
    open,
    load,
    clear,
    togglePanel,
    formatTime,
  };
}

export async function initProtocolLogListener(): Promise<void> {
  if (unlisten) return;
  unlisten = await listen<ProtocolLogEntry>("protocol-log", (event) => {
    entries.value = [...entries.value, event.payload].slice(-500);
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
