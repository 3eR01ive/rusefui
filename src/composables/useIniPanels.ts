import { ref } from "vue";
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
  source: string;
  hash: string | null;
  manifest: PanelsManifest | null;
}

export interface PanelCacheStatus {
  hash: string;
  projectKey: string;
  dir: string;
  manifestPath: string;
  generated: boolean;
}

/** Инкремент при смене INI / пересборке panel cache — перезагрузка generated UI. */
export const panelsEpoch = ref(0);

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
  panelsEpoch.value += 1;
  for (const handler of onChangedHandlers) {
    handler();
  }
}

async function loadPanelsManifestInternal(): Promise<PanelsManifestResponse> {
  const response = await invoke<PanelsManifestResponse>("panels_get_manifest");
  if (response.source === "cache" && response.manifest) {
    cachedResponse = response;
    return response;
  }
  throw new Error(
    "Panel cache недоступен — откройте проект и дождитесь загрузки INI",
  );
}

/** Актуальный manifest из user cache текущего проекта. */
export async function loadPanelsManifest(): Promise<PanelsManifestResponse> {
  if (cachedResponse?.manifest) return cachedResponse;
  if (!manifestPromise) {
    manifestPromise = loadPanelsManifestInternal().finally(() => {
      manifestPromise = null;
    });
  }
  return manifestPromise;
}

/** YAML одной автогенерированной панели из project panel cache. */
export async function loadGeneratedPanelYaml(file: string): Promise<string> {
  return invoke<string>("panels_read_yaml", { file });
}

export function normalizeGeneratedPanelFile(file: string): string {
  const trimmed = file.trim();
  if (!trimmed) return trimmed;
  return trimmed.endsWith(".yaml") ? trimmed : `${trimmed}.yaml`;
}

export async function initIniPanels(): Promise<void> {
  if (initPromise) return initPromise;
  initPromise = (async () => {
    if (!unlisten) {
      unlisten = await listen<PanelCacheStatus>("ini-panels-ready", () => {
        invalidateIniPanelsCache();
      });
    }
    try {
      await loadPanelsManifest();
    } catch {
      /* cache появится после project_load + INI */
    }
  })();
  return initPromise;
}
