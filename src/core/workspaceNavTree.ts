import type { ComponentInstance } from "./types";
import { collectNavPathsFromTree, type NavPathFlags } from "./navFlags";

/** @deprecated Используйте collectNavPathsFromTree с flags. */
export function collectNavPaths(instance: ComponentInstance, path: string): string[] {
  const paths: string[] = [];
  collectNavPathsFromTree(instance, path, paths, new Map());
  return paths;
}

export type { NavPathFlags };
