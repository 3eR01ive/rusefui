import type { ComponentInstance } from "./types";
import { childPath } from "./instance";
import { getRegisteredComponent } from "./registry";

/** Обход дерева: только leaf-компоненты (не isContainer), порядок depth-first. */
export function collectNavPaths(instance: ComponentInstance, path: string): string[] {
  if (!instance.type) return [];
  const entry = getRegisteredComponent(instance.type);
  if (!entry) return [];
  if (entry.meta.isContainer && instance.children?.length) {
    return instance.children.flatMap((child, index) =>
      collectNavPaths(child, childPath(path, index, child)),
    );
  }
  return [path];
}
