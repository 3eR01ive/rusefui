import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface PanelManifestEntry {
  id: string;
  file: string;
  title: string;
  menuPath: string;
}

export interface PanelsManifest {
  iniSource: string;
  panelCount: number;
  panels: PanelManifestEntry[];
  iniSignature?: string | null;
  iniHash?: string | null;
  generatedAtMs?: number | null;
}

export interface PanelsManifestResponse {
  source: "cache" | "bundled" | string;
  hash: string | null;
  manifest: PanelsManifest | null;
}

export interface PanelCacheStatus {
  hash: string;
  dir: string;
  manifestPath: string;
  generated: boolean;
}

const BUNDLED_MANIFEST_PATH = "/config/components/generated/manifest.json";
const BUNDLED_PANELS_BASE = "/config/components/generated";

let cachedResponse: PanelsManifestResponse | null = null;
let manifestPromise: Promise<PanelsManifestResponse> | null = null;
let initPromise: Promise<void> | null = null;
let unlisten: UnlistenFn | null = null;

const onChangedHandlers = new Set<() => void>();

export function registerIniPanelsChangedHandler(handler: () => void): void {
  onChangedHandlers.add(handler);
}

export function invalidateIniPanelsCache(): void {
  cachedResponse = null;
  manifestPromise = null;
  for (const handler of onChangedHandlers) {
    handler();
  }
}

async function fetchBundledManifest(): Promise<PanelsManifest> {
  const res = await fetch(BUNDLED_MANIFEST_PATH);
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return (await res.json()) as PanelsManifest;
}

async function loadPanelsManifestInternal(): Promise<PanelsManifestResponse> {
  try {
    const response = await invoke<PanelsManifestResponse>("panels_get_manifest");
    if (response.source === "cache" && response.manifest) {
      cachedResponse = response;
      return response;
    }
  } catch {
    /* не Tauri / offline dev */
  }

  const manifest = await fetchBundledManifest();
  const response: PanelsManifestResponse = {
    source: "bundled",
    hash: manifest.iniHash ?? null,
    manifest,
  };
  cachedResponse = response;
  return response;
}

/** Актуальный manifest: user cache или bundled fallback. */
export async function loadPanelsManifest(): Promise<PanelsManifestResponse> {
  if (cachedResponse) return cachedResponse;
  if (!manifestPromise) {
    manifestPromise = loadPanelsManifestInternal().finally(() => {
      manifestPromise = null;
    });
  }
  return manifestPromise;
}

/** YAML одной автогенерированной панели. */
export async function loadGeneratedPanelYaml(file: string): Promise<string> {
  try {
    return await invoke<string>("panels_read_yaml", { file });
  } catch {
    return readBundledPanelYaml(file);
  }
}

/** Bundled manifest из репозитория (полный набор панелей для fallback). */
export async function loadBundledPanelsManifest(): Promise<PanelsManifest> {
  return fetchBundledManifest();
}

/** YAML bundled-панели (когда cache INI не содержит нужный dialog). */
export async function readBundledPanelYaml(file: string): Promise<string> {
  const res = await fetch(`${BUNDLED_PANELS_BASE}/${file}`);
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.text();
}

export async function initIniPanels(): Promise<void> {
  if (initPromise) return initPromise;
  initPromise = (async () => {
    await loadPanelsManifest();
    if (!unlisten) {
      unlisten = await listen<PanelCacheStatus>("ini-panels-ready", () => {
        invalidateIniPanelsCache();
      });
    }
  })();
  return initPromise;
}

export function bundledPanelsManifestPath(): string {
  return BUNDLED_MANIFEST_PATH;
}
