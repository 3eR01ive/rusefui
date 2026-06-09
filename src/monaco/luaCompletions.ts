import type * as Monaco from "monaco-editor";

interface LuaWikiHint {
  label: string;
  insertText: string;
  snippet?: boolean;
  section?: string;
  doc?: string;
}

interface LuaWikiHintsFile {
  items: LuaWikiHint[];
}

let hintsCache: LuaWikiHint[] | null = null;
let providerRegistered = false;

async function loadHints(): Promise<LuaWikiHint[]> {
  if (hintsCache) {
    return hintsCache;
  }
  const res = await fetch("/config/lua-wiki-hints.json");
  if (!res.ok) {
    throw new Error(`lua-wiki-hints.json: HTTP ${res.status}`);
  }
  const data = (await res.json()) as LuaWikiHintsFile;
  hintsCache = data.items ?? [];
  return hintsCache;
}

export async function registerLuaCompletions(monaco: typeof Monaco): Promise<void> {
  if (providerRegistered) {
    return;
  }
  providerRegistered = true;

  const items = await loadHints().catch(() => [] as LuaWikiHint[]);

  monaco.languages.registerCompletionItemProvider("lua", {
    triggerCharacters: [".", "(", '"', "'"],
    provideCompletionItems(model, position) {
      const word = model.getWordUntilPosition(position);
      const range: Monaco.IRange = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: word.startColumn,
        endColumn: word.endColumn,
      };

      const suggestions: Monaco.languages.CompletionItem[] = items.map((item) => ({
        label: item.label,
        kind: monaco.languages.CompletionItemKind.Function,
        insertText: item.insertText,
        insertTextRules: item.snippet
          ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet
          : undefined,
        detail: item.section,
        documentation: item.doc
          ? { value: item.doc, isTrusted: true }
          : undefined,
        range,
      }));

      return { suggestions };
    },
  });
}
