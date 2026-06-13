<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useProject, type CommitSummary } from "../../composables/useProject";

defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const { info, historyList, diffCommits, checkoutCommit } = useProject();
const hasProject = computed(() => Boolean(info.value.path));

const commits = ref<CommitSummary[]>([]);
const selectedId = ref<string | null>(null);
const compareId = ref<string | null>(null);
const diff = ref<string>("");
const loadingHistory = ref(false);
const loadingDiff = ref(false);
const checkoutBusy = ref(false);
const errorMsg = ref<string | null>(null);

async function refresh(): Promise<void> {
  if (!hasProject.value) {
    commits.value = [];
    return;
  }
  loadingHistory.value = true;
  errorMsg.value = null;
  try {
    commits.value = await historyList();
    if (commits.value.length > 0 && !selectedId.value) {
      selectedId.value = commits.value[0].id;
    }
  } catch (e) {
    errorMsg.value = e instanceof Error ? e.message : String(e);
  } finally {
    loadingHistory.value = false;
  }
}

async function loadDiff(): Promise<void> {
  const from = selectedId.value;
  if (!from) {
    diff.value = "";
    return;
  }
  loadingDiff.value = true;
  errorMsg.value = null;
  try {
    diff.value = await diffCommits(from, compareId.value ?? undefined);
  } catch (e) {
    errorMsg.value = e instanceof Error ? e.message : String(e);
    diff.value = "";
  } finally {
    loadingDiff.value = false;
  }
}

async function onCheckout(): Promise<void> {
  if (!selectedId.value || checkoutBusy.value) return;
  checkoutBusy.value = true;
  errorMsg.value = null;
  try {
    await checkoutCommit(selectedId.value);
    await refresh();
  } catch (e) {
    errorMsg.value = e instanceof Error ? e.message : String(e);
  } finally {
    checkoutBusy.value = false;
  }
}

function selectCommit(id: string): void {
  selectedId.value = id;
  compareId.value = null;
}

function formatTs(ms: number): string {
  if (!ms) return "—";
  return new Date(ms).toLocaleString(undefined, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

watch(hasProject, (v) => { if (v) void refresh(); else commits.value = []; }, { immediate: true });
watch([selectedId, compareId], () => { void loadDiff(); });

onMounted(() => { if (hasProject.value) void refresh(); });

function diffLines(): Array<{ text: string; kind: "add" | "del" | "meta" | "plain" }> {
  return diff.value.split("\n").map((line) => {
    if (line.startsWith("+++") || line.startsWith("---")) return { text: line, kind: "meta" };
    if (line.startsWith("@@")) return { text: line, kind: "meta" };
    if (line.startsWith("+")) return { text: line, kind: "add" };
    if (line.startsWith("-")) return { text: line, kind: "del" };
    return { text: line, kind: "plain" };
  });
}

const selectedIndex = computed(() =>
  commits.value.findIndex((c) => c.id === selectedId.value),
);

const compareOptions = computed(() =>
  commits.value.filter((c) => c.id !== selectedId.value),
);
</script>

<template>
  <div class="ph-root">
    <div v-if="!hasProject" class="ph-empty">
      <span class="ph-empty-text">Откройте проект для просмотра истории.</span>
    </div>
    <template v-else>
      <!-- toolbar -->
      <div class="ph-toolbar">
        <button
          class="ph-btn primary"
          :disabled="checkoutBusy || !selectedId"
          @click="onCheckout"
        >
          Откатить к выбранной версии
        </button>
        <button class="ph-btn" :disabled="loadingHistory" @click="refresh">
          Обновить
        </button>
        <span v-if="errorMsg" class="ph-error">{{ errorMsg }}</span>
      </div>

      <div class="ph-body">
        <!-- commit list -->
        <div class="ph-list-col">
          <div class="ph-list-header">
            <span class="ph-list-title">История ({{ commits.length }})</span>
            <span v-if="loadingHistory" class="ph-spinner" />
          </div>
          <ul class="ph-list">
            <li
              v-for="commit in commits"
              :key="commit.id"
              class="ph-list-item"
              :class="{ selected: commit.id === selectedId }"
              :title="commit.id"
              @click="selectCommit(commit.id)"
            >
              <span class="ph-commit-msg">{{ commit.message }}</span>
              <span class="ph-commit-meta">
                <code class="ph-short-id">{{ commit.shortId }}</code>
                <span class="ph-commit-ts">{{ formatTs(commit.timestampMs) }}</span>
              </span>
            </li>
          </ul>
        </div>

        <!-- diff pane -->
        <div class="ph-diff-col">
          <div class="ph-diff-header">
            <span class="ph-diff-label">Diff</span>
            <template v-if="selectedId">
              <span class="ph-diff-from">от {{ selectedIndex >= 0 ? commits[selectedIndex]?.shortId : "?" }}</span>
              <span class="ph-diff-sep">→</span>
              <select v-model="compareId" class="ph-compare-select">
                <option :value="null">текущее (unsaved)</option>
                <option v-for="c in compareOptions" :key="c.id" :value="c.id">
                  {{ c.shortId }} — {{ c.message.slice(0, 40) }}
                </option>
              </select>
            </template>
            <span v-if="loadingDiff" class="ph-spinner" />
          </div>
          <div class="ph-diff-body">
            <pre
              v-if="diff"
              class="ph-diff-pre"
            ><span
              v-for="(line, i) in diffLines()"
              :key="i"
              class="ph-diff-line"
              :class="'ph-diff-' + line.kind"
            >{{ line.text }}
</span></pre>
            <div v-else-if="!loadingDiff && selectedId" class="ph-diff-empty">
              Нет изменений
            </div>
            <div v-else-if="!selectedId" class="ph-diff-empty">
              Выберите версию слева
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.ph-root {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 0.88rem;
}

.ph-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--color-text-muted);
}

.ph-empty-text {
  font-size: 0.95rem;
}

.ph-toolbar {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.5rem 0.75rem;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.ph-btn {
  padding: 0.35rem 0.75rem;
  font-size: 0.85rem;
  font-weight: 600;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border);
  background: var(--color-bg-muted);
  color: var(--color-text);
  cursor: pointer;
}

.ph-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.ph-btn:not(:disabled):hover {
  border-color: var(--color-border-strong);
}

.ph-btn.primary {
  background: var(--color-accent);
  color: var(--color-on-accent);
  border-color: var(--color-accent);
}

.ph-btn.primary:not(:disabled):hover {
  filter: brightness(1.05);
}

.ph-error {
  color: var(--color-error, #e05);
  font-size: 0.82rem;
  margin-left: 0.5rem;
}

.ph-body {
  display: flex;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.ph-list-col {
  width: 22rem;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--color-border);
  overflow: hidden;
}

.ph-list-header {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.4rem 0.65rem;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.ph-list-title {
  font-size: 0.78rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-text-muted);
}

.ph-list {
  list-style: none;
  margin: 0;
  padding: 0.25rem;
  overflow-y: auto;
  flex: 1;
}

.ph-list-item {
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
  padding: 0.45rem 0.55rem;
  border-radius: var(--radius-sm);
  cursor: pointer;
  border: 1px solid transparent;
  margin-bottom: 0.15rem;
}

.ph-list-item:hover {
  background: var(--color-bg-muted);
}

.ph-list-item.selected {
  background: var(--color-bg-muted);
  border-color: var(--color-border-strong);
}

.ph-commit-msg {
  font-size: 0.88rem;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.ph-commit-meta {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.ph-short-id {
  font-family: ui-monospace, monospace;
  font-size: 0.78rem;
  color: var(--color-text-muted);
  background: var(--color-bg-muted);
  padding: 0 0.3em;
  border-radius: 3px;
}

.ph-commit-ts {
  font-size: 0.78rem;
  color: var(--color-text-muted);
}

.ph-diff-col {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  overflow: hidden;
}

.ph-diff-header {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.4rem 0.65rem;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
  flex-wrap: wrap;
}

.ph-diff-label {
  font-size: 0.78rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-text-muted);
}

.ph-diff-from {
  font-size: 0.82rem;
  font-family: ui-monospace, monospace;
  color: var(--color-text-muted);
}

.ph-diff-sep {
  color: var(--color-text-muted);
}

.ph-compare-select {
  font-size: 0.82rem;
  background: var(--color-bg-muted);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  color: var(--color-text);
  padding: 0.15rem 0.4rem;
  max-width: 20rem;
}

.ph-diff-body {
  flex: 1;
  overflow: auto;
  min-height: 0;
  padding: 0.5rem;
}

.ph-diff-pre {
  margin: 0;
  font-family: ui-monospace, monospace;
  font-size: 0.8rem;
  line-height: 1.5;
  tab-size: 4;
  white-space: pre;
}

.ph-diff-line {
  display: block;
}

.ph-diff-add {
  background: rgba(0, 180, 80, 0.12);
  color: #3a3;
}

.ph-diff-del {
  background: rgba(220, 50, 50, 0.12);
  color: #c44;
}

.ph-diff-meta {
  color: var(--color-text-muted);
  opacity: 0.8;
}

.ph-diff-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--color-text-muted);
  font-size: 0.9rem;
}

.ph-spinner {
  display: inline-block;
  width: 0.9rem;
  height: 0.9rem;
  border: 2px solid var(--color-border);
  border-top-color: var(--color-accent);
  border-radius: 50%;
  animation: spin 0.7s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}
</style>
