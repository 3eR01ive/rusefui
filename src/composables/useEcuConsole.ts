import { invoke } from "@tauri-apps/api/core";
import { onMounted, onUnmounted, ref } from "vue";

export interface EcuConsoleLine {
  id: number;
  text: string;
  ts: string;
}

let lineCounter = 0;

function nowTs(): string {
  const d = new Date();
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  const ms = String(d.getMilliseconds()).padStart(3, "0");
  return `${hh}:${mm}:${ss}.${ms}`;
}

const POLL_INTERVAL_MS = 500;
const MAX_LINES = 1000;

export function useEcuConsole() {
  const lines = ref<EcuConsoleLine[]>([]);
  let timer: ReturnType<typeof setInterval> | null = null;

  async function poll() {
    const raw = await invoke<string>("ecu_console_poll").catch(() => "");
    if (!raw) return;
    const ts = nowTs();
    const newLines: EcuConsoleLine[] = raw
      .split("\n")
      .map((s) => s.trimEnd())
      .filter((s) => s.length > 0)
      .map((text) => ({ id: lineCounter++, text, ts }));
    if (newLines.length === 0) return;
    const combined = [...lines.value, ...newLines];
    lines.value = combined.length > MAX_LINES ? combined.slice(-MAX_LINES) : combined;
  }

  function clear() {
    lines.value = [];
  }

  function start() {
    if (timer !== null) return;
    timer = setInterval(() => { void poll(); }, POLL_INTERVAL_MS);
  }

  function stop() {
    if (timer !== null) { clearInterval(timer); timer = null; }
  }

  onMounted(start);
  onUnmounted(stop);

  return { lines, clear, poll };
}
