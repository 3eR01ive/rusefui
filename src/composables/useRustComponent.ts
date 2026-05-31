import { onMounted, onUnmounted, ref, shallowRef, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ComponentInstance } from "../core/types";
import { requiresRustLogic } from "../core/rust-logic";

export type ComponentViewState = Record<string, unknown>;

function resolveInstanceId(instance: ComponentInstance, path: string): string {
  // config-table / ignition-table: id в YAML повторяется — уникальность по path.
  if (instance.type === "config-table" || instance.type === "ignition-table") {
    return path.replace(/\//g, "-");
  }
  return instance.id ?? path.replace(/\//g, "-");
}

/**
 * Подписка на состояние компонента из Rust. Vue только рисует `state` и шлёт `dispatch`.
 */
export function useRustComponent(
  instance: ComponentInstance,
  path: string,
  mountPayload?: () =>
    | Record<string, unknown>
    | undefined
    | Promise<Record<string, unknown> | undefined>,
) {
  const instanceId = resolveInstanceId(instance, path);
  const state = shallowRef<ComponentViewState>({});
  const ready = ref(false);
  const mounting = ref(false);
  const error = ref<string | null>(null);

  let unlisten: UnlistenFn | null = null;
  let mounted = false;

  const hasLogic = computed(() => requiresRustLogic(instance.type));

  async function mountLogic(): Promise<void> {
    if (!hasLogic.value || mounted || mounting.value) return;
    mounting.value = true;
    try {
      if (!unlisten) {
        unlisten = await listen<{ instance_id: string; state: ComponentViewState }>(
          "component-state",
          (event) => {
            if (event.payload.instance_id === instanceId) {
              state.value = event.payload.state;
            }
          },
        );
      }
      const payload = await Promise.resolve(mountPayload?.() ?? {});
      const snapshot = await invoke<ComponentViewState>("component_mount", {
        params: {
          instance_id: instanceId,
          component_type: instance.type,
          payload: payload ?? {},
        },
      });
      state.value = snapshot;
      ready.value = true;
      mounted = true;
      error.value = null;
    } catch (e) {
      error.value = String(e);
    } finally {
      mounting.value = false;
    }
  }

  async function dispatch(
    action: string,
    payload: Record<string, unknown> = {},
  ): Promise<ComponentViewState | undefined> {
    if (!hasLogic.value || !ready.value) return undefined;
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
      return next;
    } catch (e) {
      error.value = String(e);
      throw e;
    }
  }

  onMounted(() => {
    if (!hasLogic.value) return;
    requestAnimationFrame(() => {
      void mountLogic();
    });
  });

  onUnmounted(() => {
    unlisten?.();
    if (hasLogic.value && mounted) {
      invoke("component_unmount", { instance_id: instanceId }).catch(() => {});
    }
  });

  return {
    instanceId,
    state,
    ready,
    mounting,
    error,
    dispatch,
    hasLogic,
  };
}
