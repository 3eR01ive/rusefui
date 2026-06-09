#!/usr/bin/env node
/**
 * Парсит https://github.com/rusefi/rusefi/wiki/Lua-Scripting
 * → public/config/lua-wiki-hints.json для Monaco.
 *
 * Запуск: node scripts/extract-lua-wiki-hints.mjs
 */

import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const WIKI_URL =
  "https://raw.githubusercontent.com/wiki/rusefi/rusefi/Lua-Scripting.md";
const OUT = join(
  dirname(fileURLToPath(import.meta.url)),
  "../public/config/lua-wiki-hints.json",
);

const LUA_KEYWORDS = new Set([
  "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto",
  "if", "in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
]);

const CLASS_SECTIONS = new Map([
  ["Timer", "Timer"],
  ["PID", "Pid"],
]);

/** @type {Map<string, { label: string, insertText: string, snippet: boolean, section: string, doc: string }>} */
const items = new Map();

let section = "General";
/** @type {string | null} */
let classContext = null;

function add(label, insertText, doc = "", opts = {}) {
  const key = opts.key ?? label;
  if (items.has(key)) return;
  const snippet = Boolean(opts.snippet ?? insertText.includes("${"));
  items.set(key, {
    label,
    insertText,
    snippet,
    section: opts.section ?? section,
    doc: doc.trim(),
  });
}

function cleanInlineMd(text) {
  return text
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/``([^`]+)``/g, "`$1`")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/\*([^*]+)\*/g, "$1")
    .trim();
}

/** Текст после заголовка функции до следующего заголовка / code block. */
function collectDoc(lines, startIdx) {
  const blocks = [];
  let i = startIdx;

  while (i < lines.length) {
    const line = lines[i];
    if (/^#{2,4}\s/.test(line)) break;
    if (/^```/.test(line)) break;
    if (/^\|/.test(line)) break;

    const trimmed = line.trim();
    if (!trimmed) {
      i++;
      continue;
    }

    if (/^- (Parameters|Returns)\b/.test(trimmed)) {
      const listLines = [trimmed];
      i++;
      while (i < lines.length) {
        const next = lines[i];
        if (/^#{2,4}\s/.test(next) || /^```/.test(next)) break;
        if (/^\|/.test(next)) break;
        if (!next.trim()) {
          if (i + 1 < lines.length && /^\s{2,}-/.test(lines[i + 1])) {
            i++;
            continue;
          }
          break;
        }
        if (/^- /.test(next) && !/^\s{2,}-/.test(next)) break;
        const nested = next.match(/^(\s+)-\s+(.+)$/);
        listLines.push(nested ? `  - ${cleanInlineMd(nested[2])}` : cleanInlineMd(next.trim()));
        i++;
      }
      blocks.push(listLines.join("\n"));
      continue;
    }

    const prose = [];
    while (i < lines.length) {
      const next = lines[i];
      if (/^#{2,4}\s/.test(next) || /^```/.test(next) || /^\|/.test(next)) break;
      const t = next.trim();
      if (!t) break;
      if (/^- (Parameters|Returns)\b/.test(t)) break;
      if (/^- /.test(t) && !/^\s{2,}-/.test(next)) break;
      prose.push(cleanInlineMd(t));
      i++;
    }
    if (prose.length) {
      blocks.push(prose.join(" "));
    }
    if (i === startIdx) i++;
  }

  return blocks.join("\n\n").trim();
}

function toSnippet(sig) {
  return sig.replace(/\(([^)]*)\)/, (_, params) => {
    if (!params.trim()) return "()";
    const parts = params.split(",").map((p, idx) => {
      const name = p.trim().replace(/[^a-zA-Z0-9_]/g, "") || `arg${idx + 1}`;
      return `\${${idx + 1}:${name}}`;
    });
    return `(${parts.join(", ")})`;
  });
}

/** @returns {{ label: string, insertText: string, opts: object } | null} */
function parseFunctionHeading(line) {
  const m = line.match(/^(#{2,4})\s+(.+)$/);
  if (!m) return null;
  const level = m[1].length;
  const raw = m[2].trim();

  if (level === 3 && !raw.startsWith("`")) {
    section = raw.replace(/\s*\{#.+\}$/, "").trim();
    classContext = CLASS_SECTIONS.get(section) ?? null;
    return null;
  }

  if (level === 3 && raw.startsWith("`")) {
    const tickedH3 = raw.match(/^`([^`]+)`$/);
    if (!tickedH3) return null;
    const sig = tickedH3[1];
    const name = sig.replace(/\(.*/, "");
    if (LUA_KEYWORDS.has(name)) return null;
    return {
      label: name,
      insertText: sig.includes("(") ? toSnippet(sig) : `${sig}()`,
      opts: { section },
    };
  }

  const ticked = raw.match(/^`([^`]+)`$/);
  if (ticked && level === 4) {
    const sig = ticked[1];
    const name = sig.replace(/\(.*/, "");
    if (LUA_KEYWORDS.has(name)) return null;
    return {
      label: name,
      insertText: sig.includes("(") ? toSnippet(sig) : `${sig}()`,
      opts: { section },
    };
  }

  if (level === 4) {
    const ident = raw.match(/^([a-zA-Z][\w]*)$/);
    if (!ident) return null;
    const name = ident[1];
    if (LUA_KEYWORDS.has(name)) return null;

    if (classContext === "Timer") {
      if (name === "reset" || name === "getElapsedSeconds") {
        return {
          label: `Timer:${name}`,
          insertText: `t:${name}()`,
          opts: { section, key: `Timer:${name}` },
        };
      }
      if (name === "getTsButtonCount") {
        return {
          label: "getTsButtonCount",
          insertText: "getTsButtonCount(${1:id})",
          opts: { section, snippet: true },
        };
      }
      return null;
    }
    if (classContext) return null;

    if (/^[a-z]/.test(name)) {
      return {
        label: name,
        insertText: `${name}()`,
        opts: { section },
      };
    }
  }

  return null;
}

function parseWiki(md) {
  const lines = md.split("\n");
  for (let i = 0; i < lines.length; ) {
    const entry = parseFunctionHeading(lines[i]);
    if (entry) {
      const doc = collectDoc(lines, i + 1);
      add(entry.label, entry.insertText, doc, entry.opts);
      i++;
      while (i < lines.length && !/^#{2,4}\s/.test(lines[i])) i++;
      continue;
    }
    i++;
  }
}

function addManualExtras() {
  add("onTick", "function onTick()\n\t$0\nend", "Called periodically by rusEFI.", {
    section: "Callbacks",
    snippet: true,
    key: "onTick",
  });
  add(
    "onCanRx",
    "function onCanRx(bus, id, dlc, data)\n\t$0\nend",
    "Default CAN RX callback when canRxAdd is used without a custom callback.",
    { section: "Callbacks", snippet: true, key: "onCanRx" },
  );

  add("Timer.new", "Timer.new()", "Create a Timer instance.", { section: "Timer", key: "Timer.new" });
  add("Sensor.new", 'Sensor.new("${1:name}")', "Create a virtual sensor.", {
    section: "Sensor",
    snippet: true,
    key: "Sensor.new",
  });
  add("Pid.new", "Pid.new(${1:p}, ${2:i}, ${3:d}, ${4:min}, ${5:max})", "Create a PID controller.", {
    section: "PID",
    snippet: true,
    key: "Pid.new",
  });
  add(
    "IndustrialPid.new",
    "IndustrialPid.new(${1:p}, ${2:i}, ${3:d}, ${4:min}, ${5:max})",
    "Create an industrial PID controller.",
    { section: "PID", snippet: true, key: "IndustrialPid.new" },
  );

  for (const m of ["setOffset", "get", "reset"]) {
    add(`Pid:${m}`, `pid:${m}()`, "", { section: "PID", key: `Pid:${m}` });
  }
  for (const m of ["setDerivativeFilterLoss", "setAntiwindupFreq"]) {
    add(`IndustrialPid:${m}`, `industrialPid:${m}()`, "", { section: "PID", key: `IndustrialPid:${m}` });
  }
  add("Sensor:set", "vssSensor:set(${1:value})", "Set sensor value.", {
    section: "Sensor",
    snippet: true,
    key: "Sensor:set",
  });
  add("Sensor:setTimeout", "vssSensor:setTimeout(${1:ms})", "Sensor validity timeout, ms.", {
    section: "Sensor",
    snippet: true,
    key: "Sensor:setTimeout",
  });
  add("Sensor:setRedundant", "sensor:setRedundant(${1:value})", "", {
    section: "Sensor",
    key: "Sensor:setRedundant",
  });
  add("Sensor:invalidate", "sensor:invalidate()", "", { section: "Sensor", key: "Sensor:invalidate" });
}

const res = await fetch(WIKI_URL);
if (!res.ok) {
  console.error("Wiki fetch failed:", res.status);
  process.exit(1);
}
const md = await res.text();
parseWiki(md);
addManualExtras();

const withDoc = [...items.values()].filter((i) => i.doc.length > 0).length;
const out = {
  source: "https://github.com/rusefi/rusefi/wiki/Lua-Scripting",
  extractedAt: new Date().toISOString().slice(0, 10),
  items: [...items.values()].sort((a, b) => a.label.localeCompare(b.label)),
};

writeFileSync(OUT, `${JSON.stringify(out, null, 2)}\n`, "utf8");
console.log(`Wrote ${out.items.length} hints (${withDoc} with doc) → ${OUT}`);
