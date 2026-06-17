import type { ComponentInstance } from "./types";
import { getRegisteredComponent } from "./registry";
import { childPath } from "./instance";

export interface NavPathFlags {
  selectable: boolean;
  activatable: boolean;
}

function isContainerInstance(instance: ComponentInstance): boolean {
  return (instance.children?.length ?? 0) > 0;
}

function propsVariant(instance: ComponentInstance): string | undefined {
  const props = instance.props as { variant?: string } | undefined;
  return props?.variant;
}

/** Поля nav* и props в YAML инстанса; иначе true. */
export function resolveNavSelectable(instance: ComponentInstance): boolean {
  if (instance.navSelectable !== undefined) return instance.navSelectable;
  if (instance.type === "text" && propsVariant(instance) === "hint") return false;
  // generated-panel загружает детей динамически — они регистрируются как extension
  if (instance.type === "generated-panel") return false;
  return true;
}

export function resolveNavActivatable(instance: ComponentInstance): boolean {
  return instance.navActivatable ?? true;
}

export function collectNavPathsFromTree(
  instance: ComponentInstance,
  path: string,
  paths: string[],
  flags: Map<string, NavPathFlags>,
): void {
  if (!instance.type) return;
  if (!getRegisteredComponent(instance.type)) return;
  if (isContainerInstance(instance)) {
    for (let index = 0; index < instance.children!.length; index++) {
      const child = instance.children![index]!;
      collectNavPathsFromTree(child, childPath(path, index, child), paths, flags);
    }
    return;
  }
  if (!resolveNavSelectable(instance)) return;
  paths.push(path);
  flags.set(path, {
    selectable: true,
    activatable: resolveNavActivatable(instance),
  });
}
