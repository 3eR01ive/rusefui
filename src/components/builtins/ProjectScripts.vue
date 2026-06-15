<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import {
  useProject,
  registerProjectUiFlushHook,
  type CommitSummary,
  type ProjectScript,
} from "../../composables/useProject";
import { useEcuConsole } from "../../composables/useEcuConsole";
import MonacoEditor from "../MonacoEditor.vue";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const scriptField = computed(() =>
  typeof props.props.scriptField === "string" ? props.props.scriptField : "luaScript",
);

const {
  info,
  listScripts,
  createScript,
  deleteScript,
  getScriptContent,
  setScriptContent,
  scriptEcuRead,
  scriptEcuWrite,
  scriptEcuBurn,
  importScript,
  scriptHistory,
  scriptDiff,
  checkoutScriptVersion,
} = useProject();

const hasProject = computed(() => Boolean(info.value.path));

// --- Script list ---
const scripts = ref<ProjectScript[]>([]);
const selectedId = ref<string | null>(null);
const editorContent = ref("");
const loadingScripts = ref(false);
const loadingContent = ref(false);
const error = ref<string | null>(null);

// --- ECU ---
const ecuBusy = ref(false);
const ecuMsg = ref<string | null>(null);
const ecuError = ref(false);

// --- ECU console ---
const consoleOpen = ref(false);
const consoleScrollRef = ref<HTMLElement | null>(null);
const autoScroll = ref(true);
const { lines: consoleLines, clear: consoleClear } = useEcuConsole();

watch(consoleLines, () => {
  if (autoScroll.value && consoleScrollRef.value) {
    consoleScrollRef.value.scrollTop = consoleScrollRef.value.scrollHeight;
  }
}, { flush: "post" });

// --- History panel ---
const historyOpen = ref(false);
const historyCommits = ref<CommitSummary[]>([]);
const historySelectedId = ref<string | null>(null);
const historyDiff = ref("");
const loadingHistory = ref(false);
const loadingHistoryDiff = ref(false);
const checkoutBusy = ref(false);

// --- Debounced save ---
let pendingContent: string | null = null;
let saveTimer: ReturnType<typeof setTimeout> | null = null;

async function flushPendingContent(): Promise<void> {
  if (saveTimer) { clearTimeout(saveTimer); saveTimer = null; }
  if (pendingContent !== null && selectedId.value) {
    const content = pendingContent;
    pendingContent = null;
    await setScriptContent(selectedId.value, content).catch(() => {});
  }
}

const unregisterFlush = registerProjectUiFlushHook(flushPendingContent);

// --- Script list ---
async function refreshScripts(): Promise<void> {
  if (!hasProject.value) { scripts.value = []; return; }
  loadingScripts.value = true;
  try {
    scripts.value = await listScripts();
    if (selectedId.value && !scripts.value.find((s) => s.id === selectedId.value)) {
      selectedId.value = null;
    }
    if (!selectedId.value && scripts.value.length > 0) {
      selectedId.value = scripts.value[0].id;
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loadingScripts.value = false;
  }
}

async function loadContent(id: string): Promise<void> {
  loadingContent.value = true;
  try {
    editorContent.value = await getScriptContent(id);
    pendingContent = null;
  } catch (e) {
    editorContent.value = "";
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loadingContent.value = false;
  }
}

watch(selectedId, async (id) => {
  await flushPendingContent();
  historyOpen.value = false;
  historyCommits.value = [];
  historySelectedId.value = null;
  historyDiff.value = "";
  if (id) await loadContent(id);
  else editorContent.value = "";
});

watch(hasProject, (v) => {
  if (v) void refreshScripts();
  else { scripts.value = []; selectedId.value = null; }
});

onMounted(() => { if (hasProject.value) void refreshScripts(); });
onUnmounted(() => { unregisterFlush(); });

function onEditorChange(content: string): void {
  pendingContent = content;
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => { void flushPendingContent(); }, 300);
}

// --- Create / Delete / Import ---
async function onNewScript(): Promise<void> {
  const name = window.prompt("Название скрипта:", "Новый скрипт");
  if (name === null) return;
  error.value = null;
  try {
    const script = await createScript(name.trim() || "Новый скрипт");
    scripts.value.push(script);
    await nextTick();
    selectedId.value = script.id;
  } catch (e) { error.value = e instanceof Error ? e.message : String(e); }
}

async function onImportFile(): Promise<void> {
  const path = await invoke<string | null>("pick_script_file");
  if (!path) return;
  error.value = null;
  try {
    const script = await importScript(path);
    scripts.value.push(script);
    await nextTick();
    selectedId.value = script.id;
  } catch (e) { error.value = e instanceof Error ? e.message : String(e); }
}

async function onDeleteScript(id: string): Promise<void> {
  const s = scripts.value.find((s) => s.id === id);
  if (!s || !window.confirm(`Удалить скрипт «${s.name}»?`)) return;
  await flushPendingContent();
  error.value = null;
  try {
    await deleteScript(id);
    scripts.value = scripts.value.filter((s) => s.id !== id);
    if (selectedId.value === id) selectedId.value = scripts.value[0]?.id ?? null;
  } catch (e) { error.value = e instanceof Error ? e.message : String(e); }
}

// --- ECU ---
async function onEcuRead(): Promise<void> {
  ecuBusy.value = true; ecuMsg.value = null; ecuError.value = false;
  try {
    const content = await scriptEcuRead(scriptField.value);
    editorContent.value = content;
    pendingContent = content;
    if (saveTimer) clearTimeout(saveTimer);
    await flushPendingContent();
    ecuMsg.value = "Прочитано из ECU";
  } catch (e) { ecuMsg.value = e instanceof Error ? e.message : String(e); ecuError.value = true; }
  finally { ecuBusy.value = false; }
}

async function onEcuWrite(): Promise<void> {
  await flushPendingContent();
  ecuBusy.value = true; ecuMsg.value = null; ecuError.value = false;
  try {
    await scriptEcuWrite(scriptField.value, editorContent.value);
    ecuMsg.value = "Записано в RAM ECU, luareset выполнен";
  } catch (e) { ecuMsg.value = e instanceof Error ? e.message : String(e); ecuError.value = true; }
  finally { ecuBusy.value = false; }
}

async function onEcuBurn(): Promise<void> {
  ecuBusy.value = true; ecuMsg.value = null; ecuError.value = false;
  try {
    await scriptEcuBurn();
    ecuMsg.value = "Burn во flash выполнен";
  } catch (e) { ecuMsg.value = e instanceof Error ? e.message : String(e); ecuError.value = true; }
  finally { ecuBusy.value = false; }
}

// --- History panel ---
async function toggleHistory(): Promise<void> {
  if (!selectedId.value) return;
  historyOpen.value = !historyOpen.value;
  if (historyOpen.value) {
    consoleOpen.value = false;
    if (historyCommits.value.length === 0) await loadHistory();
  }
}

async function loadHistory(): Promise<void> {
  if (!selectedId.value) return;
  loadingHistory.value = true;
  historyCommits.value = [];
  historySelectedId.value = null;
  historyDiff.value = "";
  try {
    historyCommits.value = await scriptHistory(selectedId.value);
    if (historyCommits.value.length > 0) {
      historySelectedId.value = historyCommits.value[0].id;
    }
  } catch (e) { error.value = e instanceof Error ? e.message : String(e); }
  finally { loadingHistory.value = false; }
}

async function loadHistoryDiff(commitId: string): Promise<void> {
  if (!selectedId.value) return;
  historySelectedId.value = commitId;
  loadingHistoryDiff.value = true;
  historyDiff.value = "";
  try {
    // Diff: from = selected commit, to = next commit (or current working)
    const idx = historyCommits.value.findIndex((c) => c.id === commitId);
    const olderCommit = historyCommits.value[idx + 1]; // next = older in walk order
    historyDiff.value = await scriptDiff(
      selectedId.value!,
      olderCommit?.id ?? commitId,
      olderCommit ? commitId : undefined,
    );
    if (!historyDiff.value) {
      // First commit — diff against nothing: from empty to this commit
      historyDiff.value = await scriptDiff(selectedId.value!, commitId, undefined);
    }
  } catch (e) { error.value = e instanceof Error ? e.message : String(e); }
  finally { loadingHistoryDiff.value = false; }
}

watch(historySelectedId, (id) => { if (id) void loadHistoryDiff(id); });

async function onRestoreVersion(): Promise<void> {
  if (!selectedId.value || !historySelectedId.value || checkoutBusy.value) return;
  checkoutBusy.value = true;
  error.value = null;
  try {
    const content = await checkoutScriptVersion(selectedId.value, historySelectedId.value);
    editorContent.value = content;
    pendingContent = null;
    await loadHistory();
  } catch (e) { error.value = e instanceof Error ? e.message : String(e); }
  finally { checkoutBusy.value = false; }
}

function formatTs(ms: number): string {
  if (!ms) return "—";
  return new Date(ms).toLocaleString(undefined, {
    month: "2-digit", day: "2-digit",
    hour: "2-digit", minute: "2-digit",
  });
}

function diffLines(text: string) {
  return text.split("\n").map((line) => {
    if (line.startsWith("+++") || line.startsWith("---") || line.startsWith("@@"))
      return { text: line, kind: "meta" };
    if (line.startsWith("+")) return { text: line, kind: "add" };
    if (line.startsWith("-")) return { text: line, kind: "del" };
    return { text: line, kind: "plain" };
  });
}
</script>

<template>
  <div class="ps-root">
    <div v-if="!hasProject" class="ps-empty">
      <span>Откройте проект для работы со скриптами.</span>
    </div>
    <template v-else>
      <div class="ps-layout">
        <!-- ---- Sidebar ---- -->
        <div class="ps-sidebar">
          <div class="ps-sidebar-header">
            <span class="ps-sidebar-title">Скрипты</span>
            <div class="ps-sidebar-actions">
              <button class="ps-icon-btn" title="Создать скрипт" :disabled="loadingScripts" @click="onNewScript">
                <svg viewBox="0 0 14 14" fill="none"><path d="M7 2v10M2 7h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
              </button>
              <button class="ps-icon-btn" title="Открыть .lua файл с диска" :disabled="loadingScripts" @click="onImportFile">
                <svg viewBox="0 0 14 14" fill="none"><path d="M2 8.5C2 7.67 2.67 7 3.5 7H5.88l1 1H10.5c.83 0 1.5.67 1.5 1.5V10.5C12 11.33 11.33 12 10.5 12h-7C2.67 12 2 11.33 2 10.5V8.5Z" fill="currentColor" opacity=".2"/><path d="M2 8.5C2 7.67 2.67 7 3.5 7h2.38l1 1H10.5c.83 0 1.5.67 1.5 1.5V10.5C12 11.33 11.33 12 10.5 12h-7C2.67 12 2 11.33 2 10.5V8.5Z" stroke="currentColor" stroke-width="1.1"/><path d="M5 7V4.5C5 3.67 5.67 3 6.5 3H9l2.5 2.5V7" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/><path d="M9 3v2.5H11.5" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/></svg>
              </button>
            </div>
          </div>

          <div v-if="loadingScripts" class="ps-spinner-wrap"><span class="ps-spinner"/></div>
          <ul v-else class="ps-script-list">
            <li
              v-for="s in scripts"
              :key="s.id"
              class="ps-script-item"
              :class="{ selected: s.id === selectedId }"
              @click="selectedId = s.id"
            >
              <span class="ps-script-name">{{ s.name }}</span>
              <button class="ps-icon-btn ps-delete-btn" title="Удалить" @click.stop="onDeleteScript(s.id)">
                <svg viewBox="0 0 12 12" fill="none"><path d="M2 2l8 8M10 2L2 10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>
              </button>
            </li>
            <li v-if="scripts.length === 0" class="ps-empty-list">Нет скриптов — нажмите +</li>
          </ul>

          <div v-if="error" class="ps-sidebar-error">{{ error }}</div>
        </div>

        <!-- ---- Editor + history ---- -->
        <div class="ps-editor-col">
          <!-- ECU toolbar -->
          <div class="ps-ecu-bar">
            <span class="ps-ecu-label">ECU ({{ scriptField }})</span>
            <button class="ps-btn" :disabled="ecuBusy || !selectedId" title="Прочитать из RAM ECU" @click="onEcuRead">Читать</button>
            <button class="ps-btn" :disabled="ecuBusy || !selectedId" title="Записать в RAM ECU + luareset" @click="onEcuWrite">Писать</button>
            <button class="ps-btn" :disabled="ecuBusy" title="Burn → Flash" @click="onEcuBurn">Burn</button>
            <span v-if="ecuBusy" class="ps-spinner"/>
            <span v-else-if="ecuMsg" class="ps-ecu-msg" :class="{ error: ecuError }">{{ ecuMsg }}</span>
            <div class="ps-ecu-spacer"/>
            <button
              class="ps-btn ps-btn--history"
              :class="{ active: historyOpen }"
              :disabled="!selectedId"
              title="История версий этого скрипта"
              @click="toggleHistory"
            >
              <svg viewBox="0 0 14 14" fill="none" style="width:0.8rem;height:0.8rem;vertical-align:-1px">
                <circle cx="7" cy="7" r="5.5" stroke="currentColor" stroke-width="1.2"/>
                <path d="M7 4.5V7l1.5 1.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
              </svg>
              История
            </button>
            <button
              class="ps-btn ps-btn--history"
              :class="{ active: consoleOpen }"
              title="Вывод ECU (Lua print)"
              @click="consoleOpen = !consoleOpen; if (consoleOpen) historyOpen = false"
            >
              <svg viewBox="0 0 14 14" fill="none" style="width:0.8rem;height:0.8rem;vertical-align:-1px">
                <rect x="1.5" y="2.5" width="11" height="9" rx="1.2" stroke="currentColor" stroke-width="1.2"/>
                <path d="M3.5 5.5l2 2-2 2M7 9.5h3.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>
              </svg>
              Консоль
            </button>
          </div>

          <!-- Monaco editor -->
          <div class="ps-monaco-wrap" :class="{ 'has-history': historyOpen, 'has-console': consoleOpen && !historyOpen }">
            <div v-if="loadingContent" class="ps-loading-overlay"><span class="ps-spinner"/></div>
            <MonacoEditor
              v-if="selectedId"
              :key="selectedId"
              v-model="editorContent"
              language="lua"
              class="ps-monaco"
              @change="onEditorChange"
            />
            <div v-else class="ps-no-selection">
              {{ scripts.length === 0 ? 'Создайте скрипт (+) или откройте файл (📂)' : 'Выберите скрипт' }}
            </div>
          </div>

          <!-- History panel -->
          <div v-if="historyOpen" class="ps-history-panel">
            <div class="ps-history-toolbar">
              <span class="ps-history-title">История скрипта</span>
              <button
                class="ps-btn ps-btn--sm"
                :disabled="checkoutBusy || !historySelectedId"
                @click="onRestoreVersion"
              >Восстановить эту версию</button>
              <button class="ps-btn ps-btn--sm" :disabled="loadingHistory" @click="loadHistory">↺</button>
              <span v-if="loadingHistory || checkoutBusy" class="ps-spinner"/>
            </div>
            <div class="ps-history-body">
              <div class="ps-history-list-col">
                <div v-if="historyCommits.length === 0 && !loadingHistory" class="ps-history-empty">
                  Нет изменений в истории
                </div>
                <ul class="ps-history-list">
                  <li
                    v-for="c in historyCommits"
                    :key="c.id"
                    class="ps-history-item"
                    :class="{ selected: c.id === historySelectedId }"
                    :title="c.id"
                    @click="historySelectedId = c.id"
                  >
                    <span class="ps-hcommit-msg">{{ c.message }}</span>
                    <span class="ps-hcommit-meta">
                      <code class="ps-hshort-id">{{ c.shortId }}</code>
                      <span class="ps-hcommit-ts">{{ formatTs(c.timestampMs) }}</span>
                    </span>
                  </li>
                </ul>
              </div>
              <div class="ps-history-diff-col">
                <span v-if="loadingHistoryDiff" class="ps-spinner"/>
                <pre v-else-if="historyDiff" class="ps-hdiff-pre"><span
                  v-for="(line, i) in diffLines(historyDiff)"
                  :key="i"
                  class="ps-hdiff-line"
                  :class="'ps-hdiff-' + line.kind"
                >{{ line.text }}
</span></pre>
                <div v-else-if="historySelectedId" class="ps-history-empty">Нет изменений</div>
                <div v-else class="ps-history-empty">Выберите версию</div>
              </div>
            </div>
          </div>
          <!-- ECU console panel -->
          <div v-if="consoleOpen" class="ps-console-panel">
            <div class="ps-console-toolbar">
              <span class="ps-console-title">Вывод ECU</span>
              <label class="ps-console-autoscroll" title="Автопрокрутка">
                <input v-model="autoScroll" type="checkbox"/>
                Auto
              </label>
              <button class="ps-btn ps-btn--sm" @click="consoleClear">Очистить</button>
            </div>
            <div ref="consoleScrollRef" class="ps-console-body">
              <div v-if="consoleLines.length === 0" class="ps-console-empty">
                Нет вывода — ECU не подключена или Lua не печатает
              </div>
              <div v-for="line in consoleLines" :key="line.id" class="ps-console-line">
                <span class="ps-console-ts">{{ line.ts }}</span>
                <span class="ps-console-text">{{ line.text }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.ps-root {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 0.88rem;
}
.ps-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--color-text-muted);
  font-size: 0.95rem;
}
.ps-layout {
  display: flex;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

/* ---- Sidebar ---- */
.ps-sidebar {
  width: 15rem;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--color-border);
  overflow: hidden;
}
.ps-sidebar-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.4rem 0.6rem;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}
.ps-sidebar-title {
  font-size: 0.78rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-text-muted);
}
.ps-sidebar-actions { display: flex; gap: 0.2rem; }
.ps-icon-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1.4rem;
  height: 1.4rem;
  background: none;
  border: none;
  border-radius: var(--radius-sm);
  cursor: pointer;
  color: var(--color-text-muted);
  padding: 0;
  flex-shrink: 0;
}
.ps-icon-btn svg { width: 0.85rem; height: 0.85rem; }
.ps-icon-btn:hover:not(:disabled) { background: var(--color-bg-muted); color: var(--color-text); }
.ps-icon-btn:disabled { opacity: 0.4; cursor: not-allowed; }
.ps-script-list {
  list-style: none;
  margin: 0;
  padding: 0.2rem;
  overflow-y: auto;
  flex: 1;
}
.ps-script-item {
  display: flex;
  align-items: center;
  gap: 0.2rem;
  padding: 0.38rem 0.45rem;
  border-radius: var(--radius-sm);
  cursor: pointer;
  border: 1px solid transparent;
  margin-bottom: 0.1rem;
}
.ps-script-item:hover { background: var(--color-bg-muted); }
.ps-script-item.selected { background: var(--color-bg-muted); border-color: var(--color-border-strong); }
.ps-script-name { flex: 1; font-size: 0.87rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ps-delete-btn { opacity: 0; }
.ps-script-item:hover .ps-delete-btn,
.ps-script-item.selected .ps-delete-btn { opacity: 0.55; }
.ps-delete-btn:hover { opacity: 1 !important; color: var(--color-error, #e05); }
.ps-empty-list { padding: 0.5rem; color: var(--color-text-muted); font-size: 0.82rem; font-style: italic; }
.ps-sidebar-error { padding: 0.35rem 0.6rem; font-size: 0.78rem; color: var(--color-error, #e05); border-top: 1px solid var(--color-border); }

/* ---- Editor column ---- */
.ps-editor-col { flex: 1; display: flex; flex-direction: column; min-width: 0; overflow: hidden; }
.ps-ecu-bar {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  padding: 0.38rem 0.6rem;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
  flex-wrap: wrap;
}
.ps-ecu-label {
  font-size: 0.75rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-text-muted);
  margin-right: 0.2rem;
}
.ps-ecu-spacer { flex: 1; }
.ps-btn {
  padding: 0.28rem 0.6rem;
  font-size: 0.82rem;
  font-weight: 600;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border);
  background: var(--color-bg-muted);
  color: var(--color-text);
  cursor: pointer;
}
.ps-btn:not(:disabled):hover { border-color: var(--color-border-strong); }
.ps-btn:disabled { opacity: 0.45; cursor: not-allowed; }
.ps-btn--sm { font-size: 0.78rem; padding: 0.22rem 0.5rem; }
.ps-btn--history { display: flex; align-items: center; gap: 0.3rem; }
.ps-btn--history.active { background: var(--color-bg-accent-soft); border-color: var(--color-accent); color: var(--color-accent-hover); }
.ps-ecu-msg { font-size: 0.78rem; color: var(--color-text-muted); }
.ps-ecu-msg.error { color: var(--color-error, #e05); }

.ps-monaco-wrap { flex: 1; position: relative; min-height: 0; overflow: hidden; transition: flex 0.15s; }
.ps-monaco-wrap.has-history { flex: 0 0 55%; }
.ps-monaco-wrap.has-console { flex: 0 0 50%; }
.ps-monaco { width: 100%; height: 100%; }
.ps-no-selection {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--color-text-muted);
  font-size: 0.9rem;
  text-align: center;
  padding: 1rem;
}
.ps-loading-overlay {
  position: absolute;
  inset: 0;
  background: var(--color-bg);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1;
}
.ps-spinner-wrap { display: flex; justify-content: center; padding: 0.75rem; }

/* ---- History panel ---- */
.ps-history-panel {
  flex: 0 0 45%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  border-top: 1px solid var(--color-border);
  overflow: hidden;
}
.ps-history-toolbar {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  padding: 0.35rem 0.6rem;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}
.ps-history-title {
  font-size: 0.75rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-text-muted);
}
.ps-history-body { display: flex; flex: 1; min-height: 0; overflow: hidden; }
.ps-history-list-col {
  width: 14rem;
  flex-shrink: 0;
  border-right: 1px solid var(--color-border);
  overflow-y: auto;
}
.ps-history-list { list-style: none; margin: 0; padding: 0.2rem; }
.ps-history-item {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  padding: 0.35rem 0.45rem;
  border-radius: var(--radius-sm);
  cursor: pointer;
  border: 1px solid transparent;
  margin-bottom: 0.1rem;
}
.ps-history-item:hover { background: var(--color-bg-muted); }
.ps-history-item.selected { background: var(--color-bg-muted); border-color: var(--color-border-strong); }
.ps-hcommit-msg { font-size: 0.82rem; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ps-hcommit-meta { display: flex; align-items: center; gap: 0.35rem; }
.ps-hshort-id {
  font-family: ui-monospace, monospace;
  font-size: 0.72rem;
  color: var(--color-text-muted);
  background: var(--color-bg-muted);
  padding: 0 0.25em;
  border-radius: 3px;
}
.ps-hcommit-ts { font-size: 0.72rem; color: var(--color-text-muted); }
.ps-history-diff-col { flex: 1; overflow: auto; padding: 0.4rem; }
.ps-history-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--color-text-muted);
  font-size: 0.82rem;
}
.ps-hdiff-pre {
  margin: 0;
  font-family: ui-monospace, monospace;
  font-size: 0.77rem;
  line-height: 1.5;
  white-space: pre;
}
.ps-hdiff-line { display: block; }
.ps-hdiff-add { background: rgba(0,180,80,.12); color: #3a3; }
.ps-hdiff-del { background: rgba(220,50,50,.12); color: #c44; }
.ps-hdiff-meta { color: var(--color-text-muted); opacity: .75; }

.ps-spinner {
  display: inline-block;
  width: 0.85rem;
  height: 0.85rem;
  border: 2px solid var(--color-border);
  border-top-color: var(--color-accent);
  border-radius: 50%;
  animation: ps-spin 0.7s linear infinite;
}
@keyframes ps-spin { to { transform: rotate(360deg); } }

/* ---- ECU console ---- */
.ps-console-panel {
  flex: 0 0 50%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  border-top: 1px solid var(--color-border);
  overflow: hidden;
}
.ps-console-toolbar {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  padding: 0.3rem 0.6rem;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}
.ps-console-title {
  font-size: 0.75rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-text-muted);
  flex: 1;
}
.ps-console-autoscroll {
  display: flex;
  align-items: center;
  gap: 0.25rem;
  font-size: 0.78rem;
  color: var(--color-text-muted);
  cursor: pointer;
  user-select: none;
}
.ps-console-body {
  flex: 1;
  overflow-y: auto;
  padding: 0.3rem 0.5rem;
  font-family: ui-monospace, monospace;
  font-size: 0.77rem;
  line-height: 1.5;
}
.ps-console-empty {
  color: var(--color-text-muted);
  font-style: italic;
  padding: 0.3rem;
}
.ps-console-line {
  display: flex;
  gap: 0.5rem;
  white-space: pre-wrap;
  word-break: break-all;
}
.ps-console-ts {
  color: var(--color-text-muted);
  flex-shrink: 0;
  font-size: 0.72rem;
  padding-top: 0.05em;
}
.ps-console-text {
  color: var(--color-text);
}
</style>
