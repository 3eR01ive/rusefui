import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DataContextState } from "../core/data-context";
import type { ConnectionStatus } from "../types/connection";

interface EcuConnectionEvent {
  connected: boolean;
  offlineMode: boolean;
  portName?: string | null;
  baudRate?: number | null;
  signature?: string | null;
  handshakeCommand?: string | null;
  lastError?: string | null;
}

function toConnectionStatus(event: EcuConnectionEvent): ConnectionStatus {
  return {
    connected: event.connected,
    port_name: event.portName ?? null,
    baud_rate: event.baudRate ?? null,
    signature: event.signature ?? null,
    handshake_command: event.handshakeCommand ?? null,
    last_error: event.lastError ?? null,
  };
}

/**
 * Глобальное состояние ECU: автоподключение, offline mode, события `ecu-connection`.
 */
export function useEcuConnection(dataCtx: DataContextState) {
  const offlineMode = ref(false);
  const scanning = ref(false);
  const busyPorts = ref<string[]>([]);
  let unlisten: UnlistenFn | null = null;

  function applyEvent(event: EcuConnectionEvent) {
    offlineMode.value = event.offlineMode;
    dataCtx.offlineMode.value = event.offlineMode;
    dataCtx.connection.value = toConnectionStatus(event);
  }

  async function setOfflineMode(next: boolean) {
    await invoke("autoconnect_set_offline_mode", { offline: next });
    offlineMode.value = next;
    dataCtx.offlineMode.value = next;
    if (next) {
      dataCtx.connection.value = {
        ...dataCtx.connection.value,
        connected: false,
      };
    }
  }

  onMounted(async () => {
    try {
      const snap = await invoke<{
        offlineMode: boolean;
        scanning: boolean;
        busyPorts: string[];
      }>("autoconnect_get_state");
      offlineMode.value = snap.offlineMode;
      dataCtx.offlineMode.value = snap.offlineMode;
      scanning.value = snap.scanning;
      busyPorts.value = snap.busyPorts ?? [];
    } catch {
      // не Tauri (dev в браузере)
    }

    unlisten = await listen<EcuConnectionEvent>("ecu-connection", (event) => {
      applyEvent(event.payload);
    });

    await listen<{ scanning: boolean; busyPorts: string[] }>("autoconnect-state", (event) => {
      scanning.value = event.payload.scanning;
      busyPorts.value = event.payload.busyPorts ?? [];
    });
  });

  onUnmounted(() => {
    unlisten?.();
  });

  return {
    offlineMode,
    scanning,
    busyPorts,
    setOfflineMode,
  };
}
