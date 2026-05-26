import { onMounted, onUnmounted, ref, shallowRef, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ComponentInstance } from "../core/types";
import { requiresRustLogic } from "../core/rust-logic";

export type ComponentViewState = Record<string, unknown>;

function resolveInstanceId(instance: ComponentInstance, path: string): string {
  return instance.id ?? path.replace(/\//g, "-");
}

/**
 * Подписка на состояние компонента из Rust. Vue только рисует `state` и шлёт `dispatch`.
 */
export function useRustComponent(instance: ComponentInstance, path: string) {
  const instanceId = resolveInstanceId(instance, path);
  const state = shallowRef<ComponentViewState>({});
  const ready = ref(false);
  const error = ref<string | null>(null);

  let unlisten: UnlistenFn | null = null;

  const hasLogic = computed(() => requiresRustLogic(instance.type));

  async function dispatch(action: string, payload: Record<string, unknown> = {}) {
    if (!hasLogic.value || !ready.value) return;
    try {
      const next = await invoke<ComponentViewState>("component_dispatch", {
        params: {
          instance_id: instanceId,
          action,
          payload,
        },
      });
      state.value = next;
      error.value = null;
    } catch (e) {
      error.value = String(e);
      throw e;
    }
  }

  onMounted(async () => {
    if (!hasLogic.value) return;

    unlisten = await listen<{ instance_id: string; state: ComponentViewState }>(
      "component-state",
      (event) => {
        if (event.payload.instance_id === instanceId) {
          state.value = event.payload.state;
        }
      },
    );

    try {
      const snapshot = await invoke<ComponentViewState>("component_mount", {
        params: {
          instance_id: instanceId,
          component_type: instance.type,
        },
      });
      state.value = snapshot;
      ready.value = true;
    } catch (e) {
      error.value = String(e);
    }
  });

  onUnmounted(() => {
    unlisten?.();
    if (hasLogic.value) {
      invoke("component_unmount", { instance_id: instanceId }).catch(() => {});
    }
  });

  return {
    instanceId,
    state,
    ready,
    error,
    dispatch,
    hasLogic,
  };
}
