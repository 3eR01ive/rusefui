<script setup lang="ts">
import { computed, onUnmounted, ref, shallowRef, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { initConfig, useConfig, configDataRevision } from "../../composables/useConfig";
import { initOutputChannels, useOutputChannels } from "../../composables/useOutputChannels";
import ComponentHost from "../ComponentHost.vue";
import ChannelPickerBtn from "../canvas/ChannelPickerBtn.vue";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

void initConfig();
void initOutputChannels();

const { getArray } = useConfig();
const { snapshot: outSnap } = useOutputChannels();

// outField — читает напрямую из быстрого snapshot (используется только в tick)
function outField(name: string): number | null {
  if (!name) return null;
  const v = outSnap.value.values[name];
  return v === undefined ? null : v;
}

// liveSnap — throttled копия для шаблона, обновляется не чаще раза в 100мс
// чтобы ECU-пакеты не триггерили ре-рендер компонента на каждом тике
const liveSnap = shallowRef<Record<string, number>>({});
{
  let pending = false;
  watch(outSnap, (snap) => {
    if (pending) return;
    pending = true;
    setTimeout(() => {
      liveSnap.value = snap.values as Record<string, number>;
      pending = false;
    }, 100);
  }, { immediate: true });
}

function liveField(name: string): number | null {
  if (!name) return null;
  const v = liveSnap.value[name];
  return v === undefined ? null : v;
}

// ── Props ───────────────────────────────────────────────────────
function str(k: string): string { return String(props.props[k] ?? ""); }
function num(k: string, def: number): number {
  const v = Number(props.props[k]);
  return Number.isFinite(v) && v > 0 ? v : def;
}

const veXBins = computed(() => str("veXBins"));
const veYBins = computed(() => str("veYBins"));
const veZBins = computed(() => str("veZBins"));
const taXBins  = computed(() => str("targetAfrXBins"));
const taYBins  = computed(() => str("targetAfrYBins"));
const taZBins  = computed(() => str("targetAfrZBins"));

// Editable channel overrides — initialized from props, changeable in UI
const afrCh       = ref(str("afrChannel")       || "AFRValue");
const targetAfrCh = ref(str("targetAfrChannel") || "targetAFR");
const rpmCh       = ref(str("rpmChannel")       || "RPMValue");
const loadCh      = ref(str("loadChannel")       || "MAPValue");

// Explicit output channels for table cell highlighting (separate from algorithm channels)
const tableXCh = computed(() => str("tableXChannel") || rpmCh.value);
const tableYCh = computed(() => str("tableYChannel") || loadCh.value);

const iniChannelNames = ref<string[]>([]);
void invoke<{ name: string }[]>("output_list_fields").then(fs => {
  iniChannelNames.value = fs.map(f => f.name).sort();
}).catch(() => {});

const outChannelNames = computed(() => {
  const live = Object.keys(liveSnap.value);
  return (live.length ? live : iniChannelNames.value).sort();
});

interface LimitDef { channel: string; op: string; value: number }

function parseLimits(raw: unknown): LimitDef[] {
  if (!Array.isArray(raw)) return [];
  return (raw as unknown[]).filter(
    (l): l is LimitDef =>
      !!l && typeof (l as LimitDef).channel === "string" &&
      typeof (l as LimitDef).value === "number" &&
      typeof (l as LimitDef).op === "string",
  );
}

const limits = ref<LimitDef[]>(parseLimits(props.props.limits));
const newLim  = ref<LimitDef | null>(null);

function addLimit(): void {
  newLim.value = { channel: rpmCh.value || "RPMValue", op: ">=", value: 500 };
}
function commitNewLim(): void {
  if (!newLim.value || !newLim.value.channel) return;
  limits.value = [...limits.value, { ...newLim.value }];
  newLim.value = null;
}
function removeLimit(i: number): void {
  limits.value = limits.value.filter((_, idx) => idx !== i);
}

const strength = ref(num("strength", 0.1));
const tickMs   = computed(() => num("tickMs", 500));

// ── Child instances ─────────────────────────────────────────────
const instId = computed(() => props.path.replace(/\//g, "-"));
const veInstance = computed<ComponentInstance>(() => ({
  type: "config-table",
  id: `${instId.value}-ve`,
  bind: { source: "config", params: {
    xBins: veXBins.value, yBins: veYBins.value, zBins: veZBins.value,
    xOutputChannel: tableXCh.value, yOutputChannel: tableYCh.value,
  } },
  props: { title: "VE" },
}));
const taInstance = computed<ComponentInstance>(() => ({
  type: "config-table",
  id: `${instId.value}-tafr`,
  bind: { source: "config", params: {
    xBins: taXBins.value, yBins: taYBins.value, zBins: taZBins.value,
    xOutputChannel: tableXCh.value, yOutputChannel: tableYCh.value,
  } },
  props: { title: "Target AFR" },
}));

// ── Axes cache ──────────────────────────────────────────────────
const xBins = ref<number[]>([]);
const yBins = ref<number[]>([]);
const cols  = computed(() => xBins.value.length);
const rows  = computed(() => yBins.value.length);
const gridSize = computed(() => cols.value * rows.value);

async function refreshAxes(): Promise<void> {
  if (!veXBins.value || !veYBins.value) return;
  try {
    const [xA, yA] = await Promise.all([
      getArray(veXBins.value),
      getArray(veYBins.value),
    ]);
    xBins.value = xA;
    yBins.value = yA;
    const sz = xA.length * yA.length;
    if (corrGrid.value.length !== sz) {
      corrGrid.value = new Array<number>(sz).fill(0);
    }
  } catch { /* not loaded yet */ }
}
watch(configDataRevision, () => { void refreshAxes(); }, { immediate: true });

// ── Correction grid ─────────────────────────────────────────────
const corrGrid  = ref<number[]>([]);
const activeIdx = ref(-1);

// ── Live values ─────────────────────────────────────────────────
// для шаблона — через liveSnap (throttled, не вызывает лишних ре-рендеров)
const liveAfr    = computed(() => liveField(afrCh.value));
const liveTarget = computed(() => liveField(targetAfrCh.value));

function findBin(bins: number[], v: number): number {
  if (!bins.length) return 0;
  for (let i = 0; i < bins.length - 1; i++) {
    if (v < (bins[i]! + bins[i + 1]!) / 2) return i;
  }
  return bins.length - 1;
}

const liveDiff = computed<number | null>(() => {
  if (liveAfr.value === null || liveTarget.value === null) return null;
  return liveAfr.value - liveTarget.value;
});
const liveCorrPct = computed<number | null>(() => {
  if (liveAfr.value === null || liveTarget.value === null || liveTarget.value === 0) return null;
  return (liveAfr.value / liveTarget.value - 1) * strength.value * 100;
});

// ── Autotune loop ───────────────────────────────────────────────
const running  = ref(false);
const ticks    = ref(0);
const errMsg   = ref<string | null>(null);
let timerId: ReturnType<typeof setInterval> | null = null;

function checkOp(v: number, op: string, thr: number): boolean {
  if (op === ">=") return v >= thr;
  if (op === "<=") return v <= thr;
  if (op === ">")  return v > thr;
  if (op === "<")  return v < thr;
  if (op === "=")  return Math.abs(v - thr) < 0.001;
  return true;
}

function tick(): void {
  for (const lim of limits.value) {
    const v = outField(lim.channel);
    if (v === null || !checkOp(v, lim.op, lim.value)) { activeIdx.value = -1; return; }
  }
  const measured = outField(afrCh.value);
  const rpm      = outField(rpmCh.value);
  const load     = outField(loadCh.value);
  if (measured === null || rpm === null || load === null || measured <= 0) return;

  const xIdx = findBin(xBins.value, rpm);
  const yIdx = findBin(yBins.value, load);
  const flat = yIdx * cols.value + xIdx;
  activeIdx.value = flat;

  const target = outField(targetAfrCh.value);
  if (target === null || target <= 0) return;

  const corr = (measured / target - 1) * strength.value;
  if (Math.abs(corr) < 1e-6) return;

  const g = corrGrid.value.slice();
  g[flat] = (g[flat] ?? 0) + corr * 100;
  corrGrid.value = g;
  ticks.value += 1;
}

async function flushToVe(): Promise<void> {
  if (!veZBins.value) return;
  errMsg.value = null;
  try {
    const veArr = await getArray(veZBins.value);
    const updates: { index: number; value: number }[] = [];
    for (let i = 0; i < corrGrid.value.length; i++) {
      const pct = corrGrid.value[i] ?? 0;
      if (Math.abs(pct) < 0.01) continue;
      const cur = veArr[i];
      if (cur === undefined) continue;
      const next = Math.max(0, Math.min(250, cur * (1 + pct / 100)));
      if (Math.abs(next - cur) >= 1e-4) updates.push({ index: i, value: next });
    }
    if (updates.length === 0) return;
    await invoke("config_set_array_values", {
      params: { field: veZBins.value, updates },
    });
    resetCorr();
  } catch (e) { errMsg.value = String(e); }
}

function start(): void {
  if (running.value) return;
  errMsg.value = null;
  running.value = true;
  void refreshAxes();
  timerId = setInterval(() => { tick(); }, tickMs.value);
}
function stop(): void {
  running.value = false;
  activeIdx.value = -1;
  if (timerId !== null) { clearInterval(timerId); timerId = null; }
}
function resetCorr(): void {
  const sz = gridSize.value;
  corrGrid.value = sz > 0 ? new Array<number>(sz).fill(0) : [];
  ticks.value = 0; activeIdx.value = -1;
}
onUnmounted(stop);

// ── Limit status ────────────────────────────────────────────────
function limOk(lim: LimitDef): boolean {
  const v = liveField(lim.channel);
  return v !== null && checkOp(v, lim.op, lim.value);
}
function limVal(lim: LimitDef): string {
  const v = liveField(lim.channel);
  return v !== null ? v.toFixed(1) : "—";
}

// ── Correction cell coloring ────────────────────────────────────
function cellBg(pct: number): string {
  if (!pct) return "";
  const t = Math.min(1, Math.abs(pct) / 10);
  const a = (0.12 + t * 0.55).toFixed(2);
  return pct > 0
    ? `rgba(220,38,38,${a})`   // red  — VE↑, добавление топлива (был lean)
    : `rgba(59,130,246,${a})`; // blue — VE↓, убавление топлива (был rich)
}
function fmtCorr(v: number): string {
  if (!v) return "";
  return (v > 0 ? "+" : "") + v.toFixed(1) + "%";
}
function fmtVal(v: number | null, dec = 3): string {
  return v !== null ? v.toFixed(dec) : "—";
}
function fmtDiff(v: number | null): string {
  if (v === null) return "—";
  return (v > 0 ? "+" : "") + v.toFixed(3);
}
function fmtCorrPct(v: number | null): string {
  if (v === null) return "—";
  return (v > 0 ? "+" : "") + v.toFixed(2) + "%";
}

// Строки correction-таблицы (перевёрнуто: высокая нагрузка сверху)
const gridRows = computed<{ yVal: number; cells: { flat: number; pct: number }[] }[]>(() => {
  if (!rows.value || !cols.value || corrGrid.value.length !== gridSize.value) return [];
  const result = [];
  for (let r = rows.value - 1; r >= 0; r--) {
    const cells = [];
    for (let c = 0; c < cols.value; c++) {
      const flat = r * cols.value + c;
      cells.push({ flat, pct: corrGrid.value[flat] ?? 0 });
    }
    result.push({ yVal: yBins.value[r] ?? r, cells });
  }
  return result;
});
</script>

<template>
  <div class="at-root">
    <!-- ── Header ── -->
    <div class="at-header">
      <span class="at-title">Автотюн смеси</span>
      <div class="at-hcontrols">
        <span v-if="running" class="at-pulse" />
        <span v-if="ticks > 0" class="at-ticks">{{ ticks }} коррекций</span>
        <button class="at-btn at-btn--sm" :disabled="running || ticks === 0" @click="resetCorr">Сброс</button>
        <button class="at-btn at-btn--apply" :disabled="running || ticks === 0" @click="() => void flushToVe()">Применить в VE</button>
        <button
          class="at-btn"
          :class="running ? 'at-btn--stop' : 'at-btn--start'"
          :disabled="!veZBins || !taZBins || !afrCh"
          @click="running ? stop() : start()"
        >{{ running ? "СТОП" : "СТАРТ" }}</button>
      </div>
    </div>

    <p v-if="errMsg" class="at-error">{{ errMsg }}</p>

    <!-- ── Settings ── -->
    <div class="at-settings">
      <label class="at-setting">
        <span class="at-setting-lbl">Сила {{ (strength * 100).toFixed(0) }}%</span>
        <input type="range" min="1" max="50" step="1" class="at-slider"
          :value="Math.round(strength * 100)"
          @input="strength = Number(($event.target as HTMLInputElement).value) / 100" />
      </label>
      <div class="at-limits">
        <span class="at-lim-label">Условия:</span>
        <span v-for="(lim, i) in limits" :key="i"
          class="at-chip" :class="limOk(lim) ? 'at-chip--ok' : 'at-chip--fail'">
          {{ lim.channel }} {{ lim.op }} {{ lim.value }}
          <span class="at-chip-live">({{ limVal(lim) }})</span>
          <button class="at-lim-rm" @click="removeLimit(i)">×</button>
        </span>
        <!-- inline new-condition form -->
        <span v-if="newLim" class="at-lim-new">
          <input class="at-lim-ch" list="at-ch-list" v-model="newLim.channel" placeholder="канал" />
          <select class="at-lim-op" v-model="newLim.op">
            <option v-for="op in ['>=','<=','>','<','=']" :key="op" :value="op">{{ op }}</option>
          </select>
          <input class="at-lim-val" type="number" v-model.number="newLim.value" step="any" />
          <button class="at-lim-ok" @click="commitNewLim">✓</button>
          <button class="at-lim-rm" @click="newLim = null">×</button>
        </span>
        <button v-else class="at-lim-add" @click="addLimit">+</button>
      </div>
    </div>

    <!-- ── Channel selectors ── -->
    <div class="at-channels">
      <div class="at-ch-label">
        <span class="at-ch-name">AFR измер.</span>
        <ChannelPickerBtn v-model="afrCh" :channels="outChannelNames" placeholder="канал" />
        <span v-if="afrCh && liveField(afrCh) !== null" class="at-ch-live">{{ liveField(afrCh)!.toFixed(3) }}</span>
        <span v-else-if="afrCh" class="at-ch-missing">нет данных</span>
      </div>
      <div class="at-ch-label">
        <span class="at-ch-name">AFR цель</span>
        <ChannelPickerBtn v-model="targetAfrCh" :channels="outChannelNames" placeholder="канал" />
        <span v-if="targetAfrCh && liveField(targetAfrCh) !== null" class="at-ch-live">{{ liveField(targetAfrCh)!.toFixed(3) }}</span>
        <span v-else-if="targetAfrCh" class="at-ch-missing">нет данных</span>
      </div>
      <div class="at-ch-label">
        <span class="at-ch-name">RPM</span>
        <ChannelPickerBtn v-model="rpmCh" :channels="outChannelNames" placeholder="канал" />
        <span v-if="rpmCh && liveField(rpmCh) !== null" class="at-ch-live">{{ liveField(rpmCh)!.toFixed(0) }}</span>
        <span v-else-if="rpmCh" class="at-ch-missing">нет данных</span>
      </div>
      <div class="at-ch-label">
        <span class="at-ch-name">Нагрузка</span>
        <ChannelPickerBtn v-model="loadCh" :channels="outChannelNames" placeholder="канал" />
        <span v-if="loadCh && liveField(loadCh) !== null" class="at-ch-live">{{ liveField(loadCh)!.toFixed(1) }}</span>
        <span v-else-if="loadCh" class="at-ch-missing">нет данных</span>
      </div>
    </div>

    <!-- ── Live values ── -->
    <div class="at-live">
      <div class="at-live-item">
        <span class="at-live-lbl">{{ afrCh || "AFR" }} · измеренное</span>
        <span class="at-live-val">{{ fmtVal(liveAfr) }}</span>
      </div>
      <div class="at-live-sep" />
      <div class="at-live-item">
        <span class="at-live-lbl">{{ targetAfrCh || "targetAFR" }} · цель</span>
        <span class="at-live-val">{{ fmtVal(liveTarget) }}</span>
      </div>
      <div class="at-live-sep" />
      <div class="at-live-item">
        <span class="at-live-lbl">Разница</span>
        <span class="at-live-val"
          :class="liveDiff !== null ? (liveDiff > 0.01 ? 'at-val--rich' : liveDiff < -0.01 ? 'at-val--lean' : 'at-val--ok') : ''">
          {{ fmtDiff(liveDiff) }}
        </span>
      </div>
      <div class="at-live-sep" />
      <div class="at-live-item">
        <span class="at-live-lbl">Коррекция VE</span>
        <span class="at-live-val"
          :class="liveCorrPct !== null ? (liveCorrPct > 0 ? 'at-val--rich' : liveCorrPct < 0 ? 'at-val--lean' : 'at-val--ok') : ''">
          {{ fmtCorrPct(liveCorrPct) }}
        </span>
      </div>
    </div>

    <!-- ── Child tables ── -->
    <div class="at-tables">
      <div class="at-tbl-wrap">
        <ComponentHost :instance="veInstance" :path="`${path}/ve`" />
      </div>
      <div class="at-tbl-wrap">
        <ComponentHost :instance="taInstance" :path="`${path}/tafr`" />
      </div>
    </div>

    <!-- ── Correction grid ── -->
    <div v-if="gridRows.length > 0" class="at-diff-wrap">
      <div class="at-diff-title">Накопленная коррекция VE за сессию</div>
      <div class="at-diff-table">
        <!-- X-axis labels -->
        <div class="at-diff-row at-diff-row--header">
          <div class="at-diff-ycell" />
          <div v-for="x in xBins" :key="x" class="at-diff-xcell">{{ x.toFixed(0) }}</div>
        </div>
        <!-- Data rows -->
        <div v-for="row in gridRows" :key="row.yVal" class="at-diff-row">
          <div class="at-diff-ycell">{{ row.yVal.toFixed(row.yVal < 10 ? 2 : 1) }}</div>
          <div
            v-for="cell in row.cells" :key="cell.flat"
            class="at-diff-cell"
            :class="{ 'at-diff-cell--active': cell.flat === activeIdx }"
            :style="cell.pct ? { background: cellBg(cell.pct) } : {}"
            :title="`${fmtCorr(cell.pct)}`"
          >{{ fmtCorr(cell.pct) }}</div>
        </div>
      </div>
    </div>
    <p v-else-if="!veZBins || !taZBins" class="at-hint">
      Укажите props: veXBins / veYBins / veZBins / targetAfrZBins / afrChannel
    </p>
  </div>
</template>

<style scoped>
.at-root { display:flex; flex-direction:column; gap:.5rem; min-width:0; padding:.5rem; }

/* Header */
.at-header { display:flex; align-items:center; gap:.5rem; flex-wrap:wrap; }
.at-title  { font-size:.85rem; font-weight:600; color:var(--color-text); flex:1; }
.at-hcontrols { display:flex; align-items:center; gap:.35rem; }
.at-pulse  {
  display:inline-block; width:8px; height:8px; border-radius:50%;
  background:var(--color-accent,#3b82f6);
  animation:at-pulse 1s ease-in-out infinite;
}
@keyframes at-pulse { 0%,100%{opacity:1} 50%{opacity:.25} }
.at-ticks  { font-size:.7rem; color:var(--color-text-muted); }

/* Buttons */
.at-btn {
  padding:.3rem .75rem; font-size:.78rem; font-weight:600;
  border:1.5px solid var(--color-border); border-radius:var(--radius-sm);
  cursor:pointer; background:var(--color-bg-elevated); color:var(--color-text-muted);
}
.at-btn:disabled { opacity:.4; cursor:default; }
.at-btn--sm  { font-size:.7rem; font-weight:400; padding:.2rem .55rem; }
.at-btn--apply { border-color:#f59e0b; color:#f59e0b; background:color-mix(in srgb,#f59e0b 10%,var(--color-bg-elevated)); }
.at-btn--apply:hover:not(:disabled) { background:color-mix(in srgb,#f59e0b 20%,var(--color-bg-elevated)); }
.at-btn--start { border-color:#22c55e; color:#22c55e; background:color-mix(in srgb,#22c55e 10%,var(--color-bg-elevated)); }
.at-btn--start:hover:not(:disabled) { background:color-mix(in srgb,#22c55e 20%,var(--color-bg-elevated)); }
.at-btn--stop  { border-color:var(--color-danger,#dc2626); color:var(--color-danger,#dc2626); background:color-mix(in srgb,var(--color-danger,#dc2626) 10%,var(--color-bg-elevated)); }
.at-btn--stop:hover { background:color-mix(in srgb,var(--color-danger,#dc2626) 20%,var(--color-bg-elevated)); }

/* Settings */
.at-settings {
  display:flex; align-items:center; gap:.75rem; flex-wrap:wrap;
  padding:.35rem .5rem; border-radius:var(--radius-sm);
  border:1px solid var(--color-border); background:var(--color-bg-elevated);
}
.at-setting { display:flex; align-items:center; gap:.35rem; }
.at-setting-lbl { font-size:.73rem; color:var(--color-text-muted); white-space:nowrap; min-width:72px; }
.at-slider { width:90px; accent-color:var(--color-accent,#3b82f6); }
.at-limits { display:flex; align-items:center; gap:.25rem; flex-wrap:wrap; }
.at-lim-label { font-size:.7rem; color:var(--color-text-subtle); }
.at-chip {
  display:inline-flex; align-items:center; gap:.2rem;
  font-size:.68rem; padding:.1rem .35rem;
  border-radius:999px; border:1px solid;
}
.at-chip--ok   { border-color:#22c55e; color:#22c55e; }
.at-chip--fail { border-color:var(--color-danger,#dc2626); color:var(--color-danger,#dc2626); }
.at-chip-live  { opacity:.65; }
.at-lim-rm {
  font-size:.7rem; line-height:1; padding:0 .1rem;
  border:none; background:none; color:inherit; opacity:.5; cursor:pointer;
}
.at-lim-rm:hover { opacity:1; }
.at-lim-new { display:inline-flex; align-items:center; gap:.2rem; }
.at-lim-ch {
  width:84px; font-size:.7rem; padding:.1rem .25rem;
  border:1px solid var(--color-border); border-radius:var(--radius-sm);
  background:var(--color-bg); color:var(--color-text); outline:none;
}
.at-lim-ch:focus { border-color:var(--color-accent,#3b82f6); }
.at-lim-op {
  font-size:.7rem; padding:.1rem .1rem;
  border:1px solid var(--color-border); border-radius:var(--radius-sm);
  background:var(--color-bg); color:var(--color-text); outline:none; cursor:pointer;
}
.at-lim-val {
  width:52px; font-size:.7rem; padding:.1rem .25rem;
  border:1px solid var(--color-border); border-radius:var(--radius-sm);
  background:var(--color-bg); color:var(--color-text); outline:none;
}
.at-lim-ok {
  font-size:.72rem; padding:.1rem .3rem; border-radius:var(--radius-sm);
  border:1px solid #22c55e; background:none; color:#22c55e; cursor:pointer;
}
.at-lim-add {
  font-size:.78rem; font-weight:600; padding:.05rem .3rem;
  border-radius:999px; border:1px dashed var(--color-border);
  background:none; color:var(--color-text-subtle); cursor:pointer; line-height:1.4;
}
.at-lim-add:hover { border-color:var(--color-accent,#3b82f6); color:var(--color-accent,#3b82f6); }

/* Live values */
.at-live {
  display:flex; align-items:stretch; gap:0;
  border:1px solid var(--color-border); border-radius:var(--radius-sm);
  background:var(--color-bg-elevated); overflow:hidden;
}
.at-live-item {
  flex:1; display:flex; flex-direction:column;
  padding:.35rem .6rem; min-width:80px;
}
.at-live-lbl  { font-size:.65rem; color:var(--color-text-subtle); margin-bottom:.15rem; }
.at-live-val  { font-size:.95rem; font-weight:600; font-variant-numeric:tabular-nums; color:var(--color-text); }
.at-live-sep  { width:1px; background:var(--color-border); }
.at-val--lean { color:#3b82f6; }
.at-val--rich { color:var(--color-danger,#dc2626); }
.at-val--ok   { color:#22c55e; }

/* Child tables */
.at-tables { display:flex; gap:.5rem; flex-wrap:wrap; min-width:0; }
.at-tbl-wrap { flex:1; min-width:280px; min-width:0; }

/* Diff table */
.at-diff-wrap { display:flex; flex-direction:column; gap:.25rem; overflow-x:auto; }
.at-diff-title { font-size:.7rem; color:var(--color-text-muted); font-weight:500; }
.at-diff-table { display:table; border-collapse:collapse; font-size:.62rem; }
.at-diff-row   { display:table-row; }
.at-diff-row--header .at-diff-xcell { color:var(--color-text-subtle); font-weight:500; }
.at-diff-ycell {
  display:table-cell; padding:1px 4px 1px 2px;
  color:var(--color-text-subtle); font-size:.6rem;
  white-space:nowrap; vertical-align:middle;
  border-right:1px solid var(--color-border);
}
.at-diff-xcell {
  display:table-cell; padding:1px 3px; text-align:center;
  color:var(--color-text-subtle); vertical-align:middle;
  border-bottom:1px solid var(--color-border);
  white-space:nowrap;
}
.at-diff-cell {
  display:table-cell; padding:2px 3px; text-align:center;
  color:var(--color-text); vertical-align:middle;
  border:1px solid color-mix(in srgb,var(--color-border) 60%,transparent);
  min-width:32px; white-space:nowrap;
  transition:background .25s;
}
.at-diff-cell--active {
  outline:1.5px solid var(--color-accent,#3b82f6);
  outline-offset:-1px;
  position:relative; z-index:1;
}

.at-error { margin:0; font-size:.78rem; color:var(--color-danger,#dc2626);
  background:color-mix(in srgb,var(--color-danger,#dc2626) 8%,var(--color-bg));
  padding:.25rem .5rem; border-radius:var(--radius-sm); }
.at-hint  { margin:0; font-size:.78rem; color:var(--color-text-subtle); }

/* Channel selectors */
.at-channels {
  display:flex; gap:.5rem; flex-wrap:wrap;
  padding:.35rem .5rem; border-radius:var(--radius-sm);
  border:1px solid var(--color-border); background:var(--color-bg-elevated);
}
.at-ch-label {
  display:flex; align-items:center; gap:.3rem; flex:1; min-width:160px;
}
.at-ch-name    { font-size:.72rem; color:var(--color-text-muted); white-space:nowrap; min-width:56px; }
.at-ch-live    { font-size:.72rem; color:var(--color-accent,#3b82f6); white-space:nowrap; font-variant-numeric:tabular-nums; }
.at-ch-missing { font-size:.68rem; color:var(--color-text-subtle); white-space:nowrap; }

/* Unit toggle */
.at-unit-toggle { display:flex; border:1px solid var(--color-border); border-radius:var(--radius-sm); overflow:hidden; flex-shrink:0; }
.at-unit-btn {
  padding:.1rem .3rem; font-size:.68rem; cursor:pointer;
  border:none; background:var(--color-bg-elevated); color:var(--color-text-muted);
}
.at-unit-btn--active { background:var(--color-accent,#3b82f6); color:#fff; }


</style>
