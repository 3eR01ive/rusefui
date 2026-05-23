import { inject, provide, type InjectionKey, ref, type Ref } from "vue";
import type { DataBinding, DataSourceId } from "./types";
import type { ConnectionStatus } from "../types/connection";

export interface DataContextState {
  connection: Ref<ConnectionStatus>;
}

const DataContextKey: InjectionKey<DataContextState> = Symbol("rusefui-data");

export function provideDataContext(state: DataContextState): void {
  provide(DataContextKey, state);
}

export function useDataContext(): DataContextState {
  const ctx = inject(DataContextKey);
  if (!ctx) {
    throw new Error("DataContext is not available");
  }
  return ctx;
}

export function createDataContext(): DataContextState {
  return {
    connection: ref<ConnectionStatus>({ connected: false }),
  };
}

/**
 * Разрешает декларативную привязку из YAML в runtime-значение.
 * По мере роста проекта сюда добавляются config / outputChannels / logs.
 */
export function resolveBinding(bind: DataBinding | undefined): unknown {
  if (!bind) return undefined;

  const ctx = useDataContext();

  switch (bind.source as DataSourceId) {
    case "connection":
      return ctx.connection.value;
    case "config":
      return { field: bind.field, params: bind.params, _placeholder: true };
    case "outputChannels":
      return { field: bind.field, params: bind.params, _placeholder: true };
    case "textLog":
      return { _placeholder: true };
    default:
      console.warn(`[data] unknown source: ${bind.source}`);
      return undefined;
  }
}
