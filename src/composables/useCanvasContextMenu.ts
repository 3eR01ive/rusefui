import { ref, onMounted, onBeforeUnmount } from "vue";
import type { Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { ComponentInstance, DataBinding, ComponentMeta } from "../core/types";
import { snapGrid } from "./useCanvasLayout";
import { listRegisteredComponents } from "../core/registry";

// ── Public types ───────────────────────────────────────────────

export interface TableEntry {
  id: string; title: string;
  zBins: string; xBins?: string; yBins?: string;
}
export interface CurveEntry {
  id: string; title: string;
  xBins: string; yBins: string;
}
export interface ConfigFieldEntry { name: string; units?: string; ty: string; }
export interface OutputFieldEntry { name: string; units?: string; kind: string; }

export type CtxStage = "types" | "table" | "curve" | "field" | "output-field";

export interface CtxState {
  menuX: number;
  menuY: number;
  /** Canvas-координаты точки добавления (не нужны в режиме редактирования). */
  canvasX: number;
  canvasY: number;
  stage: CtxStage;
  selectedType: string | null;
  /** Если задан — меняем bind существующего extra-инстанса, не добавляем новый. */
  editKey: string | null;
  tables: TableEntry[];
  curves: CurveEntry[];
  configFields: ConfigFieldEntry[];
  outputFields: OutputFieldEntry[];
  loading: boolean;
}

export interface CanvasItem {
  key: string;
  isExtra: boolean;
  child: ComponentInstance;
}

// ── Константы ──────────────────────────────────────────────────

const EXCLUDE_FROM_MENU = new Set(["stack", "row", "section", "composite", "canvas"]);

const CONFIG_FIELD_TYPE: Record<string, string> = {
  "scalar-field": "scalar",
  "string-field": "string",
  "enum-field": "enum",
};

export function listMenuTypes(): ComponentMeta[] {
  return listRegisteredComponents().filter((m) => !EXCLUDE_FROM_MENU.has(m.type));
}

// ── Composable ─────────────────────────────────────────────────

export function useCanvasContextMenu(deps: {
  menuTypes: ComponentMeta[];
  addExtraInstance: (inst: ComponentInstance, layout?: { x?: number; y?: number }) => string;
  updateExtraInstanceBind: (id: string, bind: DataBinding | undefined) => void;
  editMode: Ref<boolean>;
  containerRef: Ref<HTMLElement | null>;
}) {
  const { menuTypes, addExtraInstance, updateExtraInstanceBind, editMode, containerRef } = deps;

  const ctx = ref<CtxState | null>(null);

  function makeEmptyCtx(menuX: number, menuY: number, canvasX: number, canvasY: number): CtxState {
    return {
      menuX, menuY, canvasX, canvasY,
      stage: "types", selectedType: null, editKey: null,
      tables: [], curves: [], configFields: [], outputFields: [],
      loading: false,
    };
  }

  // ── Canvas background right-click ──────────────────────────────
  function onCanvasContextMenu(e: MouseEvent): void {
    if (!editMode.value) return;
    if ((e.target as HTMLElement) !== (e.currentTarget as HTMLElement)) return;
    e.preventDefault();

    const cr = containerRef.value?.getBoundingClientRect();
    if (!cr) return;
    const scrollTop = containerRef.value?.scrollTop ?? 0;

    const menuW = 260;
    const menuH = Math.min(460, menuTypes.length * 32 + 48);
    const x = e.clientX + menuW > window.innerWidth ? e.clientX - menuW : e.clientX;
    const y = e.clientY + menuH > window.innerHeight ? e.clientY - menuH : e.clientY;

    ctx.value = makeEmptyCtx(
      x, y,
      snapGrid(Math.max(0, e.clientX - cr.left)),
      snapGrid(Math.max(0, e.clientY - cr.top + scrollTop)),
    );
  }

  // ── Component right-click (edit bind) ─────────────────────────
  async function onComponentContextMenu(key: string, item: CanvasItem, e: MouseEvent): Promise<void> {
    e.stopPropagation();
    if (!item.isExtra) return;
    const bm = menuTypes.find((m) => m.type === item.child.type)?.bindMeta;
    if (!bm?.needsTable && !bm?.needsCurve && !bm?.needsConfigField && !bm?.needsOutputField) return;

    const menuW = 260;
    const menuH = 420;
    const x = e.clientX + menuW > window.innerWidth ? e.clientX - menuW : e.clientX;
    const y = e.clientY + menuH > window.innerHeight ? e.clientY - menuH : e.clientY;

    ctx.value = { ...makeEmptyCtx(x, y, 0, 0), selectedType: item.child.type, editKey: key };
    await _loadStage(item.child.type);
  }

  // ── Staged logic ───────────────────────────────────────────────
  async function onSelectType(type: string): Promise<void> {
    if (!ctx.value) return;
    const bm = menuTypes.find((m) => m.type === type)?.bindMeta;
    const needsPicker = bm?.needsTable || bm?.needsCurve || bm?.needsConfigField || bm?.needsOutputField;

    if (!needsPicker) {
      if (!ctx.value.editKey) {
        const bind: DataBinding | undefined = bm?.autoSource ? { source: bm.autoSource } : undefined;
        addExtraInstance({ type, bind }, { x: ctx.value.canvasX, y: ctx.value.canvasY });
      }
      ctx.value = null;
      return;
    }

    ctx.value.selectedType = type;
    await _loadStage(type);
  }

  async function _loadStage(type: string): Promise<void> {
    if (!ctx.value) return;
    const bm = menuTypes.find((m) => m.type === type)?.bindMeta;

    if (bm?.needsTable) {
      ctx.value.loading = true;
      ctx.value.stage = "table";
      try {
        ctx.value.tables = await invoke<TableEntry[]>("ini_list_tables");
      } finally {
        ctx.value.loading = false;
      }
      return;
    }

    if (bm?.needsCurve) {
      ctx.value.loading = true;
      ctx.value.stage = "curve";
      try {
        ctx.value.curves = await invoke<CurveEntry[]>("ini_list_curves");
      } finally {
        ctx.value.loading = false;
      }
      return;
    }

    if (bm?.needsConfigField) {
      ctx.value.loading = true;
      ctx.value.stage = "field";
      try {
        type RawField = { name: string; units?: string; ty: string };
        const all = await invoke<RawField[]>("config_list_fields");
        const needTy = CONFIG_FIELD_TYPE[type] ?? "";
        ctx.value.configFields = needTy ? all.filter((f) => f.ty === needTy) : all;
      } finally {
        ctx.value.loading = false;
      }
      return;
    }

    if (bm?.needsOutputField) {
      ctx.value.loading = true;
      ctx.value.stage = "output-field";
      try {
        ctx.value.outputFields = await invoke<OutputFieldEntry[]>("output_list_fields");
      } finally {
        ctx.value.loading = false;
      }
    }
  }

  function _applyBind(bind: DataBinding): void {
    if (!ctx.value?.selectedType) return;
    if (ctx.value.editKey) {
      updateExtraInstanceBind(ctx.value.editKey, bind);
    } else {
      addExtraInstance(
        { type: ctx.value.selectedType, bind },
        { x: ctx.value.canvasX, y: ctx.value.canvasY },
      );
    }
    ctx.value = null;
  }

  function onSelectTable(t: TableEntry): void {
    _applyBind({ source: "config", params: { zBins: t.zBins, xBins: t.xBins, yBins: t.yBins } });
  }

  function onSelectCurve(c: CurveEntry): void {
    _applyBind({ source: "config", params: { xBins: c.xBins, yBins: c.yBins } });
  }

  function onSelectConfigField(name: string): void {
    const bm = menuTypes.find((m) => m.type === ctx.value?.selectedType)?.bindMeta;
    _applyBind({ source: bm?.autoSource ?? "config", field: name });
  }

  function onSelectOutputField(name: string): void {
    const bm = menuTypes.find((m) => m.type === ctx.value?.selectedType)?.bindMeta;
    _applyBind({ source: bm?.autoSource ?? "outputChannels", field: name });
  }

  function ctxBack(): void {
    if (!ctx.value) return;
    if (ctx.value.editKey) { ctx.value = null; return; }
    ctx.value.stage = "types";
    ctx.value.selectedType = null;
  }

  // ── Document listeners ─────────────────────────────────────────
  function onDocPointerDown(e: PointerEvent): void {
    if (!ctx.value) return;
    const menu = document.querySelector(".ccm-menu");
    if (menu && menu.contains(e.target as Node)) return;
    ctx.value = null;
  }

  function onDocKeydown(e: KeyboardEvent): void {
    if (!ctx.value) return;
    if (e.key === "Escape") {
      if (ctx.value.stage !== "types" || ctx.value.editKey) ctxBack();
      else ctx.value = null;
    }
  }

  onMounted(() => {
    document.addEventListener("pointerdown", onDocPointerDown, true);
    document.addEventListener("keydown", onDocKeydown, true);
  });
  onBeforeUnmount(() => {
    document.removeEventListener("pointerdown", onDocPointerDown, true);
    document.removeEventListener("keydown", onDocKeydown, true);
  });

  return {
    ctx,
    onCanvasContextMenu,
    onComponentContextMenu,
    onSelectType,
    onSelectTable,
    onSelectCurve,
    onSelectConfigField,
    onSelectOutputField,
    ctxBack,
  };
}
