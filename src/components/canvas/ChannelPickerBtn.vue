<script setup lang="ts">
import { ref, computed, watch, nextTick, onBeforeUnmount } from "vue";

const props = defineProps<{
  modelValue: string;
  channels: string[];
  placeholder?: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [v: string];
}>();

const open = ref(false);
const query = ref("");
const btnRef = ref<HTMLElement | null>(null);
const inputRef = ref<HTMLInputElement | null>(null);
const pos = ref({ x: 0, y: 0, above: false });

const filtered = computed(() => {
  const q = query.value.toLowerCase();
  return q ? props.channels.filter(c => c.toLowerCase().includes(q)) : props.channels;
});

function toggle() {
  if (open.value) { close(); return; }
  const r = btnRef.value?.getBoundingClientRect();
  if (!r) return;
  const popH = Math.min(320, props.channels.length * 28 + 60);
  const above = r.bottom + popH > window.innerHeight;
  pos.value = {
    x: r.left,
    y: above ? r.top - popH : r.bottom + 2,
    above,
  };
  open.value = true;
  query.value = "";
  void nextTick(() => inputRef.value?.focus());
}

function select(ch: string) {
  emit("update:modelValue", ch);
  close();
}

function close() {
  open.value = false;
  query.value = "";
}

function onDocDown(e: PointerEvent) {
  if (!open.value) return;
  const pop = document.querySelector(".cpb-popup");
  if (pop?.contains(e.target as Node) || btnRef.value?.contains(e.target as Node)) return;
  close();
}
function onDocKey(e: KeyboardEvent) {
  if (e.key === "Escape" && open.value) { close(); e.stopPropagation(); }
}

document.addEventListener("pointerdown", onDocDown, true);
document.addEventListener("keydown", onDocKey, true);
onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onDocDown, true);
  document.removeEventListener("keydown", onDocKey, true);
});

watch(() => props.channels, () => { /* refilter is reactive via computed */ });
</script>

<template>
  <button
    ref="btnRef"
    type="button"
    class="cpb-btn"
    :class="{ 'cpb-btn--open': open, 'cpb-btn--empty': !modelValue }"
    @click="toggle"
  >
    <span class="cpb-val">{{ modelValue || placeholder || '—' }}</span>
    <svg class="cpb-arrow" viewBox="0 0 10 6" fill="none">
      <path d="M1 1l4 4 4-4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
    </svg>
  </button>

  <Teleport to="body">
    <div
      v-if="open"
      class="cpb-popup ccm-menu"
      :style="{ left: `${pos.x}px`, top: `${pos.y}px`, minWidth: `${btnRef?.offsetWidth ?? 160}px` }"
      @contextmenu.prevent
    >
      <div class="ccm-search" @pointerdown.stop>
        <input
          ref="inputRef"
          v-model="query"
          class="ccm-search-input"
          placeholder="Поиск…"
          @keydown.stop
          @keydown.enter="filtered[0] && select(filtered[0])"
        />
      </div>
      <div v-if="!filtered.length" class="ccm-hint">Нет совпадений</div>
      <div v-else class="ccm-scroll">
        <button
          v-for="ch in filtered"
          :key="ch"
          type="button"
          class="ccm-item"
          :class="{ 'ccm-item--active': ch === modelValue }"
          @pointerdown.stop
          @click="select(ch)"
        >{{ ch }}</button>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.cpb-btn {
  display: inline-flex;
  align-items: center;
  gap: 0.3rem;
  padding: 0.18rem 0.45rem;
  font-size: 0.75rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: var(--color-bg);
  color: var(--color-text);
  cursor: pointer;
  min-width: 0;
  flex: 1;
  text-align: left;
  transition: border-color 0.1s;
}
.cpb-btn:hover, .cpb-btn--open { border-color: var(--color-accent, #3b82f6); }
.cpb-btn--empty .cpb-val { color: var(--color-text-subtle); }
.cpb-val { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: monospace; }
.cpb-arrow { width: 10px; height: 6px; color: var(--color-text-muted); flex-shrink: 0; }
</style>

<style>
.cpb-popup { max-height: 320px; }
.ccm-item--active {
  background: color-mix(in srgb, var(--color-accent, #3b82f6) 15%, var(--color-bg-elevated));
  color: var(--color-accent, #3b82f6);
}
</style>
