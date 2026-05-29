import type { Component } from "vue";

/** Режим компонента: правка flash-конфига или только отображение. */
export type ComponentMode = "edit" | "display";

/** Источник данных (Rust `sources/*`) или logic-компонент (`connection`, `dyno`, …). */
export type DataSourceId =
  | "connection"
  | "config"
  | "outputChannels"
  | "textLog"
  | "knockScope"
  | "compositeLogger";

export interface DataBinding {
  /** `config`, `outputChannels`, `knockScope`, … */
  source: DataSourceId | string;
  /** Одно поле INI / один канал outputChannels. */
  field?: string;
  /** Несколько каналов (график, список). */
  fields?: string[];
  /** Доп. имена: `xBins`/`yBins`/`zBins`, `rpmField`, … */
  params?: Record<string, unknown>;
}

/** Инстанс компонента в дереве layout (из YAML). */
export interface ComponentInstance {
  /** Уникальный id инстанса на вкладке. */
  id?: string;
  /** Зарегистрированный тип из кода. */
  type: string;
  props?: Record<string, unknown>;
  bind?: DataBinding;
  /** Вложенные инстансы (для контейнеров и composite-файлов). */
  children?: ComponentInstance[];
  /** false — не участвует в навигации стрелками (YAML). */
  navSelectable?: boolean;
  /** false — только выбор, без Enter/active (YAML). */
  navActivatable?: boolean;
}

/**
 * Файл определения составного компонента (`config/components/*.yaml`).
 * Сам по себе не регистрируется в коде — это дерево инстансов.
 */
export interface ComponentDefinitionFile {
  id: string;
  description?: string;
  children: ComponentInstance[];
}

/** Файл вкладки (`config/tabs/*.tab.yaml`). */
export interface TabDefinitionFile {
  tab: {
    id: string;
    title: string;
  };
  /** Корень: inline-дерево или ссылка на components/*.yaml */
  root: ComponentInstance | ComponentRef;
}

export interface ComponentRef {
  $component: string;
}

export function isComponentRef(v: unknown): v is ComponentRef {
  return (
    typeof v === "object" &&
    v !== null &&
    "$component" in v &&
    typeof (v as ComponentRef).$component === "string"
  );
}

/** Корневой конфиг приложения (`config/app.yaml`). */
export interface AppConfigFile {
  app: {
    title?: string;
  };
  tabs: TabRef[];
}

export interface TabRef {
  $tab: string;
}

export interface ResolvedTab {
  id: string;
  title: string;
  root: ComponentInstance;
}

export interface ComponentMeta {
  type: string;
  label: string;
  mode: ComponentMode;
  /** Может содержать дочерние инстансы (есть `children` в YAML). */
  isContainer: boolean;
  description?: string;
}

export interface RegisteredComponent {
  meta: ComponentMeta;
  component: Component;
}
