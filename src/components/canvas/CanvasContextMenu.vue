<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { CtxState, TableEntry, CurveEntry, ConfigFieldEntry, OutputFieldEntry } from "../../composables/useCanvasContextMenu";
import type { ComponentMeta } from "../../core/types";

const props = defineProps<{
  ctx: CtxState | null;
  menuTypes: ComponentMeta[];
}>();

const emit = defineEmits<{
  "select-type": [type: string];
  "select-table": [t: TableEntry];
  "select-curve": [c: CurveEntry];
  "select-config-field": [name: string];
  "select-output-field": [name: string];
  back: [];
  close: [];
}>();

// Фильтры живут здесь и сбрасываются при смене stage
const tableFilter = ref("");
const curveFilter = ref("");
const fieldFilter = ref("");
const outputFilter = ref("");

watch(() => props.ctx?.stage, () => {
  tableFilter.value = "";
  curveFilter.value = "";
  fieldFilter.value = "";
  outputFilter.value = "";
});

const filteredTables = computed<TableEntry[]>(() => {
  if (!props.ctx || props.ctx.stage !== "table") return [];
  const q = tableFilter.value.toLowerCase();
  return q
    ? props.ctx.tables.filter((t) => t.title.toLowerCase().includes(q) || t.id.toLowerCase().includes(q))
    : props.ctx.tables;
});

const filteredCurves = computed<CurveEntry[]>(() => {
  if (!props.ctx || props.ctx.stage !== "curve") return [];
  const q = curveFilter.value.toLowerCase();
  return q
    ? props.ctx.curves.filter((c) => c.title.toLowerCase().includes(q) || c.id.toLowerCase().includes(q))
    : props.ctx.curves;
});

const filteredConfigFields = computed<ConfigFieldEntry[]>(() => {
  if (!props.ctx || props.ctx.stage !== "field") return [];
  const q = fieldFilter.value.toLowerCase();
  return q
    ? props.ctx.configFields.filter((f) => f.name.toLowerCase().includes(q) || (f.units ?? "").toLowerCase().includes(q))
    : props.ctx.configFields;
});

const filteredOutputFields = computed<OutputFieldEntry[]>(() => {
  if (!props.ctx || props.ctx.stage !== "output-field") return [];
  const q = outputFilter.value.toLowerCase();
  return q
    ? props.ctx.outputFields.filter((f) => f.name.toLowerCase().includes(q) || (f.units ?? "").toLowerCase().includes(q))
    : props.ctx.outputFields;
});

function hasBind(m: ComponentMeta): boolean {
  return !!(m.bindMeta?.needsTable || m.bindMeta?.needsCurve || m.bindMeta?.needsConfigField || m.bindMeta?.needsOutputField);
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="ctx"
      class="ccm-menu"
      :style="{ left: `${ctx.menuX}px`, top: `${ctx.menuY}px` }"
      @contextmenu.prevent
    >
      <!-- Stage: список типов -->
      <template v-if="ctx.stage === 'types'">
        <div class="ccm-header">Добавить компонент</div>
        <div class="ccm-scroll">
          <button
            v-for="m in menuTypes"
            :key="m.type"
            type="button"
            class="ccm-item"
            :class="{ 'ccm-item--has-bind': hasBind(m) }"
            @pointerdown.stop
            @click="emit('select-type', m.type)"
          >
            <span class="ccm-item-label">{{ m.label }}</span>
            <span v-if="hasBind(m)" class="ccm-item-arrow">›</span>
          </button>
        </div>
      </template>

      <!-- Stage: выбор таблицы -->
      <template v-else-if="ctx.stage === 'table'">
        <div class="ccm-header ccm-header--nav">
          <button class="ccm-back" @pointerdown.stop @click="emit('back')">‹</button>
          Выберите таблицу
        </div>
        <div v-if="ctx.loading" class="ccm-hint">Загрузка…</div>
        <template v-else-if="ctx.tables.length">
          <div class="ccm-search" @pointerdown.stop>
            <input v-model="tableFilter" class="ccm-search-input" placeholder="Поиск…" autofocus @keydown.stop />
          </div>
          <div v-if="!filteredTables.length" class="ccm-hint">Нет совпадений</div>
          <div v-else class="ccm-scroll">
            <button
              v-for="t in filteredTables"
              :key="t.id"
              type="button"
              class="ccm-item"
              @pointerdown.stop
              @click="emit('select-table', t)"
            >{{ t.title }}</button>
          </div>
        </template>
        <div v-else class="ccm-hint">INI не загружен или таблиц нет</div>
      </template>

      <!-- Stage: выбор кривой -->
      <template v-else-if="ctx.stage === 'curve'">
        <div class="ccm-header ccm-header--nav">
          <button class="ccm-back" @pointerdown.stop @click="emit('back')">‹</button>
          Выберите кривую
        </div>
        <div v-if="ctx.loading" class="ccm-hint">Загрузка…</div>
        <template v-else-if="ctx.curves.length">
          <div class="ccm-search" @pointerdown.stop>
            <input v-model="curveFilter" class="ccm-search-input" placeholder="Поиск…" autofocus @keydown.stop />
          </div>
          <div v-if="!filteredCurves.length" class="ccm-hint">Нет совпадений</div>
          <div v-else class="ccm-scroll">
            <button
              v-for="c in filteredCurves"
              :key="c.id"
              type="button"
              class="ccm-item"
              @pointerdown.stop
              @click="emit('select-curve', c)"
            >{{ c.title }}</button>
          </div>
        </template>
        <div v-else class="ccm-hint">INI не загружен или кривых нет</div>
      </template>

      <!-- Stage: выбор поля конфига -->
      <template v-else-if="ctx.stage === 'field'">
        <div class="ccm-header ccm-header--nav">
          <button v-if="!ctx.editKey" class="ccm-back" @pointerdown.stop @click="emit('back')">‹</button>
          {{ ctx.editKey ? 'Сменить параметр' : 'Выберите параметр' }}
        </div>
        <div class="ccm-search" @pointerdown.stop>
          <input v-model="fieldFilter" class="ccm-search-input" placeholder="Поиск…" autofocus @keydown.stop />
        </div>
        <div v-if="ctx.loading" class="ccm-hint">Загрузка…</div>
        <div v-else-if="!ctx.configFields.length" class="ccm-hint">INI не загружен или параметров нет</div>
        <div v-else-if="!filteredConfigFields.length" class="ccm-hint">Нет совпадений</div>
        <div v-else class="ccm-scroll">
          <button
            v-for="f in filteredConfigFields"
            :key="f.name"
            type="button"
            class="ccm-item"
            @pointerdown.stop
            @click="emit('select-config-field', f.name)"
          >
            <span class="ccm-item-label">{{ f.name }}</span>
            <span v-if="f.units" class="ccm-item-units">{{ f.units }}</span>
          </button>
        </div>
      </template>

      <!-- Stage: выбор output-канала -->
      <template v-else-if="ctx.stage === 'output-field'">
        <div class="ccm-header ccm-header--nav">
          <button v-if="!ctx.editKey" class="ccm-back" @pointerdown.stop @click="emit('back')">‹</button>
          {{ ctx.editKey ? 'Сменить канал' : 'Выберите канал' }}
        </div>
        <div class="ccm-search" @pointerdown.stop>
          <input v-model="outputFilter" class="ccm-search-input" placeholder="Поиск…" autofocus @keydown.stop />
        </div>
        <div v-if="ctx.loading" class="ccm-hint">Загрузка…</div>
        <div v-else-if="!ctx.outputFields.length" class="ccm-hint">INI не загружен</div>
        <div v-else-if="!filteredOutputFields.length" class="ccm-hint">Нет совпадений</div>
        <div v-else class="ccm-scroll">
          <button
            v-for="f in filteredOutputFields"
            :key="f.name"
            type="button"
            class="ccm-item"
            @pointerdown.stop
            @click="emit('select-output-field', f.name)"
          >
            <span class="ccm-item-label">{{ f.name }}</span>
            <span v-if="f.units" class="ccm-item-units">{{ f.units }}</span>
          </button>
        </div>
      </template>
    </div>
  </Teleport>
</template>

<style>
.ccm-menu {
  position: fixed;
  z-index: 9999;
  width: 260px;
  max-height: 460px;
  display: flex;
  flex-direction: column;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  box-shadow: 0 6px 24px rgba(0,0,0,.3);
  overflow: hidden;
}
.ccm-header {
  padding: 0.45rem 0.75rem 0.35rem;
  font-size: 0.7rem;
  font-weight: 600;
  color: var(--color-text-subtle);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}
.ccm-header--nav {
  display: flex;
  align-items: center;
  gap: 0.35rem;
}
.ccm-back {
  padding: 0 0.3rem;
  font-size: 1rem;
  line-height: 1;
  background: none;
  border: none;
  color: var(--color-text-muted);
  cursor: pointer;
  flex-shrink: 0;
}
.ccm-back:hover { color: var(--color-text); }
.ccm-scroll {
  overflow-y: auto;
  flex: 1;
  padding: 0.2rem 0;
}
.ccm-search {
  padding: 0.4rem 0.6rem;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}
.ccm-search-input {
  width: 100%;
  box-sizing: border-box;
  padding: 0.3rem 0.5rem;
  font-size: 0.82rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: var(--color-bg);
  color: var(--color-text);
  outline: none;
}
.ccm-search-input:focus { border-color: var(--color-accent, #3b82f6); }
.ccm-item {
  display: flex;
  align-items: center;
  width: 100%;
  padding: 0.36rem 0.75rem;
  text-align: left;
  font-size: 0.82rem;
  color: var(--color-text);
  background: transparent;
  border: none;
  cursor: pointer;
  gap: 0.3rem;
}
.ccm-item:hover {
  background: color-mix(in srgb, var(--color-accent, #3b82f6) 10%, var(--color-bg-elevated));
  color: var(--color-accent, #3b82f6);
}
.ccm-item-label { flex: 1; }
.ccm-item-arrow {
  font-size: 1rem;
  color: var(--color-text-subtle);
  flex-shrink: 0;
}
.ccm-item-units {
  font-size: 0.72rem;
  color: var(--color-text-subtle);
  flex-shrink: 0;
  margin-left: auto;
  padding-left: 0.4rem;
}
.ccm-hint {
  padding: 0.6rem 0.75rem;
  font-size: 0.78rem;
  color: var(--color-text-subtle);
  flex-shrink: 0;
}
</style>
