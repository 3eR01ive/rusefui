import { parse as parseYaml } from "yaml";
import type { ComponentInstance, DataBinding } from "../core/types";
import type { ChecklistEditor } from "./useConfig";

interface ManifestEntry {
  id: string;
  file: string;
}

interface Manifest {
  panels: ManifestEntry[];
}

const MANIFEST_PATH = "/config/components/generated/manifest.json";
const panelChildrenCache = new Map<string, ComponentInstance[]>();
const fieldLocationCache = new Map<string, ComponentInstance | null>();
let manifestPromise: Promise<Manifest> | null = null;

async function loadManifest(): Promise<Manifest> {
  if (!manifestPromise) {
    manifestPromise = (async () => {
      const res = await fetch(MANIFEST_PATH);
      if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
      return (await res.json()) as Manifest;
    })();
  }
  return manifestPromise;
}

async function loadPanelChildren(file: string): Promise<ComponentInstance[]> {
  const cached = panelChildrenCache.get(file);
  if (cached) return cached;

  const res = await fetch(`/config/components/generated/${file}`);
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  const doc = parseYaml(await res.text()) as { children?: ComponentInstance[] };
  const children = doc.children ?? [];
  panelChildrenCache.set(file, children);
  return children;
}

function bindMatchesField(bind: DataBinding | undefined, field: string): boolean {
  if (!bind) return false;
  if (bind.field === field) return true;
  const params = bind.params;
  if (!params) return false;
  return params.zBins === field || params.yBins === field || params.xBins === field;
}

function findInTree(
  nodes: ComponentInstance[],
  field: string,
  componentId?: string | null,
): ComponentInstance | null {
  for (const node of nodes) {
    if (componentId && node.id === componentId) return node;
    if (!componentId && bindMatchesField(node.bind, field)) return node;
    if (node.children?.length) {
      const nested = findInTree(node.children, field, componentId);
      if (nested) return nested;
    }
  }
  return null;
}

function fieldCacheKey(field: string, componentId?: string | null): string {
  return componentId ? `${field}\0${componentId}` : field;
}

/** Найти enum-field (или другой bind) по имени поля config во всех INI-панелях. */
async function findFieldComponentInManifest(
  field: string,
  componentId?: string | null,
): Promise<ComponentInstance | null> {
  const key = fieldCacheKey(field, componentId);
  if (fieldLocationCache.has(key)) {
    const hit = fieldLocationCache.get(key);
    return hit ? structuredClone(hit) : null;
  }

  const manifest = await loadManifest();
  for (const entry of manifest.panels) {
    const children = await loadPanelChildren(entry.file);
    const found =
      findInTree(children, field, componentId) ?? findInTree(children, field, null);
    if (found) {
      fieldLocationCache.set(key, found);
      return structuredClone(found);
    }
  }

  fieldLocationCache.set(key, null);
  return null;
}

/** Загрузить YAML-компонент редактора для пункта checklist. */
export async function resolveChecklistEditor(
  editor: ChecklistEditor | Readonly<ChecklistEditor>,
): Promise<ComponentInstance | null> {
  const editors = await resolveChecklistEditors([editor]);
  return editors[0] ?? null;
}

/** Загрузить редакторы для всех полей пункта checklist (конфликты). */
export async function resolveChecklistEditors(
  editors: ReadonlyArray<ChecklistEditor | Readonly<ChecklistEditor>>,
): Promise<ComponentInstance[]> {
  const out: ComponentInstance[] = [];
  for (const editor of editors) {
    const inst = await resolveSingleChecklistEditor(editor);
    if (inst) out.push(inst);
  }
  return out;
}

async function resolveSingleChecklistEditor(
  editor: ChecklistEditor | Readonly<ChecklistEditor>,
): Promise<ComponentInstance | null> {
  if (editor.panel) {
    const manifest = await loadManifest();
    const entry = manifest.panels.find((p) => p.id === editor.panel);
    if (entry) {
      const children = await loadPanelChildren(entry.file);
      const found =
        findInTree(children, editor.field, editor.component) ??
        findInTree(children, editor.field, null);
      if (found) return structuredClone(found);
    }
  }

  return findFieldComponentInManifest(editor.field, editor.component);
}
