<script setup lang="ts">
import {
  computed,
  onMounted,
  onUnmounted,
  ref,
  shallowRef,
  watch,
} from "vue";
import { parse as parseYaml } from "yaml";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { childPath } from "../../core/instance";
import ComponentHost from "../ComponentHost.vue";
import { useDataContext } from "../../core/data-context";
import { initOutputChannels, useOutputChannels } from "../../composables/useOutputChannels";
import { initConfig, useConfig } from "../../composables/useConfig";
import { drawDynoChart, type DynoRunPoint } from "../../composables/drawDynoChart";
import { clampSmoothStrength, smoothDynoPoints } from "../../composables/smoothDynoCurve";
import {
  initProject,
  PERSIST_KEY_DYNO,
  projectUiEpoch,
  useProject,
  workspaceResetEpoch,
  type DynoUiSettings,
} from "../../composables/useProject";
import { useRustComponent } from "../../composables/useRustComponent";
import { useInstanceBind } from "../../composables/useInstanceBind";
import { useTabActivity, useTabFrozenDisplay } from "../../composables/useTabActivity";
import {
  measureChartWidth,
  useChartCanvasLayout,
} from "../../composables/useChartCanvasLayout";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const SMOOTH_MAX = 20;
const CHART_HEIGHT_MIN = 180;
const CHART_HEIGHT_MAX = 720;

const chartSizeOverride = { height: null as number | null };

const chartHeight = computed(() => {
  if (chartSizeOverride.height !== null && chartSizeOverride.height > CHART_HEIGHT_MIN) {
    return chartSizeOverride.height;
  }
  const h = Number(props.props.height ?? 360);
  return h >= CHART_HEIGHT_MIN ? h : 360;
});

const { state, dispatch, error, hasLogic, ready, mounting } = useRustComponent(
  props.instance,
  props.path,
);
const instanceRef = computed(() => props.instance);
const { paramStringOr, source: bindSource } = useInstanceBind(instanceRef);
const dataCtx = useDataContext();
const { snapshot } = useOutputChannels();
const { snapshot: configSnapshot } = useConfig();
const { isActive: tabActive } = useTabActivity();
const { getProjectUi, setProjectUi } = useProject();

let applyingProjectUi = false;
let saveDynoUiTimer = 0;
let channelsConfigured = false;

const rpmField = computed(() =>
  String(state.value.rpmField ?? paramStringOr("rpmField", "RPMValue")),
);
const tpsField = computed(() =>
  String(state.value.tpsField ?? paramStringOr("tpsField", "TPSValue")),
);

watch(ready, (isReady) => {
  if (!isReady || channelsConfigured) return;
  if (bindSource.value && bindSource.value !== "outputChannels") {
    console.warn(
      `[dyno] bind.source должен быть outputChannels (каналы RPM/TPS), получен ${bindSource.value}`,
    );
  }
  const rpm = paramStringOr("rpmField", "RPMValue");
  const tps = paramStringOr("tpsField", "TPSValue");
  channelsConfigured = true;
  void dispatch("set_channels", { rpmField: rpm, tpsField: tps });
});

const liveRpm = useTabFrozenDisplay(
  () => snapshot.value.values[rpmField.value] ?? null,
  null as number | null,
);
const liveTps = useTabFrozenDisplay(
  () => snapshot.value.values[tpsField.value] ?? null,
  null as number | null,
);

const recording = computed(() => Boolean(state.value.recording));
const runPoints = computed(
  () => (state.value.runPoints as DynoRunPoint[] | undefined) ?? [],
);
const previousRunPoints = computed(
  () => (state.value.previousRunPoints as DynoRunPoint[] | undefined) ?? [],
);
const currentTorque = computed(() => Number(state.value.currentTorque ?? 0));
const currentHp = computed(() => Number(state.value.currentHp ?? 0));
const message = computed(() => (state.value.message as string) ?? null);

const ignoreTpsMin = ref(false);
const minRpm = ref(0);
const smoothStrength = ref(0);
const settingsOpen = ref(false);

const chartPoints = computed(() =>
  smoothDynoPoints(runPoints.value, smoothStrength.value),
);
const chartPreviousPoints = computed(() =>
  smoothDynoPoints(previousRunPoints.value, smoothStrength.value),
);

const connected = computed(
  () => Boolean(state.value.connected ?? dataCtx.connection.value.connected),
);
const configLoaded = computed(
  () => Boolean(state.value.configLoaded ?? configSnapshot.value.loaded),
);

const peakTorque = computed(() =>
  chartPoints.value.reduce((m, p) => Math.max(m, p.torqueNm), 0),
);
const peakHp = computed(() => chartPoints.value.reduce((m, p) => Math.max(m, p.hp), 0));

const showRunPeaks = computed(
  () => !recording.value && runPoints.value.length > 0,
);

const displayTorque = computed(() =>
  showRunPeaks.value ? peakTorque.value : currentTorque.value,
);
const displayHp = computed(() => (showRunPeaks.value ? peakHp.value : currentHp.value));

const canToggleRecord = computed(() => {
  if (recording.value) return true;
  return (
    ready.value &&
    connected.value &&
    configLoaded.value &&
    hasLogic.value
  );
});
const canClear = computed(
  () =>
    !recording.value &&
    (runPoints.value.length > 0 || previousRunPoints.value.length > 0),
);

const statusMode = computed(() => {
  if (recording.value) return "recording";
  if (!connected.value) return "offline";
  if (!configLoaded.value) return "noconfig";
  if (runPoints.value.length > 0) return "done";
  return "idle";
});

const statusLabel = computed(() => {
  switch (statusMode.value) {
    case "recording":
      return "Запись";
    case "offline":
      return "Нет ECU";
    case "noconfig":
      return "Нет config";
    case "done":
      return "Есть прогон";
    default:
      return "Готов";
  }
});

const smoothPct = computed(() => (smoothStrength.value / SMOOTH_MAX) * 100);
const smoothTrackRef = ref<HTMLElement | null>(null);
const smoothDisabled = computed(() => runPoints.value.length < 3);

function smoothFromClientX(clientX: number): number {
  const el = smoothTrackRef.value;
  if (!el) return smoothStrength.value;
  const rect = el.getBoundingClientRect();
  if (rect.width <= 0) return smoothStrength.value;
  const t = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
  return clampSmoothStrength(Math.round(t * SMOOTH_MAX));
}

function onSmoothTrackPointerDown(event: MouseEvent): void {
  if (smoothDisabled.value) return;
  event.preventDefault();
  smoothStrength.value = smoothFromClientX(event.clientX);
  const onMove = (ev: MouseEvent) => {
    smoothStrength.value = smoothFromClientX(ev.clientX);
  };
  const onUp = () => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
    scheduleSaveDynoUiToProject();
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
}

function onSmoothTrackKeydown(event: KeyboardEvent): void {
  if (smoothDisabled.value) return;
  if (event.key === "ArrowRight" || event.key === "ArrowUp") {
    smoothStrength.value = clampSmoothStrength(smoothStrength.value + 1);
  } else if (event.key === "ArrowLeft" || event.key === "ArrowDown") {
    smoothStrength.value = clampSmoothStrength(smoothStrength.value - 1);
  } else {
    return;
  }
  event.preventDefault();
  scheduleSaveDynoUiToProject();
}

function toggleRecording(): void {
  if (recording.value) {
    void dispatch("stop_run");
  } else {
    void dispatch("start_run");
  }
}

function clearRun() {
  return dispatch("clear");
}

function buildDynoUiSettings(): DynoUiSettings {
  return {
    ignoreTpsMin: ignoreTpsMin.value,
    minRpm: Math.max(0, Math.round(minRpm.value)),
    smoothStrength: clampSmoothStrength(smoothStrength.value),
    chartHeight: chartHeight.value,
    settingsOpen: settingsOpen.value,
  };
}

async function syncOptionsToRust(): Promise<void> {
  if (!ready.value) return;
  await dispatch("set_options", {
    ignoreTpsMin: ignoreTpsMin.value,
    minRpm: Math.max(0, Math.round(minRpm.value)),
  });
}

async function applyDynoUiFromProject(): Promise<void> {
  applyingProjectUi = true;
  try {
    const ui = await getProjectUi<DynoUiSettings>(PERSIST_KEY_DYNO);
    ignoreTpsMin.value = ui.ignoreTpsMin;
    minRpm.value = ui.minRpm;
    smoothStrength.value = clampSmoothStrength(ui.smoothStrength);
    chartSizeOverride.height = ui.chartHeight > CHART_HEIGHT_MIN ? ui.chartHeight : null;
    settingsOpen.value = ui.settingsOpen;
  } catch {
    ignoreTpsMin.value = Boolean(state.value.ignoreTpsMin);
    minRpm.value = Number(state.value.minRpm ?? 0);
  } finally {
    applyingProjectUi = false;
  }
  await syncOptionsToRust();
  scheduleRedraw();
}

function scheduleSaveDynoUiToProject(): void {
  if (applyingProjectUi) return;
  if (saveDynoUiTimer !== 0) window.clearTimeout(saveDynoUiTimer);
  saveDynoUiTimer = window.setTimeout(() => {
    saveDynoUiTimer = 0;
    void setProjectUi(PERSIST_KEY_DYNO, buildDynoUiSettings());
  }, 400);
}

const dynoCharsChildren = shallowRef<ComponentInstance[]>([]);
const dynoCharsLoading = ref(false);
const dynoCharsError = ref<string | null>(null);
let dynoCharsLoaded = false;

const dynoCharsBasePath = computed(() => `${props.path}/dyno-chars`);

async function ensureDynoCharsPanel(): Promise<void> {
  if (dynoCharsLoaded || dynoCharsLoading.value) return;
  dynoCharsLoading.value = true;
  dynoCharsError.value = null;
  try {
    const panelId = paramStringOr("dynoCharsPanel", "generated/dynochars.panel");
    const res = await fetch(`/config/components/${panelId}.yaml`);
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
    const doc = parseYaml(await res.text()) as { children?: ComponentInstance[] };
    dynoCharsChildren.value = doc.children ?? [];
    dynoCharsLoaded = true;
  } catch (e) {
    dynoCharsError.value = e instanceof Error ? e.message : String(e);
  } finally {
    dynoCharsLoading.value = false;
  }
}

function toggleSettings(): void {
  settingsOpen.value = !settingsOpen.value;
  if (settingsOpen.value) void ensureDynoCharsPanel();
  scheduleSaveDynoUiToProject();
}

function onChartHeightChange(event: Event): void {
  const raw = Number((event.target as HTMLInputElement).value);
  if (!Number.isFinite(raw)) return;
  const h = Math.min(CHART_HEIGHT_MAX, Math.max(CHART_HEIGHT_MIN, Math.round(raw)));
  chartSizeOverride.height = h;
  scheduleSaveDynoUiToProject();
  scheduleRedraw();
}

const canvasRef = ref<HTMLCanvasElement | null>(null);
const containerRef = ref<HTMLDivElement | null>(null);

function redraw(): void {
  const canvas = canvasRef.value;
  const container = containerRef.value;
  if (!canvas || !container) return;
  const dpr = window.devicePixelRatio || 1;
  const w = measureChartWidth(container);
  const h = chartHeight.value;
  canvas.width = Math.floor(w * dpr);
  canvas.height = Math.floor(h * dpr);
  canvas.style.height = `${h}px`;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  drawDynoChart(ctx, w, h, chartPoints.value, undefined, chartPreviousPoints.value);
}

let redrawRaf = 0;
function scheduleRedraw(): void {
  if (!tabActive.value) return;
  cancelAnimationFrame(redrawRaf);
  redrawRaf = requestAnimationFrame(redraw);
}

useChartCanvasLayout(containerRef, scheduleRedraw);

watch(
  () => configSnapshot.value.values,
  () => {
    if (!ready.value || recording.value) return;
    void dispatch("reload_config");
  },
  { deep: true },
);

watch(ready, (r) => {
  if (r) void applyDynoUiFromProject();
});

watch(projectUiEpoch, () => {
  void applyDynoUiFromProject();
});

watch(workspaceResetEpoch, () => {
  void applyDynoUiFromProject();
});

watch([ignoreTpsMin, minRpm], () => {
  if (applyingProjectUi) return;
  void syncOptionsToRust();
  scheduleSaveDynoUiToProject();
});

watch(smoothStrength, (v) => {
  const c = clampSmoothStrength(v);
  if (c !== v) smoothStrength.value = c;
  scheduleRedraw();
});

watch(chartHeight, () => {
  scheduleSaveDynoUiToProject();
  scheduleRedraw();
});

watch([runPoints, previousRunPoints, chartPoints], () => scheduleRedraw(), { deep: true });

watch(tabActive, (active, wasActive) => {
  if (active && !wasActive) scheduleRedraw();
});

watch(settingsOpen, (open) => {
  if (open) void ensureDynoCharsPanel();
});

onMounted(async () => {
  await Promise.all([initOutputChannels(), initConfig(), initProject()]);
  scheduleRedraw();
});

onUnmounted(() => {
  cancelAnimationFrame(redrawRaf);
  if (saveDynoUiTimer !== 0) window.clearTimeout(saveDynoUiTimer);
});
</script>

<template>
  <div class="dyno-card">
    <p v-if="mounting" class="dyno-hint">Подключение…</p>

    <template v-else-if="ready || !hasLogic">
      <header class="dyno-header">
        <div class="dyno-status" :data-mode="statusMode">
          <span class="dyno-status-dot" aria-hidden="true" />
          <span>{{ statusLabel }}</span>
        </div>
        <button
          type="button"
          class="dyno-gear"
          :class="{ 'dyno-gear--open': settingsOpen }"
          title="Настройки dyno"
          aria-label="Настройки dyno"
          @click="toggleSettings"
        >
          ⚙
        </button>
      </header>

      <div class="dyno-metrics">
        <div class="dyno-metric">
          <span class="dyno-metric-label">RPM</span>
          <span class="dyno-metric-value">
            {{ liveRpm != null ? Math.round(liveRpm).toLocaleString("ru-RU") : "—" }}
          </span>
        </div>
        <div class="dyno-metric">
          <span class="dyno-metric-label">TPS</span>
          <span class="dyno-metric-value">
            {{ liveTps != null ? `${liveTps.toFixed(1)}%` : "—" }}
          </span>
        </div>
        <div class="dyno-metric" :class="{ 'dyno-metric--live': recording || showRunPeaks }">
          <span class="dyno-metric-label">Nm</span>
          <span class="dyno-metric-value">{{ displayTorque.toFixed(1) }}</span>
        </div>
        <div class="dyno-metric" :class="{ 'dyno-metric--live': recording || showRunPeaks }">
          <span class="dyno-metric-label">HP</span>
          <span class="dyno-metric-value">{{ displayHp.toFixed(1) }}</span>
        </div>
      </div>

      <div ref="containerRef" class="dyno-chart-wrap">
        <canvas ref="canvasRef" class="dyno-canvas" />
        <p v-if="runPoints.length === 0 && !recording && previousRunPoints.length === 0" class="dyno-chart-empty">
          Start → разгон → Stop
        </p>
      </div>

      <div class="dyno-actions">
        <button
          type="button"
          class="dyno-btn dyno-btn--toggle"
          :class="{ 'dyno-btn--toggle-recording': recording }"
          :disabled="!canToggleRecord"
          @click="toggleRecording"
        >
          {{ recording ? "Stop" : "Start" }}
        </button>
      </div>

      <Transition name="dyno-settings">
        <section v-if="settingsOpen" class="dyno-settings">
          <div class="dyno-settings-block">
            <h3 class="dyno-settings-title">Параметры записи</h3>

          <label class="dyno-check">
            <input v-model="ignoreTpsMin" type="checkbox" :disabled="recording" />
            <span>Без ограничения TPS (≥ 30%)</span>
          </label>

          <label class="dyno-field">
            <span>Мин. RPM</span>
            <input
              v-model.number="minRpm"
              type="number"
              min="0"
              max="20000"
              step="100"
              :disabled="recording"
            />
            <span class="dyno-field-hint">0 — не использовать</span>
          </label>

          <div class="dyno-field">
            <span>Сглаживание графика</span>
            <div
              ref="smoothTrackRef"
              class="dyno-smooth-track"
              :class="{ 'dyno-smooth-track--disabled': smoothDisabled }"
              role="slider"
              tabindex="0"
              aria-valuemin="0"
              :aria-valuemax="SMOOTH_MAX"
              :aria-valuenow="smoothStrength"
              aria-label="Сглаживание кривой"
              @mousedown="onSmoothTrackPointerDown"
              @keydown="onSmoothTrackKeydown"
            >
              <div class="dyno-smooth-rail" />
              <div class="dyno-smooth-fill" :style="{ width: `${smoothPct}%` }" />
              <div class="dyno-smooth-thumb" :style="{ left: `${smoothPct}%` }" />
            </div>
            <span class="dyno-field-hint">
              {{ clampSmoothStrength(smoothStrength) }} / {{ SMOOTH_MAX }} — только отображение
            </span>
          </div>

          <label class="dyno-field">
            <span>Высота графика, px</span>
            <input
              :value="chartHeight"
              type="number"
              :min="CHART_HEIGHT_MIN"
              :max="CHART_HEIGHT_MAX"
              step="20"
              @change="onChartHeightChange"
            />
          </label>

          <button
            type="button"
            class="dyno-link"
            :disabled="!canClear"
            @click="clearRun"
          >
            Очистить график
          </button>
          </div>

          <div class="dyno-settings-block">
            <h3 class="dyno-settings-title">Параметры авто (dynoChars)</h3>
            <p v-if="dynoCharsLoading" class="dyno-field-hint">Загрузка полей…</p>
            <p v-else-if="dynoCharsError" class="dyno-note dyno-note--error">{{ dynoCharsError }}</p>
            <div v-else class="dyno-chars-host">
              <ComponentHost
                v-for="(child, index) in dynoCharsChildren"
                :key="child.id ?? `${index}`"
                :instance="child"
                :path="childPath(dynoCharsBasePath, index, child)"
              />
            </div>
          </div>
        </section>
      </Transition>

      <p v-if="!connected" class="dyno-note dyno-note--warn">Подключите ECU для live output.</p>
      <p v-else-if="!configLoaded" class="dyno-note dyno-note--warn">
        Загрузите config — параметры dyno из dynoChars.
      </p>
      <p
        v-if="message || error"
        class="dyno-note"
        :class="{
          'dyno-note--error': !!error,
          'dyno-note--ok': recording && !error,
        }"
      >
        {{ error ?? message }}
      </p>
    </template>
  </div>
</template>

<style scoped>
.dyno-card {
  width: 100%;
  max-width: none;
  box-sizing: border-box;
  padding: 1.15rem 1.25rem 1.25rem;
  border-radius: var(--radius-lg, 12px);
  border: 1px solid var(--color-border);
  background: linear-gradient(
    165deg,
    var(--color-bg-elevated) 0%,
    var(--color-bg-subtle, var(--color-bg-elevated)) 100%
  );
  box-shadow: var(--shadow-card, 0 4px 24px rgba(0, 0, 0, 0.12));
  box-sizing: border-box;
}

.dyno-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  margin-bottom: 0.85rem;
}

.dyno-status {
  display: inline-flex;
  align-items: center;
  gap: 0.45rem;
  padding: 0.25rem 0.65rem;
  border-radius: 999px;
  font-size: 0.78rem;
  font-weight: 600;
  letter-spacing: 0.03em;
  text-transform: uppercase;
  background: var(--color-bg-subtle, rgba(255, 255, 255, 0.06));
  color: var(--color-text-muted);
}

.dyno-status-dot {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 50%;
  background: currentColor;
  opacity: 0.85;
}

.dyno-status[data-mode="recording"] {
  color: var(--color-accent-hover, var(--color-accent));
}

.dyno-status[data-mode="done"] {
  color: var(--color-success-text, #6ecf8a);
  background: var(--color-bg-accent-soft, rgba(110, 207, 138, 0.12));
}

.dyno-status[data-mode="offline"],
.dyno-status[data-mode="noconfig"] {
  opacity: 0.65;
}

.dyno-gear {
  width: 2.1rem;
  height: 2.1rem;
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-md, 8px);
  background: transparent;
  color: var(--color-text-muted);
  font-size: 1rem;
  line-height: 1;
  cursor: pointer;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}

.dyno-gear:hover {
  color: var(--color-text);
  border-color: var(--color-accent);
}

.dyno-gear--open {
  background: var(--color-bg-accent-soft, rgba(255, 255, 255, 0.08));
  color: var(--color-accent);
  border-color: var(--color-accent);
}

.dyno-metrics {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 0.5rem;
  margin-bottom: 0.85rem;
}

.dyno-metric {
  padding: 0.55rem 0.65rem;
  border-radius: var(--radius-md, 8px);
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  text-align: center;
}

.dyno-metric--live {
  border-color: var(--color-accent);
  background: var(--color-bg-accent-soft, rgba(255, 255, 255, 0.04));
}

.dyno-metric-label {
  display: block;
  font-size: 0.68rem;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--color-text-subtle);
  margin-bottom: 0.2rem;
}

.dyno-metric-value {
  display: block;
  font-size: 1.05rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: var(--color-text);
}

.dyno-chart-wrap {
  position: relative;
  width: 100%;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md, 8px);
  background: var(--color-bg);
  overflow: hidden;
  margin-bottom: 0.75rem;
}

.dyno-canvas {
  display: block;
  width: 100%;
}

.dyno-chart-empty {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  margin: 0;
  font-size: 0.88rem;
  color: var(--color-text-subtle);
  pointer-events: none;
}

.dyno-actions {
  display: block;
}

.dyno-btn {
  width: 100%;
  padding: 0.72rem 0.75rem;
  border-radius: var(--radius-md, 8px);
  border: 1px solid transparent;
  font-weight: 600;
  font-size: 0.88rem;
  cursor: pointer;
  transition: opacity 0.15s, transform 0.1s;
}

.dyno-btn:active:not(:disabled) {
  transform: scale(0.98);
}

.dyno-btn:disabled {
  opacity: 0.38;
  cursor: not-allowed;
}

.dyno-btn--toggle {
  background: var(--color-accent);
  color: var(--color-on-accent);
}

.dyno-btn--toggle-recording {
  background: var(--color-gray);
  color: var(--color-on-gray);
}

.dyno-settings {
  margin-top: 1rem;
  padding-top: 1rem;
  border-top: 1px solid var(--color-border);
  display: grid;
  gap: 0.85rem;
}

.dyno-settings-block {
  display: grid;
  gap: 0.75rem;
}

.dyno-settings-block + .dyno-settings-block {
  padding-top: 0.85rem;
  border-top: 1px dashed var(--color-border);
}

.dyno-chars-host {
  display: grid;
  gap: 0.65rem;
}

.dyno-chars-host :deep(.section) {
  width: auto;
  padding: 0.75rem 0.85rem;
  box-shadow: none;
}

.dyno-chars-host :deep(.section-title) {
  margin-bottom: 0.65rem;
  font-size: 0.85rem;
}

.dyno-chars-host :deep(.enum-field),
.dyno-chars-host :deep(.scalar-field),
.dyno-chars-host :deep(.string-field) {
  width: auto;
  max-width: 100%;
}

.dyno-chars-host :deep(.field-select),
.dyno-chars-host :deep(.field-input) {
  width: 100%;
  max-width: 14rem;
  box-sizing: border-box;
}

.dyno-settings-title {
  margin: 0;
  font-size: 0.82rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-text-muted);
}

.dyno-check {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.88rem;
  color: var(--color-text);
  cursor: pointer;
}

.dyno-check input {
  width: 1rem;
  height: 1rem;
}

.dyno-field {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.dyno-field > span:first-child {
  font-size: 0.72rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-text-subtle);
}

.dyno-field input[type="number"] {
  max-width: 8rem;
  padding: 0.45rem 0.55rem;
  border-radius: var(--radius-md, 8px);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 0.88rem;
}

.dyno-field-hint {
  font-size: 0.75rem;
  color: var(--color-text-subtle);
}

.dyno-smooth-track {
  position: relative;
  height: 1.25rem;
  max-width: 16rem;
  cursor: pointer;
  touch-action: none;
}

.dyno-smooth-track--disabled {
  opacity: 0.38;
  pointer-events: none;
  cursor: not-allowed;
}

.dyno-smooth-track:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: 3px;
  border-radius: 999px;
}

.dyno-smooth-rail {
  position: absolute;
  left: 0;
  right: 0;
  top: 50%;
  height: 0.3rem;
  transform: translateY(-50%);
  border-radius: 999px;
  background: var(--color-border-strong);
}

.dyno-smooth-fill {
  position: absolute;
  top: 50%;
  left: 0;
  height: 0.3rem;
  transform: translateY(-50%);
  border-radius: 999px;
  background: var(--color-accent);
  pointer-events: none;
}

.dyno-smooth-thumb {
  position: absolute;
  top: 50%;
  width: 1rem;
  height: 1rem;
  margin-left: -0.5rem;
  border-radius: 50%;
  background: var(--color-bg-elevated);
  border: 2px solid var(--color-accent);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.2);
  transform: translateY(-50%);
  pointer-events: none;
}

.dyno-note {
  margin: 0.85rem 0 0;
  font-size: 0.82rem;
  color: var(--color-text-muted);
}

.dyno-note--warn {
  color: var(--color-text-subtle);
}

.dyno-note--error {
  color: var(--color-error);
}

.dyno-note--ok {
  color: var(--color-success-text);
}

.dyno-hint {
  margin: 0;
  font-size: 0.88rem;
  color: var(--color-text-muted);
}

.dyno-settings-enter-active,
.dyno-settings-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}

.dyno-settings-enter-from,
.dyno-settings-leave-to {
  opacity: 0;
  transform: translateY(-6px);
}

@media (max-width: 520px) {
  .dyno-metrics {
    grid-template-columns: repeat(2, 1fr);
  }
}
</style>
