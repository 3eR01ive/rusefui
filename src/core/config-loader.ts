import { invoke } from "@tauri-apps/api/core";
import { parse as parseYaml } from "yaml";
import type {
  AppConfigFile,
  ComponentDefinitionFile,
  ComponentInstance,
  ComponentRef,
  ResolvedTab,
  TabDefinitionFile,
} from "./types";
import { isComponentRef } from "./types";

const CONFIG_BASE = "/config";

async function fetchText(path: string): Promise<string> {
  const url = `${CONFIG_BASE}/${path}`.replace(/\/+/g, "/");
  const res = await fetch(url);
  if (!res.ok) {
    throw new Error(`Config not found: ${url} (${res.status})`);
  }
  const ct = res.headers.get("content-type") ?? "";
  if (ct.includes("text/html")) {
    // Windows WebView2 возвращает index.html (SPA-fallback) вместо YAML-файла — читаем через IPC.
    return invoke<string>("read_ui_config", { path });
  }
  return res.text();
}

function parseFile<T>(text: string, path: string): T {
  try {
    return parseYaml(text) as T;
  } catch (e) {
    throw new Error(`Invalid YAML in ${path}: ${e}`);
  }
}

async function loadComponentDefinition(
  componentId: string,
): Promise<ComponentInstance[]> {
  const path = `components/${componentId}.yaml`;
  const text = await fetchText(path);
  const doc = parseFile<ComponentDefinitionFile>(text, path);
  if (doc.id !== componentId) {
    console.warn(
      `[config] components/${componentId}.yaml: id "${doc.id}" != expected "${componentId}"`,
    );
  }
  if (!Array.isArray(doc.children)) {
    throw new Error(`[config] ${path}: ожидается массив children`);
  }
  return doc.children;
}

async function resolveInstanceTree(
  node: ComponentInstance | ComponentRef,
  at = "root",
): Promise<ComponentInstance> {
  if (node == null || typeof node !== "object") {
    throw new Error(`[config] пустой узел layout (${at})`);
  }
  if (isComponentRef(node)) {
    const children = await loadComponentDefinition(node.$component);
    return {
      id: node.$component,
      type: "composite",
      children: await Promise.all(
        children.map((child, index) =>
          resolveInstanceTree(child, `${at}.$component[${index}]`),
        ),
      ),
    };
  }

  const resolved: ComponentInstance = {
    ...node,
    children: node.children
      ? await Promise.all(
          node.children.map((child, index) =>
            resolveInstanceTree(
              child as ComponentInstance | ComponentRef,
              `${at}.children[${index}]`,
            ),
          ),
        )
      : undefined,
  };
  return resolved;
}

async function loadTab(tabPath: string): Promise<ResolvedTab> {
  const path = `tabs/${tabPath}.tab.yaml`;
  const text = await fetchText(path);
  const doc = parseFile<TabDefinitionFile>(text, path);
  if (!doc?.tab?.id || !doc.root) {
    throw new Error(`[config] ${path}: нужны tab.id и root`);
  }

  let root: ComponentInstance;
  if (isComponentRef(doc.root)) {
    const children = await loadComponentDefinition(doc.root.$component);
    root = {
      id: doc.root.$component,
      type: "composite",
      children: await Promise.all(
        children.map((child, index) =>
          resolveInstanceTree(child, `tab.root.$component[${index}]`),
        ),
      ),
    };
  } else {
    root = await resolveInstanceTree(doc.root);
  }

  return {
    id: doc.tab.id,
    title: doc.tab.title,
    root,
  };
}

export interface LoadedAppConfig {
  title: string;
  tabs: ResolvedTab[];
}

export async function loadAppConfig(): Promise<LoadedAppConfig> {
  const appText = await fetchText("app.yaml");
  const appDoc = parseFile<AppConfigFile>(appText, "app.yaml");

  const tabs = await Promise.all(
    appDoc.tabs.map((ref) => loadTab(ref.$tab)),
  );

  return {
    title: appDoc.app.title ?? "rusefui",
    tabs,
  };
}
