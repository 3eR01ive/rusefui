import type { ComponentInstance } from "./types";

let counter = 0;

/** Стабильный ключ инстанса для Vue :key. */
export function instanceKey(instance: ComponentInstance, path: string): string {
  return instance.id ?? `${path}:${instance.type}:${counter}`;
}

export function childPath(parentPath: string, index: number, child: ComponentInstance): string {
  const id = child.id ?? String(index);
  return `${parentPath}/${id}`;
}

export function bumpInstanceCounter(): void {
  counter += 1;
}
