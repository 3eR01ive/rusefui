import { inject, provide, type InjectionKey, ref, type Ref } from "vue";
import type { DataBinding, DataSourceId } from "./types";
import type { ConnectionStatus } from "../types/connection";

export interface DataContextState {
  connection: Ref<ConnectionStatus>;
  offlineMode: Ref<boolean>;
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
    offlineMode: ref(false),
  };
}

/**
 * Привязка к источнику данных. Снимки готовятся в Rust; здесь — только чтение
 * уже синхронизированного в context (например connection из connection-компонента).
 */
export function resolveBinding(bind: DataBinding | undefined): unknown {
  if (!bind) return undefined;

  switch (bind.source) {
    case "connection":
      return useDataContext().connection.value;
    case "config":
    case "outputChannels":
    case "textLog":
    case "knockScope":
    case "compositeLogger":
      return {
        source: bind.source,
        field: bind.field,
        fields: bind.fields,
        params: bind.params,
      };
    default:
      console.warn(`[data] unknown source: ${bind.source}`);
      return undefined;
  }
}
