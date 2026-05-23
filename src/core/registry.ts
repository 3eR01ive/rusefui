import type { Component } from "vue";
import type { ComponentMeta, RegisteredComponent } from "./types";

const registry = new Map<string, RegisteredComponent>();

export function registerComponent(
  meta: ComponentMeta,
  component: Component,
): void {
  if (registry.has(meta.type)) {
    console.warn(`[registry] component type "${meta.type}" re-registered`);
  }
  registry.set(meta.type, { meta, component });
}

export function getRegisteredComponent(
  type: string,
): RegisteredComponent | undefined {
  return registry.get(type);
}

export function requireRegisteredComponent(type: string): RegisteredComponent {
  const entry = registry.get(type);
  if (!entry) {
    throw new Error(
      `Component type "${type}" is not registered. Implement it in code and call registerComponent().`,
    );
  }
  return entry;
}

export function listRegisteredComponents(): ComponentMeta[] {
  return [...registry.values()]
    .map((e) => e.meta)
    .sort((a, b) => a.type.localeCompare(b.type));
}

export function isRegisteredType(type: string): boolean {
  return registry.has(type);
}
