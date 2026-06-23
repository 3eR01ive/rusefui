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
  | "compositeLogger"
  | "engineSniffer";

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

/**
 * Подсказка позиционирования для дочернего элемента canvas-контейнера.
 * Используется как default-позиция до первого сохранения layout.
 */
export interface ComponentLayoutHint {
  x?: number;
  y?: number;
  w?: number;
  h?: number;
  minW?: number;
  minH?: number;
  /** Запрет перемещения в edit-mode. */
  locked?: boolean;
  /** Может перекрываться с другими (парит поверх, не участвует в overlap resolution). */
  floating?: boolean;
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
  /** Позиционирование в CanvasLayout (x/y/w/h в пикселях). */
  layout?: ComponentLayoutHint;
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
  /** Таб создан пользователем (не из YAML). */
  isCustom?: boolean;
}

/** Метаданные для настройки bind при добавлении компонента на канвас. */
export interface ComponentBindMeta {
  /** Автоматически выставить bind.source (без участия пользователя). */
  autoSource?: DataSourceId;
  /** Нужно выбрать 2D-таблицу из INI (zBins/xBins/yBins). */
  needsTable?: boolean;
  /** Нужно выбрать кривую из INI (xBins/yBins). */
  needsCurve?: boolean;
  /** Нужно ввести имя поля конфига (bind.field). */
  needsConfigField?: boolean;
  /** Нужно выбрать output-канал (bind.field из outputChannels). */
  needsOutputField?: boolean;
}

export interface ComponentMeta {
  type: string;
  label: string;
  mode: ComponentMode;
  /** Может содержать дочерние инстансы (есть `children` в YAML). */
  isContainer: boolean;
  /**
   * Компонент сам рендерит своих детей (например, CanvasLayout).
   * ComponentHost не будет инжектировать children через slot.
   */
  handlesOwnChildren?: boolean;
  description?: string;
  /** Метаданные bind для канвас-пикера. */
  bindMeta?: ComponentBindMeta;
}

export interface RegisteredComponent {
  meta: ComponentMeta;
  component: Component;
}
