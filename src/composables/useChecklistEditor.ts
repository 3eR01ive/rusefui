import { parse as parseYaml } from "yaml";
import type { ComponentInstance, DataBinding } from "../core/types";
import type { ChecklistEditor } from "./useConfig";
import {
  initIniPanels,
  invalidateIniPanelsCache,
  loadBundledPanelsManifest,
  loadGeneratedPanelYaml,
  loadPanelsManifest,
  readBundledPanelYaml,
  registerIniPanelsChangedHandler,
} from "./useIniPanels";

interface ManifestEntry {
  id: string;
  file: string;
}

interface ManifestLayer {
  id: "active" | "bundled";
  entries: ManifestEntry[];
  readYaml: (file: string) => Promise<string>;
}

const panelChildrenCache = new Map<string, ComponentInstance[]>();
const fieldLocationCache = new Map<string, ComponentInstance | null>();

export function invalidateChecklistEditorCache(): void {
  panelChildrenCache.clear();
  fieldLocationCache.clear();
  invalidateIniPanelsCache();
}

async function manifestLayers(): Promise<ManifestLayer[]> {
  await initIniPanels();
  const layers: ManifestLayer[] = [];

  const response = await loadPanelsManifest();
  const active = response.manifest?.panels ?? [];
  if (active.length) {
    layers.push({
      id: "active",
      entries: active,
      readYaml: loadGeneratedPanelYaml,
    });
  }

  layers.push({
    id: "bundled",
    entries: (await loadBundledPanelsManifest()).panels ?? [],
    readYaml: readBundledPanelYaml,
  });

  return layers;
}

function panelChildrenCacheKey(layerId: string, file: string): string {
  return `${layerId}\0${file}`;
}

async function loadPanelChildren(layer: ManifestLayer, file: string): Promise<ComponentInstance[]> {
  const cacheKey = panelChildrenCacheKey(layer.id, file);
  const cached = panelChildrenCache.get(cacheKey);
  if (cached) return cached;

  const text = await layer.readYaml(file);
  const doc = parseYaml(text) as { children?: ComponentInstance[] };
  const children = doc.children ?? [];
  panelChildrenCache.set(cacheKey, children);
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

  for (const layer of await manifestLayers()) {
    for (const entry of layer.entries) {
      const children = await loadPanelChildren(layer, entry.file);
      const found =
        findInTree(children, field, componentId) ?? findInTree(children, field, null);
      if (found) {
        fieldLocationCache.set(key, found);
        return structuredClone(found);
      }
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
    for (const layer of await manifestLayers()) {
      const entry = layer.entries.find((p) => p.id === editor.panel);
      if (!entry) continue;
      const children = await loadPanelChildren(layer, entry.file);
      const found =
        findInTree(children, editor.field, editor.component) ??
        findInTree(children, editor.field, null);
      if (found) return structuredClone(found);
    }
  }

  return findFieldComponentInManifest(editor.field, editor.component);
}

registerIniPanelsChangedHandler(() => {
  panelChildrenCache.clear();
  fieldLocationCache.clear();
});
