import { ref } from "vue";
import type { ComponentInstance } from "../core/types";
import { collectNavPaths } from "../core/workspaceNavTree";

export type NavMode = "select" | "active";

export interface NavExtension {
  basePath: string;
  instance: ComponentInstance;
}

/** Режим навигации по компонентам текущей вкладки. */
export const navMode = ref<NavMode>("select");
/** Синяя рамка — выбранный узел (стрелки). */
export const selectedPath = ref("");
/** Зелёная рамка — активный компонент (перехватывает клавиатуру). */
export const activePath = ref("");
/** Leaf-пути текущей вкладки (depth-first). */
export const navPaths = ref<string[]>([]);
/** Динамически подгружаемые поддеревья (checklist editor, INI preview…). */
export const navExtensions = ref<NavExtension[]>([]);
/** Пункты бокового меню вместо leaf-оболочки (checklist, INI browser…). */
export const navMenuPaths = ref<Map<string, string[]>>(new Map());
/** Выбранный пункт меню для возврата ← из редактора. */
export const navSidebarAnchor = ref("");

export type NavArrowKey = "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight";
export type NavRegion = "sidebar" | "main" | "default";

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
  const staticPaths = collectNavPaths(tabRoot, tabPath);
  const result: string[] = [];
  for (const p of staticPaths) {
    const menu = menuPaths.get(p);
    if (menu?.length) {
      result.push(...menu);
    } else {
      result.push(p);
    }
  }
  const sorted = extensions
    .filter((e) => e.basePath.startsWith(`${tabPath}/`))
    .sort((a, b) => a.basePath.localeCompare(b.basePath));
  for (const ext of sorted) {
    result.push(...collectNavPaths(ext.instance, ext.basePath));
  }
  return result;
}

export function isComponentActive(path: string): boolean {
  return navMode.value === "active" && activePath.value === path;
}

export function activateComponent(path: string): void {
  selectedPath.value = path;
  activePath.value = path;
  navMode.value = "active";
}

export function selectComponent(path: string): void {
  if (navMode.value === "active" && activePath.value !== path) {
    deactivateComponent();
  }
  if (path.includes("/menu/")) {
    navSidebarAnchor.value = path;
  }
  selectedPath.value = path;
}

export function deactivateComponent(): void {
  navMode.value = "select";
  activePath.value = "";
}

export function resetWorkspaceNav(): void {
  navMode.value = "select";
  selectedPath.value = "";
  activePath.value = "";
  navSidebarAnchor.value = "";
}

export function navRegion(path: string): NavRegion {
  if (path.includes("/menu/") || path.endsWith("/filter")) return "sidebar";
  if (
    path.includes("/editor/") ||
    path.endsWith("/editor") ||
    path.includes("/preview/") ||
    path.endsWith("/preview")
  ) {
    return "main";
  }
  return "default";
}

function pathsInRegion(paths: readonly string[], region: NavRegion): string[] {
  return paths.filter((p) => navRegion(p) === region);
}

function moveLinear(delta: -1 | 1): void {
  const paths = navPaths.value;
  if (!paths.length) return;
  const cur = paths.indexOf(selectedPath.value);
  const next =
    cur < 0
      ? delta > 0
        ? 0
        : paths.length - 1
      : Math.max(0, Math.min(cur + delta, paths.length - 1));
  const path = paths[next]!;
  selectComponent(path);
  scrollNavPathIntoView(path);
}

function moveWithinRegion(delta: -1 | 1, region: NavRegion): void {
  const regionPaths = pathsInRegion(navPaths.value, region);
  if (!regionPaths.length) return;
  const cur = regionPaths.indexOf(selectedPath.value);
  const next =
    cur < 0
      ? delta > 0
        ? 0
        : regionPaths.length - 1
      : Math.max(0, Math.min(cur + delta, regionPaths.length - 1));
  const path = regionPaths[next]!;
  selectComponent(path);
  scrollNavPathIntoView(path);
}

function jumpToMain(): void {
  const mains = pathsInRegion(navPaths.value, "main");
  if (!mains.length) return;
  const path = mains[0]!;
  selectComponent(path);
  scrollNavPathIntoView(path);
}

function jumpToSidebarAnchor(): void {
  const paths = navPaths.value;
  const anchor = navSidebarAnchor.value;
  if (anchor && paths.includes(anchor)) {
    selectComponent(anchor);
    scrollNavPathIntoView(anchor);
    return;
  }
  const sidebarMenus = paths.filter((p) => navRegion(p) === "sidebar" && p.includes("/menu/"));
  const fallback = sidebarMenus[0];
  if (fallback) {
    selectComponent(fallback);
    scrollNavPathIntoView(fallback);
  }
}

export function moveNavSelection(key: NavArrowKey): void {
  const paths = navPaths.value;
  if (!paths.length) return;
  const cur = selectedPath.value;
  const region = navRegion(cur);

  if (region === "default") {
    const delta = key === "ArrowDown" || key === "ArrowRight" ? 1 : -1;
    moveLinear(delta);
    return;
  }

  if (region === "sidebar") {
    if (key === "ArrowDown") {
      moveWithinRegion(1, "sidebar");
      return;
    }
    if (key === "ArrowUp") {
      moveWithinRegion(-1, "sidebar");
      return;
    }
    if (key === "ArrowRight") {
      if (cur.includes("/menu/")) {
        navSidebarAnchor.value = cur;
      }
      jumpToMain();
      return;
    }
    if (key === "ArrowLeft") {
      moveWithinRegion(-1, "sidebar");
    }
    return;
  }

  // main (редактор / preview)
  if (key === "ArrowDown") {
    moveWithinRegion(1, "main");
    return;
  }
  if (key === "ArrowUp") {
    moveWithinRegion(-1, "main");
    return;
  }
  if (key === "ArrowLeft") {
    jumpToSidebarAnchor();
    return;
  }
  if (key === "ArrowRight") {
    moveWithinRegion(1, "main");
  }
}

export function ensureSelectedInNav(): void {
  if (!navPaths.value.length) {
    selectedPath.value = "";
    return;
  }
  if (!navPaths.value.includes(selectedPath.value)) {
    selectedPath.value = navPaths.value[0]!;
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
