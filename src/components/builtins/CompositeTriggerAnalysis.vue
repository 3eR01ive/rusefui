<script setup lang="ts">
import { computed } from "vue";

export interface TriggerFault {
  toothIndex: number;
  cycleTooth?: number | null;
  cycleSlot?: number | null;
  tUs: number;
  pos: number;
  rpm: number;
  kind: string;
  detail: string;
  ratio: number;
  teethCounted?: number | null;
}

export interface RpmBucket {
  rpmFrom: number;
  countTotal: number;
  countFaults: number;
}

export interface TriggerAnalysis {
  channel: string;
  edgeMode: string;
  edgesUsed: number;
  learned: boolean;
  teethPerRev?: number | null;
  nominalSlots?: number | null;
  wideRatio?: number | null;
  sectionPattern: number[];
  narrowUsMin?: number | null;
  narrowUsMax?: number | null;
  wideGapsTotal: number;
  faults: TriggerFault[];
  faultCount: number;
  faultByKind: Record<string, number>;
  firstFaultRpm?: number | null;
  rpmHistogram: RpmBucket[];
  message?: string | null;
}

const props = defineProps<{
  analysis: TriggerAnalysis | null;
  busy?: boolean;
}>();

const emit = defineEmits<{
  (e: "refresh"): void;
  (e: "jump", fault: TriggerFault): void;
}>();

const KIND_LABEL: Record<string, string> = {
  missedEdge: "потерян фронт",
  extraEdge: "лишний фронт",
  syncMismatch: "рассинхрон",
};

const FAULT_LIMIT = 300;

const shownFaults = computed(() => props.analysis?.faults.slice(0, FAULT_LIMIT) ?? []);
const truncated = computed(
  () => (props.analysis?.faults.length ?? 0) > FAULT_LIMIT,
);

/** Максимум сбоев в бакете — для нормировки баров гистограммы. */
const maxBucketFaults = computed(() => {
  const h = props.analysis?.rpmHistogram ?? [];
  return Math.max(1, ...h.map((b) => b.countFaults));
});

function kindLabel(k: string): string {
  return KIND_LABEL[k] ?? k;
}

/** На каком зубе цикла концентрируются потери фронта (слабый зуб колеса). */
const hotTooth = computed(() => {
  const a = props.analysis;
  if (!a) return null;
  const counts = new Map<number, number>();
  let total = 0;
  for (const f of a.faults) {
    if (f.kind === "missedEdge" && f.cycleTooth != null) {
      counts.set(f.cycleTooth, (counts.get(f.cycleTooth) ?? 0) + 1);
      total += 1;
    }
  }
  if (total < 3) return null;
  let tooth = -1;
  let n = 0;
  for (const [t, c] of counts) {
    if (c > n) {
      n = c;
      tooth = t;
    }
  }
  const share = Math.round((n / total) * 100);
  // Показываем только при явной концентрации (не размазано по всему обороту).
  if (tooth < 0 || share < 40) return null;
  return { tooth, share };
});
</script>

<template>
  <div class="ta">
    <div class="ta-head">
      <span class="ta-title">Анализ сбоев триггера</span>
      <button type="button" class="btn ta-refresh" :disabled="busy" @click="emit('refresh')">
        {{ busy ? "Анализ…" : "Анализировать" }}
      </button>
      <span v-if="analysis && analysis.learned" class="ta-sub">
        колесо: {{ analysis.teethPerRev }} зуб/об · {{ analysis.nominalSlots }} слотов ·
        широкий ×{{ analysis.wideRatio }} · паттерн [{{ analysis.sectionPattern.join(", ") }}] ·
        узкий {{ analysis.narrowUsMin }}–{{ analysis.narrowUsMax }} µs ·
        {{ analysis.edgesUsed }} фронтов
      </span>
    </div>

    <p v-if="!analysis" class="ta-hint">
      Запиши/открой trigger-лог и нажми «Анализировать» — найдём потерянные/лишние фронты и
      рассинхрон, с привязкой к зубу и оборотам.
    </p>

    <template v-else>
      <p v-if="analysis.message" class="ta-hint">{{ analysis.message }}</p>

      <div class="ta-summary">
        <span class="ta-stat" :class="{ 'ta-stat--ok': analysis.faultCount === 0 }">
          сбоев: <b>{{ analysis.faultCount }}</b>
        </span>
        <span v-for="(n, k) in analysis.faultByKind" :key="k" class="ta-badge" :class="`ta-badge--${k}`">
          {{ kindLabel(String(k)) }}: {{ n }}
        </span>
        <span v-if="analysis.firstFaultRpm" class="ta-stat ta-stat--warn">
          первый сбой ≈ <b>{{ analysis.firstFaultRpm.toFixed(0) }}</b> об/мин
        </span>
      </div>

      <p v-if="hotTooth" class="ta-insight">
        ⚠ Потери фронта концентрируются на зубе
        <b>{{ hotTooth.tooth }}/{{ analysis.teethPerRev }}</b> цикла
        ({{ hotTooth.share }}% потерь) — вероятно слабый/проблемный зуб колеса
        или захвата сигнала именно в этой точке.
      </p>

      <!-- Гистограмма сбоев по оборотам -->
      <div v-if="analysis.rpmHistogram.length" class="ta-hist">
        <div
          v-for="b in analysis.rpmHistogram"
          :key="b.rpmFrom"
          class="ta-hist-col"
          :title="`${b.rpmFrom}–${b.rpmFrom + 250} об/мин: ${b.countFaults} сбоев из ${b.countTotal} зубьев`"
        >
          <div class="ta-hist-bar-wrap">
            <div
              class="ta-hist-bar"
              :style="{ height: `${(b.countFaults / maxBucketFaults) * 100}%` }"
              :class="{ 'ta-hist-bar--zero': b.countFaults === 0 }"
            />
          </div>
          <span class="ta-hist-x">{{ (b.rpmFrom / 1000).toFixed(1) }}k</span>
        </div>
      </div>

      <!-- Таблица сбоев -->
      <div v-if="shownFaults.length" class="ta-table-wrap">
        <table class="ta-table">
          <thead>
            <tr>
              <th>зуб&nbsp;цикла</th>
              <th>об/мин</th>
              <th>тип</th>
              <th>ratio</th>
              <th>описание</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="f in shownFaults"
              :key="`${f.toothIndex}-${f.kind}`"
              class="ta-row"
              :class="`ta-row--${f.kind}`"
              @click="emit('jump', f)"
            >
              <td
                class="ta-mono"
                :title="`запись #${f.toothIndex}${f.cycleSlot != null ? ' · слот ' + f.cycleSlot : ''}`"
              >
                <template v-if="f.cycleTooth != null">
                  {{ f.cycleTooth }}<span class="ta-dim">/{{ analysis.teethPerRev }}</span>
                </template>
                <template v-else>—</template>
              </td>
              <td class="ta-mono">{{ f.rpm.toFixed(0) }}</td>
              <td><span class="ta-badge" :class="`ta-badge--${f.kind}`">{{ kindLabel(f.kind) }}</span></td>
              <td class="ta-mono">{{ f.ratio.toFixed(2) }}</td>
              <td class="ta-detail">{{ f.detail }}</td>
            </tr>
          </tbody>
        </table>
        <p v-if="truncated" class="ta-hint">
          Показаны первые {{ FAULT_LIMIT }} из {{ analysis.faults.length }} сбоев.
        </p>
        <p v-else-if="analysis.faultCount === 0" class="ta-hint ta-hint--ok">
          Сбоев не найдено — счёт зубьев и структура колеса стабильны на всём логе.
        </p>
      </div>
    </template>
  </div>
</template>

<style scoped>
.ta {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px solid var(--color-border);
}

.ta-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
}

.ta-title {
  font-size: 13px;
  font-weight: 700;
  color: var(--color-fg, #e5e7eb);
}

.ta-refresh {
  padding: 2px 10px;
  font-size: 12px;
}

.ta-sub {
  font-size: 11px;
  color: var(--color-text-muted);
  font-variant-numeric: tabular-nums;
}

.ta-hint {
  margin: 0;
  font-size: 12px;
  color: var(--color-text-muted);
}

.ta-hint--ok {
  color: #16a34a;
}

.ta-insight {
  margin: 0;
  padding: 5px 9px;
  font-size: 12px;
  color: var(--color-fg, #e5e7eb);
  background: rgba(217, 119, 6, 0.12);
  border-left: 3px solid var(--color-warning, #d97706);
  border-radius: 4px;
}

.ta-dim {
  color: var(--color-text-muted);
}

.ta-summary {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.ta-stat {
  font-size: 12px;
  color: var(--color-fg, #e5e7eb);
}

.ta-stat--ok {
  color: #16a34a;
}

.ta-stat--warn {
  color: var(--color-warning, #d97706);
  font-weight: 700;
}

.ta-badge {
  font-size: 11px;
  font-weight: 700;
  padding: 1px 7px;
  border-radius: 10px;
  white-space: nowrap;
}

.ta-badge--missedEdge {
  background: #fde2e2;
  color: #b91c1c;
}

.ta-badge--extraEdge {
  background: #fef3c7;
  color: #92400e;
}

.ta-badge--syncMismatch {
  background: #ede9fe;
  color: #6d28d9;
}

/* Гистограмма */
.ta-hist {
  display: flex;
  align-items: flex-end;
  gap: 3px;
  height: 56px;
  padding: 2px 0;
}

.ta-hist-col {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  min-width: 18px;
}

.ta-hist-bar-wrap {
  display: flex;
  align-items: flex-end;
  height: 40px;
  width: 14px;
}

.ta-hist-bar {
  width: 100%;
  background: #ef4444;
  border-radius: 2px 2px 0 0;
  min-height: 1px;
}

.ta-hist-bar--zero {
  background: var(--color-border);
}

.ta-hist-x {
  font-size: 9px;
  color: var(--color-text-muted);
  font-variant-numeric: tabular-nums;
}

/* Таблица */
.ta-table-wrap {
  max-height: 280px;
  overflow-y: auto;
  border: 1px solid var(--color-border);
  border-radius: 6px;
}

.ta-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 11px;
}

.ta-table thead th {
  position: sticky;
  top: 0;
  background: var(--color-bg, #0f1115);
  text-align: left;
  padding: 4px 8px;
  font-weight: 700;
  color: var(--color-text-muted);
  border-bottom: 1px solid var(--color-border);
}

.ta-row {
  cursor: pointer;
  border-bottom: 1px solid var(--color-border);
}

.ta-row:hover {
  background: rgba(148, 163, 184, 0.12);
}

.ta-row td {
  padding: 3px 8px;
  vertical-align: top;
}

.ta-mono {
  font-family: var(--font-mono, monospace);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.ta-detail {
  color: var(--color-text-muted);
}

.ta-row--missedEdge .ta-mono {
  color: #b91c1c;
}

.ta-row--syncMismatch .ta-mono {
  color: #6d28d9;
}
</style>
