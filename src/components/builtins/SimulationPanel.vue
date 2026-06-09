<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import type { ComponentInstance, ComponentMeta } from "../../core/types";
import { useDataContext } from "../../core/data-context";
import { useOutputChannels } from "../../composables/useOutputChannels";
import { useRustComponent } from "../../composables/useRustComponent";
import { useInstanceBind } from "../../composables/useInstanceBind";
import { useTabFrozenDisplay } from "../../composables/useTabActivity";
import {
  initProject,
  PERSIST_KEY_SIMULATION,
  projectUiEpoch,
  registerProjectUiFlushHook,
  useProject,
  workspaceResetEpoch,
  type SimulationRampCurve,
  type SimulationUiSettings,
} from "../../composables/useProject";
import {
  runStimulatorRamp,
  type RampCurve,
} from "../../composables/useStimulatorRamp";

const props = defineProps<{
  instance: ComponentInstance;
  path: string;
  props: Record<string, unknown>;
  binding: unknown;
  meta: ComponentMeta;
}>();

const { state, dispatch, error, ready, mounting } = useRustComponent(
  props.instance,
  props.path,
);

const instanceRef = computed(() => props.instance);
const { paramStringOr } = useInstanceBind(instanceRef);
const dataCtx = useDataContext();
const { getField } = useOutputChannels();
const { getProjectUi, setProjectUi } = useProject();

const RPM_STEP = 50;

const rpmMin = computed(() => Number(state.value.rpmMin ?? 0));
const rpmMax = computed(() => Number(state.value.rpmMax ?? 30000));

const localRpm = ref(1500);
const idleRpm = ref(800);
const peakRpm = ref(4500);
const rampUpSec = ref(4);
const rampDownSec = ref(4);
const rampCurve = ref<SimulationRampCurve>("linear");
const settingsOpen = ref(false);

let applyingProjectUi = false;
let saveUiTimer = 0;

watch(
  () => state.value.rpm,
  (v) => {
    if (v != null && !rampRunning.value) localRpm.value = Number(v);
  },
  { immediate: true },
);

const sliderPct = computed(() => {
  const span = rpmMax.value - rpmMin.value;
  if (span <= 0) return 0;
  return ((localRpm.value - rpmMin.value) / span) * 100;
});

const trackRef = ref<HTMLElement | null>(null);

const connected = computed(() => dataCtx.connection.value.connected);
const active = computed(() => Boolean(state.value.active));
const busy = computed(() => Boolean(state.value.busy));
const message = computed(() => (state.value.message as string) ?? null);
const messageIsError = computed(() => Boolean(state.value.messageIsError));

const rpmFieldName = computed(() => paramStringOr("rpmField", "RPMValue"));
const ecuRpm = useTabFrozenDisplay(
  () => getField(rpmFieldName.value),
  null as number | null,
);

const rampRunning = ref(false);
const rampPhase = ref<"up" | "down" | null>(null);
const rampError = ref<string | null>(null);
const liveRampRpm = ref<number | null>(null);
let rampAbort: AbortController | null = null;

const sliderDisabled = computed(
  () => !connected.value || busy.value || active.value || rampRunning.value,
);

const canStart = computed(
  () => connected.value && !busy.value && !active.value && !rampRunning.value && localRpm.value > 0,
);
const canStop = computed(
  () => connected.value && !busy.value && active.value && !rampRunning.value,
);
const canRamp = computed(
  () =>
    connected.value &&
    active.value &&
    !busy.value &&
    !rampRunning.value &&
    peakRpm.value !== idleRpm.value,
);

const displayRpm = computed(() => (rampRunning.value ? liveRampRpm.value ?? localRpm.value : localRpm.value));

const statusMode = computed(() => {
  if (rampRunning.value) return "ramp";
  if (busy.value) return "busy";
  if (active.value) return "running";
  if (!connected.value) return "offline";
  return "idle";
});

const statusLabel = computed(() => {
  switch (statusMode.value) {
    case "ramp":
      return rampPhase.value === "down" ? "Сброс RPM" : "Разгон";
    case "busy":
      return "Команда…";
    case "running":
      return "Стимуляция";
    case "offline":
      return "Нет ECU";
    default:
      return "Готов";
  }
});

function snapRpm(raw: number): number {
  const span = rpmMax.value - rpmMin.value;
  if (span <= 0) return rpmMin.value;
  const steps = Math.floor(span / RPM_STEP);
  const idx = Math.round((raw - rpmMin.value) / RPM_STEP);
  const clamped = Math.min(steps, Math.max(0, idx));
  return rpmMin.value + clamped * RPM_STEP;
}

function rpmFromClientX(clientX: number): number {
  const el = trackRef.value;
  if (!el) return localRpm.value;
  const rect = el.getBoundingClientRect();
  if (rect.width <= 0) return localRpm.value;
  const t = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
  const raw = rpmMin.value + t * (rpmMax.value - rpmMin.value);
  return snapRpm(raw);
}

function onTrackPointerDown(event: MouseEvent): void {
  if (sliderDisabled.value) return;
  event.preventDefault();
  localRpm.value = rpmFromClientX(event.clientX);
  const onMove = (ev: MouseEvent) => {
    localRpm.value = rpmFromClientX(ev.clientX);
  };
  const onUp = () => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
    void commitRpm();
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
}

function onTrackKeydown(event: KeyboardEvent): void {
  if (sliderDisabled.value) return;
  const span = rpmMax.value - rpmMin.value;
  const step = Math.max(RPM_STEP, Math.round(span / 40 / RPM_STEP) * RPM_STEP);
  if (event.key === "ArrowRight" || event.key === "ArrowUp") {
    localRpm.value = snapRpm(localRpm.value + step);
  } else if (event.key === "ArrowLeft" || event.key === "ArrowDown") {
    localRpm.value = snapRpm(localRpm.value - step);
  } else {
    return;
  }
  event.preventDefault();
  void commitRpm();
}

function commitRpm(): Promise<void> {
  return dispatch("set_rpm", { rpm: localRpm.value }).then(() => {});
}

function buildUiSettings(): SimulationUiSettings {
  return {
    targetRpm: snapRpm(localRpm.value),
    idleRpm: snapRpm(idleRpm.value),
    peakRpm: snapRpm(peakRpm.value),
    rampUpSec: Math.min(120, Math.max(0.1, rampUpSec.value)),
    rampDownSec: Math.min(120, Math.max(0.1, rampDownSec.value)),
    rampCurve: rampCurve.value,
    settingsOpen: settingsOpen.value,
  };
}

async function applyUiFromProject(): Promise<void> {
  applyingProjectUi = true;
  try {
    const ui = await getProjectUi<SimulationUiSettings>(PERSIST_KEY_SIMULATION);
    localRpm.value = ui.targetRpm;
    idleRpm.value = ui.idleRpm;
    peakRpm.value = ui.peakRpm;
    rampUpSec.value = ui.rampUpSec;
    rampDownSec.value = ui.rampDownSec;
    rampCurve.value = ui.rampCurve;
    settingsOpen.value = ui.settingsOpen;
    if (ready.value) await commitRpm();
  } catch {
    /* defaults + state from Rust */
  } finally {
    applyingProjectUi = false;
  }
}

async function flushUiToProject(): Promise<void> {
  if (saveUiTimer !== 0) {
    window.clearTimeout(saveUiTimer);
    saveUiTimer = 0;
  }
  if (applyingProjectUi) return;
  await setProjectUi(PERSIST_KEY_SIMULATION, buildUiSettings());
}

function scheduleSaveUiToProject(): void {
  if (applyingProjectUi) return;
  if (saveUiTimer !== 0) window.clearTimeout(saveUiTimer);
  saveUiTimer = window.setTimeout(() => {
    saveUiTimer = 0;
    void flushUiToProject();
  }, 400);
}

function toggleSettings(): void {
  settingsOpen.value = !settingsOpen.value;
  scheduleSaveUiToProject();
}

function applyIdleFromTarget(): void {
  idleRpm.value = localRpm.value;
  scheduleSaveUiToProject();
}

function onNumberRpmChange(): void {
  localRpm.value = snapRpm(localRpm.value);
  void commitRpm();
  scheduleSaveUiToProject();
}

function start() {
  return dispatch("start");
}

function stop() {
  return dispatch("stop");
}

function cancelRamp(): void {
  rampAbort?.abort();
}

async function startRamp(): Promise<void> {
  if (!canRamp.value) return;
  rampError.value = null;
  rampRunning.value = true;
  rampPhase.value = "up";
  rampAbort = new AbortController();

  try {
    await runStimulatorRamp(
      {
        idleRpm: idleRpm.value,
        peakRpm: peakRpm.value,
        rampUpSec: rampUpSec.value,
        rampDownSec: rampDownSec.value,
        curve: rampCurve.value as RampCurve,
        stepMs: 100,
        rpmMin: rpmMin.value,
        rpmMax: rpmMax.value,
      },
      rampAbort.signal,
      (stepRpm, phase) => {
        liveRampRpm.value = stepRpm;
        rampPhase.value = phase;
      },
    );
    localRpm.value = idleRpm.value;
    await commitRpm();
    rampError.value = null;
  } catch (e) {
    if (e instanceof DOMException && e.name === "AbortError") {
      rampError.value = "Разгон отменён.";
    } else {
      rampError.value = e instanceof Error ? e.message : String(e);
    }
  } finally {
    rampRunning.value = false;
    rampPhase.value = null;
    liveRampRpm.value = null;
    rampAbort = null;
  }
}

watch(ready, (r) => {
  if (r) void applyUiFromProject();
});

watch(projectUiEpoch, () => {
  void applyUiFromProject();
});

watch(workspaceResetEpoch, () => {
  void applyUiFromProject();
});

watch(
  [localRpm, idleRpm, peakRpm, rampUpSec, rampDownSec, rampCurve],
  () => scheduleSaveUiToProject(),
);

let unregUiFlush: (() => void) | null = null;

onMounted(() => {
  void initProject().then(() => {
    unregUiFlush = registerProjectUiFlushHook(flushUiToProject);
    void applyUiFromProject();
  });
});

onUnmounted(() => {
  unregUiFlush?.();
  rampAbort?.abort();
  if (saveUiTimer !== 0) window.clearTimeout(saveUiTimer);
});
</script>

<template>
  <div class="sim-card">
    <p v-if="mounting" class="sim-hint">Подключение…</p>

    <template v-else-if="ready">
      <header class="sim-header">
        <div class="sim-status" :data-mode="statusMode">
          <span class="sim-status-dot" aria-hidden="true" />
          <span class="sim-status-text">{{ statusLabel }}</span>
        </div>
        <button
          type="button"
          class="sim-gear"
          :class="{ 'sim-gear--open': settingsOpen }"
          title="Настройки разгона"
          aria-label="Настройки разгона"
          @click="toggleSettings"
        >
          ⚙
        </button>
      </header>

      <div class="sim-hero">
        <p class="sim-rpm-value">{{ Math.round(displayRpm).toLocaleString("ru-RU") }}</p>
        <p class="sim-rpm-unit">RPM задано</p>
        <p v-if="ecuRpm != null && connected" class="sim-ecu">
          ECU <strong>{{ Math.round(ecuRpm).toLocaleString("ru-RU") }}</strong>
        </p>
      </div>

      <div
        ref="trackRef"
        class="sim-track"
        :class="{ 'sim-track--disabled': sliderDisabled }"
        role="slider"
        tabindex="0"
        :aria-valuemin="rpmMin"
        :aria-valuemax="rpmMax"
        :aria-valuenow="localRpm"
        aria-label="RPM стимулятора"
        @mousedown="onTrackPointerDown"
        @keydown="onTrackKeydown"
      >
        <div class="sim-track-rail" />
        <div class="sim-track-fill" :style="{ width: `${sliderPct}%` }" />
        <div class="sim-track-thumb" :style="{ left: `${sliderPct}%` }" />
      </div>

      <div class="sim-rpm-edit">
        <input
          v-model.number="localRpm"
          type="number"
          class="sim-rpm-input"
          :min="rpmMin"
          :max="rpmMax"
          step="50"
          :disabled="sliderDisabled"
          @change="onNumberRpmChange"
        />
        <span class="sim-range-label">{{ rpmMin }} – {{ rpmMax.toLocaleString("ru-RU") }}</span>
      </div>

      <div class="sim-actions">
        <button type="button" class="sim-btn sim-btn--start" :disabled="!canStart" @click="start">
          {{ busy ? "…" : "Старт" }}
        </button>
        <button type="button" class="sim-btn sim-btn--stop" :disabled="!canStop" @click="stop">
          Стоп
        </button>
        <button
          type="button"
          class="sim-btn sim-btn--ramp"
          :disabled="!canRamp && !rampRunning"
          @click="rampRunning ? cancelRamp() : startRamp()"
        >
          {{ rampRunning ? "Отмена" : "Разгон" }}
        </button>
      </div>

      <Transition name="sim-settings">
        <section v-if="settingsOpen" class="sim-settings">
          <h3 class="sim-settings-title">Профиль разгона</h3>
          <div class="sim-settings-grid">
            <label class="sim-field">
              <span>Холостые</span>
              <input v-model.number="idleRpm" type="number" :min="rpmMin" :max="rpmMax" step="50" :disabled="rampRunning" />
            </label>
            <label class="sim-field">
              <span>Пик</span>
              <input v-model.number="peakRpm" type="number" :min="rpmMin" :max="rpmMax" step="50" :disabled="rampRunning" />
            </label>
            <label class="sim-field">
              <span>Разгон, с</span>
              <input v-model.number="rampUpSec" type="number" min="0.1" max="120" step="0.5" :disabled="rampRunning" />
            </label>
            <label class="sim-field">
              <span>Сброс, с</span>
              <input v-model.number="rampDownSec" type="number" min="0.1" max="120" step="0.5" :disabled="rampRunning" />
            </label>
            <label class="sim-field sim-field--wide">
              <span>Кривая</span>
              <select v-model="rampCurve" :disabled="rampRunning">
                <option value="linear">Линейная</option>
                <option value="smooth">Плавная</option>
              </select>
            </label>
          </div>
          <button type="button" class="sim-link" :disabled="rampRunning || active" @click="applyIdleFromTarget">
            Холостые = текущий RPM
          </button>
        </section>
      </Transition>

      <p v-if="!connected" class="sim-note sim-note--warn">Подключите ECU для стимуляции.</p>
      <p v-else-if="!active && !rampRunning" class="sim-note">Сначала «Старт», затем «Разгон».</p>
      <p v-if="rampError" class="sim-note sim-note--error">{{ rampError }}</p>
      <p
        v-if="message || error"
        class="sim-note"
        :class="{ 'sim-note--error': messageIsError || !!error, 'sim-note--ok': active && !messageIsError && !error }"
      >
        {{ error ?? message }}
      </p>
    </template>
  </div>
</template>

<style scoped>
.sim-card {
  width: auto;
  max-width: 26rem;
  min-width: 16rem;
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

.sim-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  margin-bottom: 0.85rem;
}

.sim-status {
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

.sim-status-dot {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 50%;
  background: currentColor;
  opacity: 0.85;
}

.sim-status[data-mode="running"] {
  color: var(--color-success-text, #6ecf8a);
  background: var(--color-bg-accent-soft, rgba(110, 207, 138, 0.12));
}

.sim-status[data-mode="ramp"],
.sim-status[data-mode="busy"] {
  color: var(--color-accent-hover, var(--color-accent));
}

.sim-status[data-mode="offline"] {
  opacity: 0.65;
}

.sim-gear {
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

.sim-gear:hover {
  color: var(--color-text);
  border-color: var(--color-accent);
}

.sim-gear--open {
  background: var(--color-bg-accent-soft, rgba(255, 255, 255, 0.08));
  color: var(--color-accent);
  border-color: var(--color-accent);
}

.sim-hero {
  text-align: center;
  margin-bottom: 1rem;
}

.sim-rpm-value {
  margin: 0;
  font-size: clamp(2.4rem, 8vw, 3.2rem);
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
  line-height: 1;
  color: var(--color-text);
}

.sim-rpm-unit {
  margin: 0.35rem 0 0;
  font-size: 0.78rem;
  text-transform: uppercase;
  letter-spacing: 0.12em;
  color: var(--color-text-subtle);
}

.sim-ecu {
  margin: 0.5rem 0 0;
  font-size: 0.88rem;
  color: var(--color-text-muted);
}

.sim-track {
  position: relative;
  height: 1.35rem;
  margin: 0 0 0.65rem;
  cursor: pointer;
  touch-action: none;
}

.sim-track--disabled {
  opacity: 0.38;
  pointer-events: none;
  cursor: not-allowed;
}

.sim-track:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: 3px;
  border-radius: 999px;
}

.sim-track-rail {
  position: absolute;
  left: 0;
  right: 0;
  top: 50%;
  height: 0.35rem;
  transform: translateY(-50%);
  border-radius: 999px;
  background: var(--color-border-strong);
}

.sim-track-fill {
  position: absolute;
  top: 50%;
  left: 0;
  height: 0.35rem;
  transform: translateY(-50%);
  border-radius: 999px;
  background: var(--color-accent);
  pointer-events: none;
}

.sim-track-thumb {
  position: absolute;
  top: 50%;
  width: 1.15rem;
  height: 1.15rem;
  margin-left: -0.575rem;
  border-radius: 50%;
  background: var(--color-bg-elevated);
  border: 2px solid var(--color-accent);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.22);
  transform: translateY(-50%);
  pointer-events: none;
}

.sim-rpm-edit {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  margin-bottom: 1rem;
}

.sim-rpm-input {
  width: 6.5rem;
  padding: 0.45rem 0.6rem;
  border-radius: var(--radius-md, 8px);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg);
  color: var(--color-text);
  font-variant-numeric: tabular-nums;
  font-size: 0.95rem;
}

.sim-range-label {
  font-size: 0.78rem;
  color: var(--color-text-subtle);
  font-variant-numeric: tabular-nums;
}

.sim-actions {
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  gap: 0.5rem;
}

.sim-btn {
  padding: 0.62rem 0.5rem;
  border-radius: var(--radius-md, 8px);
  border: 1px solid transparent;
  font-weight: 600;
  font-size: 0.88rem;
  cursor: pointer;
  transition: opacity 0.15s, transform 0.1s;
}

.sim-btn:active:not(:disabled) {
  transform: scale(0.98);
}

.sim-btn:disabled {
  opacity: 0.38;
  cursor: not-allowed;
}

.sim-btn--start {
  background: var(--color-accent);
  color: var(--color-on-accent);
}

.sim-btn--stop {
  background: var(--color-gray);
  color: var(--color-on-gray);
}

.sim-btn--ramp {
  background: transparent;
  border-color: var(--color-border-strong);
  color: var(--color-text);
}

.sim-settings {
  margin-top: 1rem;
  padding-top: 1rem;
  border-top: 1px solid var(--color-border);
}

.sim-settings-title {
  margin: 0 0 0.65rem;
  font-size: 0.82rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-text-muted);
}

.sim-settings-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 0.65rem;
}

.sim-field {
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
}

.sim-field--wide {
  grid-column: span 2;
}

.sim-field span {
  font-size: 0.72rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-text-subtle);
}

.sim-field input,
.sim-field select {
  padding: 0.45rem 0.55rem;
  border-radius: var(--radius-md, 8px);
  border: 1px solid var(--color-border-strong);
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 0.88rem;
}

.sim-link {
  margin-top: 0.65rem;
  padding: 0;
  border: none;
  background: none;
  color: var(--color-accent);
  font-size: 0.82rem;
  cursor: pointer;
  text-decoration: underline;
  text-underline-offset: 2px;
}

.sim-link:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.sim-note {
  margin: 0.85rem 0 0;
  font-size: 0.82rem;
  color: var(--color-text-muted);
}

.sim-note--warn {
  color: var(--color-text-subtle);
}

.sim-note--error {
  color: var(--color-error);
}

.sim-note--ok {
  color: var(--color-success-text);
}

.sim-hint {
  margin: 0;
  font-size: 0.88rem;
  color: var(--color-text-muted);
}

.sim-settings-enter-active,
.sim-settings-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}

.sim-settings-enter-from,
.sim-settings-leave-to {
  opacity: 0;
  transform: translateY(-6px);
}
</style>
