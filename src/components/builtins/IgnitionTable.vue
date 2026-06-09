<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useInstanceBind } from "../../composables/useInstanceBind";
import { useRustComponent } from "../../composables/useRustComponent";
import {
  activateComponent,
  deactivateComponent,
  selectComponent,
  setNavExtension,
} from "../../composables/useWorkspaceNav";
import ConfigTable from "./ConfigTable.vue";

interface EngineParams {
  boreMm: number;
  strokeMm: number;
  rodLengthMm: number | null;
  cylinderCount: number;
  displacementCc: number | null;
  compressionRatio: number;
  valvesPerCylinder: number;
  sparkLocation: string;
  chamberType: string;
  intakeDurationDeg: number | null;
  exhaustDurationDeg: number | null;
  overlapDeg: number | null;
  fuel: string;
  aspiration: string;
}

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const instanceRef = computed(() => props.instance);
const propsRef = computed(() => props.props);
const bindingRef = computed(() => props.binding);
const { paramString } = useInstanceBind(instanceRef, bindingRef);

const settingsExpanded = ref(false);

function buildMountPayload(): Record<string, unknown> {
  return {
    xBins: paramString("xBins") ?? "",
    yBins: paramString("yBins") ?? "",
    zBins: paramString("zBins") ?? "",
  };
}

const { state, dispatch, ready, error } = useRustComponent(
  props.instance,
  props.path,
  buildMountPayload,
);

const generating = computed(() => Boolean(state.value.generating));
const canGenerate = computed(() => Boolean(state.value.canGenerate));
const message = computed(() => String(state.value.message ?? ""));
const localError = computed(() => (error.value ? String(error.value) : ""));

const tableInstance = computed((): ComponentInstance => ({
  type: "config-table",
  id: `${props.instance.id ?? "ignition"}-grid`,
  props: {
    title: propsRef.value.title ?? "Ignition Table",
    xLabel: propsRef.value.xLabel ?? "RPM",
    yLabel: propsRef.value.yLabel ?? "Load",
    variant: "table",
  },
  bind: props.instance.bind,
}));

/** `table` после `settings` в localeCompare — совпадает с порядком на экране (↓ вниз). */
const tablePath = computed(() => `${props.path}/table`);
const settingsPath = computed(() => `${props.path}/settings`);

/** Якорь в дереве nav (без UI); клавиатура — нативные поля формы. */
const settingsNavInstance = computed((): ComponentInstance => ({
  type: "text",
  id: `${props.instance.id ?? "ignition"}-settings-nav`,
  props: { text: "\u00a0" },
  navSelectable: true,
  navActivatable: false,
}));

const gridRef = ref<{ handleKeydown: (e: KeyboardEvent) => boolean } | null>(null);

function syncNavExtensions(): void {
  setNavExtension(settingsPath.value, settingsNavInstance.value);
  setNavExtension(tablePath.value, tableInstance.value);
}

function clearNavExtensions(): void {
  setNavExtension(settingsPath.value, null);
  setNavExtension(tablePath.value, null);
}

onMounted(() => {
  syncNavExtensions();
});

onBeforeUnmount(() => {
  clearNavExtensions();
});

watch([tablePath, settingsPath], () => {
  syncNavExtensions();
});

function onSettingsMouseDown(): void {
  selectComponent(settingsPath.value);
  deactivateComponent();
}

function onGridMouseDown(): void {
  selectComponent(tablePath.value);
  activateComponent(tablePath.value);
}

function rustParams(): EngineParams {
  const p = state.value.params as Record<string, unknown> | undefined;
  return {
    boreMm: Number(p?.boreMm ?? p?.bore_mm ?? 86),
    strokeMm: Number(p?.strokeMm ?? p?.stroke_mm ?? 86),
    rodLengthMm: numOrNull(p?.rodLengthMm ?? p?.rod_length_mm),
    cylinderCount: Number(p?.cylinderCount ?? p?.cylinder_count ?? 4),
    displacementCc: numOrNull(p?.displacementCc ?? p?.displacement_cc),
    compressionRatio: Number(p?.compressionRatio ?? p?.compression_ratio ?? 10),
    valvesPerCylinder: Number(p?.valvesPerCylinder ?? p?.valves_per_cylinder ?? 4),
    sparkLocation: String(p?.sparkLocation ?? p?.spark_location ?? "center"),
    chamberType: String(p?.chamberType ?? p?.chamber_type ?? "pentroof"),
    intakeDurationDeg: numOrNull(p?.intakeDurationDeg ?? p?.intake_duration_deg),
    exhaustDurationDeg: numOrNull(p?.exhaustDurationDeg ?? p?.exhaust_duration_deg),
    overlapDeg: numOrNull(p?.overlapDeg ?? p?.overlap_deg),
    fuel: String(p?.fuel ?? "gasoline_95"),
    aspiration: String(p?.aspiration ?? "naturally_aspirated"),
  };
}

function numOrNull(v: unknown): number | null {
  if (v === null || v === undefined || v === "") return null;
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
}

const params = ref<EngineParams>(rustParams());

watch(
  () => state.value.params,
  () => {
    params.value = rustParams();
  },
);

const setupSummary = computed(() => {
  const p = params.value;
  const asp =
    p.aspiration === "turbocharged"
      ? "Turbo"
      : p.aspiration === "supercharged"
        ? "SC"
        : "NA";
  return `${asp} · Ø${p.boreMm}×${p.strokeMm} mm · CR ${p.compressionRatio}`;
});

function toggleSettings(): void {
  settingsExpanded.value = !settingsExpanded.value;
}

async function patchParam(payload: Record<string, unknown>): Promise<void> {
  await dispatch("set_params", payload);
}

async function onGenerate(): Promise<void> {
  await dispatch("generate_map");
  window.dispatchEvent(new CustomEvent("config-undo-redo"));
}

const CHAMBER_OPTIONS = [
  { value: "pentroof", label: "Pentroof" },
  { value: "wedge", label: "Wedge" },
  { value: "hemi", label: "Hemi" },
  { value: "bathtub", label: "Bathtub" },
];

const SPARK_OPTIONS = [
  { value: "center", label: "Center" },
  { value: "near_center", label: "Near center" },
  { value: "edge", label: "Edge" },
];

const FUEL_OPTIONS = [
  { value: "gasoline_92", label: "Gasoline 92" },
  { value: "gasoline_95", label: "Gasoline 95" },
  { value: "gasoline_98", label: "Gasoline 98" },
  { value: "e85", label: "E85" },
];

const ASPIRATION_OPTIONS = [
  { value: "naturally_aspirated", label: "Naturally aspirated" },
  { value: "turbocharged", label: "Turbocharged" },
  { value: "supercharged", label: "Supercharged" },
];
</script>

<template>
  <div class="ignition-table" :class="{ 'ignition-table--compact': !settingsExpanded }">
    <div
      class="ignition-settings-zone nav-node"
      data-nav-node="1"
      :data-nav-path="settingsPath"
      data-nav-activatable="false"
      @mousedown.stop="onSettingsMouseDown"
    >
      <div class="ignition-gen-chrome">
        <button
          type="button"
          class="ignition-setup-toggle"
          :aria-expanded="settingsExpanded"
          :title="settingsExpanded ? 'Свернуть параметры' : 'Развернуть параметры'"
          @click="toggleSettings"
        >
          <span class="ignition-setup-chevron" :class="{ open: settingsExpanded }">▸</span>
          <span>{{ settingsExpanded ? "Свернуть" : "Параметры генерации" }}</span>
        </button>

        <span v-if="!settingsExpanded" class="ignition-compact-summary">{{ setupSummary }}</span>

        <button
          type="button"
          class="ignition-generate-btn"
          :disabled="!ready || !canGenerate || generating"
          title="Заполнить таблицу по статической модели УОЗ"
          @click="onGenerate"
        >
          {{ generating ? "Генерация…" : "Сгенерировать начальную таблицу" }}
        </button>
      </div>

      <p v-if="localError || message" class="ignition-status" :class="{ 'ignition-status--error': !!localError }">
        {{ localError || message }}
      </p>

      <div v-show="settingsExpanded" class="ignition-settings">
      <div class="ignition-settings-grid">
        <label class="ignition-field">
          <span>Диаметр цилиндра, mm</span>
          <input
            v-model.number="params.boreMm"
            type="number"
            min="50"
            max="200"
            step="0.1"
            @change="patchParam({ bore_mm: params.boreMm })"
          />
        </label>
        <label class="ignition-field">
          <span>Ход поршня, mm</span>
          <input
            v-model.number="params.strokeMm"
            type="number"
            min="50"
            max="200"
            step="0.1"
            @change="patchParam({ stroke_mm: params.strokeMm })"
          />
        </label>
        <label class="ignition-field">
          <span>Степень сжатия</span>
          <input
            v-model.number="params.compressionRatio"
            type="number"
            min="5"
            max="20"
            step="0.1"
            @change="patchParam({ compression_ratio: params.compressionRatio })"
          />
        </label>
        <label class="ignition-field">
          <span>Цилиндров</span>
          <input
            v-model.number="params.cylinderCount"
            type="number"
            min="1"
            max="16"
            step="1"
            @change="patchParam({ cylinder_count: params.cylinderCount })"
          />
        </label>
        <label class="ignition-field">
          <span>Клапанов на цилиндр</span>
          <select
            v-model.number="params.valvesPerCylinder"
            @change="patchParam({ valves_per_cylinder: params.valvesPerCylinder })"
          >
            <option :value="2">2</option>
            <option :value="3">3</option>
            <option :value="4">4</option>
            <option :value="5">5</option>
          </select>
        </label>
        <label class="ignition-field">
          <span>Форма камеры</span>
          <select
            v-model="params.chamberType"
            @change="patchParam({ chamber_type: params.chamberType })"
          >
            <option v-for="o in CHAMBER_OPTIONS" :key="o.value" :value="o.value">{{ o.label }}</option>
          </select>
        </label>
        <label class="ignition-field">
          <span>Положение свечи</span>
          <select
            v-model="params.sparkLocation"
            @change="patchParam({ spark_location: params.sparkLocation })"
          >
            <option v-for="o in SPARK_OPTIONS" :key="o.value" :value="o.value">{{ o.label }}</option>
          </select>
        </label>
        <label class="ignition-field">
          <span>Топливо</span>
          <select v-model="params.fuel" @change="patchParam({ fuel: params.fuel })">
            <option v-for="o in FUEL_OPTIONS" :key="o.value" :value="o.value">{{ o.label }}</option>
          </select>
        </label>
        <label class="ignition-field">
          <span>Наддув</span>
          <select
            v-model="params.aspiration"
            @change="patchParam({ aspiration: params.aspiration })"
          >
            <option v-for="o in ASPIRATION_OPTIONS" :key="o.value" :value="o.value">{{ o.label }}</option>
          </select>
        </label>
        <label class="ignition-field">
          <span>Overlap, °</span>
          <input
            v-model.number="params.overlapDeg"
            type="number"
            min="0"
            max="80"
            step="1"
            placeholder="—"
            @change="patchParam({ overlap_deg: params.overlapDeg })"
          />
        </label>
        <label class="ignition-field">
          <span>Длина шатуна, mm</span>
          <input
            v-model.number="params.rodLengthMm"
            type="number"
            min="80"
            max="200"
            step="0.1"
            placeholder="—"
            @change="patchParam({ rod_length_mm: params.rodLengthMm })"
          />
        </label>
      </div>
      <p class="ignition-settings-hint">
        Модель рассчитывает УОЗ по осям таблицы (RPM × MAP). Для корректного результата ось нагрузки должна быть MAP, kPa.
      </p>
      </div>
    </div>

    <div
      class="ignition-grid-host nav-node"
      data-nav-node="1"
      :data-nav-path="tablePath"
      @mousedown.stop="onGridMouseDown"
    >
      <ConfigTable
        ref="gridRef"
        :instance="tableInstance"
        :path="tablePath"
        :props="tableInstance.props ?? {}"
        :binding="binding"
        :meta="meta"
      />
    </div>
  </div>
</template>

<style scoped>
.ignition-table {
  display: flex;
  flex-direction: column;
  gap: 0.65rem;
  width: 100%;
  min-width: 0;
}

.ignition-settings-zone {
  display: flex;
  flex-direction: column;
  gap: 0.65rem;
}

.ignition-gen-chrome {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
}

.ignition-setup-toggle {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  padding: 0.35rem 0.55rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md, 8px);
  background: transparent;
  font-size: 0.78rem;
  cursor: pointer;
}

.ignition-setup-chevron {
  display: inline-block;
  transition: transform 0.15s ease;
}

.ignition-setup-chevron.open {
  transform: rotate(90deg);
}

.ignition-compact-summary {
  font-size: 0.75rem;
  color: var(--color-text-muted);
}

.ignition-generate-btn {
  margin-left: auto;
  padding: 0.45rem 0.85rem;
  border-radius: var(--radius-md, 8px);
  border: 1px solid var(--color-accent);
  background: var(--color-accent);
  color: var(--color-on-accent, #111);
  font-size: 0.8rem;
  font-weight: 600;
  cursor: pointer;
}

.ignition-generate-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.ignition-status {
  margin: 0;
  font-size: 0.75rem;
  color: var(--color-text-muted);
}

.ignition-status--error {
  color: var(--color-danger, #c0392b);
}

.ignition-settings {
  padding: 0.75rem 0.85rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md, 8px);
  background: var(--color-bg-subtle, rgba(255, 255, 255, 0.03));
}

.ignition-settings-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(11rem, 1fr));
  gap: 0.55rem 0.75rem;
}

.ignition-field {
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
  font-size: 0.78rem;
}

.ignition-field > span:first-child {
  font-size: 0.68rem;
  color: var(--color-text-muted);
}

.ignition-field input,
.ignition-field select {
  width: 100%;
  padding: 0.3rem 0.45rem;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm, 4px);
  background: var(--color-bg, #fff);
  font-size: 0.78rem;
}

.ignition-settings-hint {
  margin: 0.65rem 0 0;
  font-size: 0.72rem;
  line-height: 1.35;
  color: var(--color-text-muted);
}

.ignition-grid-host {
  width: 100%;
  min-width: 0;
  align-self: stretch;
}
</style>
