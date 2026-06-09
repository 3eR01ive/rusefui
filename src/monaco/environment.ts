import type * as Monaco from "monaco-editor";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";

let environmentReady = false;

function cssVar(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value || fallback;
}

export function ensureMonacoEnvironment(): void {
  if (environmentReady || typeof window === "undefined") {
    return;
  }
  environmentReady = true;

  self.MonacoEnvironment = {
    getWorker() {
      return new EditorWorker();
    },
  };
}

export function defineRusefuiTheme(monaco: typeof Monaco): void {
  const bg = cssVar("--color-bg-elevated", "#ffffff");
  const fg = cssVar("--color-text", "#3a3530");
  const muted = cssVar("--color-bg-muted", "#f3efe8");
  const subtle = cssVar("--color-text-subtle", "#9c948a");
  const accent = cssVar("--color-accent", "#e07020");
  const border = cssVar("--color-border", "#e0d9ce");

  monaco.editor.defineTheme("rusefui", {
    base: "vs",
    inherit: true,
    rules: [
      { token: "comment", foreground: subtle.replace("#", ""), fontStyle: "italic" },
      { token: "string", foreground: "8b5a2b" },
      { token: "keyword", foreground: "b84a2a" },
      { token: "number", foreground: "6e6760" },
      { token: "identifier", foreground: fg.replace("#", "") },
    ],
    colors: {
      "editor.background": bg,
      "editor.foreground": fg,
      "editor.lineHighlightBackground": muted,
      "editor.selectionBackground": `${accent}44`,
      "editor.inactiveSelectionBackground": `${accent}22`,
      "editorLineNumber.foreground": subtle,
      "editorLineNumber.activeForeground": fg,
      "editorCursor.foreground": accent,
      "editorWidget.border": border,
      "editorOverviewRuler.border": border,
    },
  });
}
