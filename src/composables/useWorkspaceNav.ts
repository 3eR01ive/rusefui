import { ref } from "vue";
import type { ComponentInstance } from "../core/types";
import {
  collectNavPathsFromTree,
  type NavPathFlags,
} from "../core/navFlags";

export type NavMode = "select" | "active";

export interface NavExtension {
  basePath: string;
  instance: ComponentInstance;
}

/** Режим навигации по компонентам текущей вкладки. */
export const navMode = ref<NavMode>("select");
/** Синяя рамка — выбранный узел (стрелки). */
export const selectedPath = ref("");
/** Активный компонент (перехватывает клавиатуру, без отдельной подсветки). */
export const activePath = ref("");
/** Leaf-пути текущей вкладки (depth-first). */
export const navPaths = ref<string[]>([]);
/** Динамически подгружаемые поддеревья (checklist editor, INI preview…). */
export const navExtensions = ref<NavExtension[]>([]);
/** Пункты бокового меню вместо leaf-оболочки (checklist, INI browser…). */
export const navMenuPaths = ref<Map<string, string[]>>(new Map());

export type NavArrowKey = "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight";

/** Флаги nav по path (из YAML + menu/filter). */
export const navPathFlags = ref<Map<string, NavPathFlags>>(new Map());

// Пространственные позиции (центры) nav-узлов — строятся при входе на таб.
interface SpatialPos { cx: number; cy: number }
const spatialData = new Map<string, SpatialPos>();

/** Сканирует DOM-позиции всех nav-узлов и запоминает их центры. */
export function buildSpatialData(paths: readonly string[]): void {
  spatialData.clear();
  for (const path of paths) {
    const el = navNodeEl(path);
    if (!el) continue;
    const r = el.getBoundingClientRect();
    if (r.width === 0 && r.height === 0) continue;
    spatialData.set(path, { cx: r.left + r.width / 2, cy: r.top + r.height / 2 });
  }
}

export function isFilterNavPath(path: string): boolean {
  return path.endsWith("/filter");
}

/** Путь меню INI/checklist — activatable задаётся в navPathFlags. */
export function isMenuNavPath(path: string): boolean {
  return path.includes("/menu/");
}

export function isNavActivatablePath(path: string): boolean {
  const flags = navPathFlags.value.get(path);
  return flags?.activatable ?? true;
}

let lastVisualSelectedPath = "";

function escapeNavPath(path: string): string {
  return typeof CSS !== "undefined" && "escape" in CSS ? CSS.escape(path) : path;
}

function navNodeEl(path: string): HTMLElement | null {
  if (!path) return null;
  return document.querySelector<HTMLElement>(`[data-nav-path="${escapeNavPath(path)}"]`);
}

/** Подсветка выбора — напрямую в DOM, без перерисовки всего дерева Vue. */
export function syncNavSelectionVisual(nextPath: string): void {
  if (typeof document === "undefined") return;
  const prev = lastVisualSelectedPath;
  if (prev === nextPath) return;
  if (prev) navNodeEl(prev)?.classList.remove("nav-node--selected");
  if (nextPath) navNodeEl(nextPath)?.classList.add("nav-node--selected");
  lastVisualSelectedPath = nextPath;
}

export function clearNavSelectionVisual(): void {
  if (typeof document === "undefined") return;
  for (const node of document.querySelectorAll<HTMLElement>(".nav-node--selected")) {
    node.classList.remove("nav-node--selected");
  }
  lastVisualSelectedPath = "";
}

export function setNavPaths(paths: string[]): void {
  navPaths.value = paths;
}

/** Заменяет host-path leaf на список пунктов меню внутри него. */
export function setNavMenuPaths(hostPath: string, paths: readonly string[]): void {
  const next = new Map(navMenuPaths.value);
  if (paths.length) next.set(hostPath, [...paths]);
  else next.delete(hostPath);
  navMenuPaths.value = next;
}

/** Регистрация/снятие динамического поддерева для навигации стрелками. */
export function setNavExtension(basePath: string, instance: ComponentInstance | null): void {
  const next = navExtensions.value.filter((e) => e.basePath !== basePath);
  if (instance) {
    next.push({ basePath, instance });
  }
  navExtensions.value = next;
}

export function collectAllNavPaths(
  tabRoot: ComponentInstance,
  tabPath: string,
  extensions: readonly NavExtension[] = navExtensions.value,
  menuPaths: ReadonlyMap<string, string[]> = navMenuPaths.value,
): string[] {
  const paths: string[] = [];
  const flags = new Map<string, NavPathFlags>();
  collectNavPathsFromTree(tabRoot, tabPath, paths, flags);

  const menuHostsUsed = new Set<string>();
  for (const p of [...paths]) {
    const menu = menuPaths.get(p);
    if (menu?.length) {
      const idx = paths.indexOf(p);
      paths.splice(idx, 1);
      flags.delete(p);
      for (const mp of menu) {
        paths.push(mp);
        flags.set(mp, { selectable: true, activatable: false });
      }
      menuHostsUsed.add(p);
    }
  }

  for (const [hostPath, menu] of menuPaths) {
    if (!hostPath.startsWith(`${tabPath}/`) || menuHostsUsed.has(hostPath) || !menu.length) {
      continue;
    }
    for (const mp of menu) {
      paths.push(mp);
      flags.set(mp, { selectable: true, activatable: false });
    }
  }

  const sorted = extensions
    .filter((e) => e.basePath.startsWith(`${tabPath}/`))
    .sort((a, b) => a.basePath.localeCompare(b.basePath));
  for (const ext of sorted) {
    collectNavPathsFromTree(ext.instance, ext.basePath, paths, flags);
  }

  navPathFlags.value = flags;
  return paths;
}

export function isComponentActive(path: string): boolean {
  return navMode.value === "active" && activePath.value === path;
}

export function activateComponent(path: string): void {
  if (!isNavActivatablePath(path)) return;
  selectedPath.value = path;
  activePath.value = path;
  navMode.value = "active";
  syncNavSelectionVisual(path);
}

export function selectComponent(path: string): void {
  if (navMode.value === "active" && activePath.value !== path) {
    deactivateComponent();
  }
  selectedPath.value = path;
  syncNavSelectionVisual(path);
}

export function deactivateComponent(): void {
  navMode.value = "select";
  activePath.value = "";
}

export function resetWorkspaceNav(): void {
  navMode.value = "select";
  selectedPath.value = "";
  activePath.value = "";
  navPathFlags.value = new Map();
  spatialData.clear();
  clearNavSelectionVisual();
}

/**
 * Пространственная навигация: от текущего компонента находим ближайший
 * в запрошенном направлении по координатам центров.
 * score = primary_dist + secondary_dist * 0.5
 */
export function moveNavSelection(key: NavArrowKey): void {
  const paths = navPaths.value;
  if (!paths.length) return;

  const curPath = selectedPath.value;
  const cur = spatialData.get(curPath);

  // Нет пространственных данных — просто первый/последний
  if (!cur) {
    const path =
      key === "ArrowDown" || key === "ArrowRight"
        ? paths[0]!
        : paths[paths.length - 1]!;
    selectComponent(path);
    scrollNavPathIntoView(path);
    return;
  }

  let bestPath = "";
  let bestScore = Infinity;
  // Небольшой порог игнорирования — чтобы компоненты почти на одном уровне
  // не блокировали друг друга.
  const EPS = 12;

  for (const path of paths) {
    if (path === curPath) continue;
    const r = spatialData.get(path);
    if (!r) continue;

    const dx = r.cx - cur.cx;
    const dy = r.cy - cur.cy;
    let primary: number;
    let secondary: number;

    switch (key) {
      case "ArrowRight": if (dx <= EPS)  continue; primary = dx;  secondary = Math.abs(dy); break;
      case "ArrowLeft":  if (dx >= -EPS) continue; primary = -dx; secondary = Math.abs(dy); break;
      case "ArrowDown":  if (dy <= EPS)  continue; primary = dy;  secondary = Math.abs(dx); break;
      case "ArrowUp":    if (dy >= -EPS) continue; primary = -dy; secondary = Math.abs(dx); break;
      default: continue;
    }

    const score = primary + secondary * 0.5;
    if (score < bestScore) { bestScore = score; bestPath = path; }
  }

  if (bestPath) {
    selectComponent(bestPath);
    scrollNavPathIntoView(bestPath);
  }
}

export function ensureSelectedInNav(): void {
  if (!navPaths.value.length) {
    selectedPath.value = "";
    clearNavSelectionVisual();
    return;
  }
  if (!navPaths.value.includes(selectedPath.value)) {
    selectComponent(navPaths.value[0]!);
  }
}

export function scrollNavPathIntoView(path: string): void {
  const escaped = typeof CSS !== "undefined" && "escape" in CSS ? CSS.escape(path) : path;
  document
    .querySelector<HTMLElement>(`[data-nav-path="${escaped}"]`)
    ?.scrollIntoView({ block: "nearest" });
}

/** Фокус внутрь активного leaf-компонента (таблица, поле…). */
export function focusComponent(path: string): void {
  const escaped = typeof CSS !== "undefined" && "escape" in CSS ? CSS.escape(path) : path;
  const node = document.querySelector<HTMLElement>(`[data-nav-path="${escaped}"]`);
  if (!node) return;
  const target =
    node.querySelector<HTMLElement>(
      '[data-nav-focus], .grid-scroll, button:not([disabled]), input:not([disabled]):not([tabindex="-1"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ) ?? node;
  target.focus({ preventScroll: true });
}
