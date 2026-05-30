import { shallowRef, readonly } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface KnockSpectrogramView {
  width: number;
  height: number;
  freqStartHz: number;
  freqStepHz: number;
  pixels: Uint8Array;
}

export interface KnockSpectrogramPatch {
  width: number;
  height: number;
  freqStartHz: number;
  freqStepHz: number;
  shiftLeft: number;
  newColumns: number[];
}

/** Полный снимок (invoke) или лёгкий tick (event). */
export interface KnockScopeUiTick {
  connected: boolean;
  scopeEnabled: boolean;
  polling: boolean;
  knockScopeReady?: boolean;
  enableKnockScopeInConfig?: boolean | null;
  captureCount: number;
  sampleCount: number;
  sampleMin: number;
  sampleMax: number;
  lastByteLen: number;
  sampleRateHz: number;
  bufferDurationMs: number;
  statusMessage?: string | null;
  lastError?: string | null;
  spectrogramPatch?: KnockSpectrogramPatch | null;
  waveformChunk?: number[];
}

export interface KnockScopeSnapshot extends KnockScopeUiTick {
  samples?: number[];
  spectrogram?: {
    width: number;
    height: number;
    freqStartHz: number;
    freqStepHz: number;
    pixels?: number[];
  };
}

const emptySpectrogram = (): KnockSpectrogramView => ({
  width: 0,
  height: 64,
  freqStartHz: 4000,
  freqStepHz: 0,
  pixels: new Uint8Array(0),
});

const emptySnapshot = (): KnockScopeSnapshot => ({
  connected: false,
  scopeEnabled: false,
  polling: false,
  captureCount: 0,
  sampleCount: 0,
  sampleMin: 0,
  sampleMax: 0,
  lastByteLen: 0,
  sampleRateHz: 218_750,
  bufferDurationMs: 0,
});

const snapshot = shallowRef<KnockScopeSnapshot>(emptySnapshot());
const spectrogramView = shallowRef<KnockSpectrogramView>(emptySpectrogram());
const waveformRing = shallowRef<number[]>([]);

let spectrogramPixels = new Uint8Array(0);
let spectrogramWidth = 0;
let spectrogramHeight = 64;
let spectrogramFreqStart = 4000;
let spectrogramFreqStep = 0;
let lastCaptureCount = 0;
let ringMaxSamples = Math.round((218_750 * 500) / 1000);

function setWaveformWindowMs(windowMs: number): void {
  ringMaxSamples = Math.max(4096, Math.round((218_750 * windowMs) / 1000));
}

function resetWaveformRing(): void {
  waveformRing.value = [];
  lastCaptureCount = 0;
}

function appendWaveformChunk(tick: KnockScopeUiTick): void {
  if (!tick.scopeEnabled) {
    resetWaveformRing();
    return;
  }
  if (tick.captureCount === 0) {
    resetWaveformRing();
    return;
  }
  if (tick.captureCount <= lastCaptureCount) return;
  const chunk = tick.waveformChunk ?? [];
  if (chunk.length === 0) {
    lastCaptureCount = tick.captureCount;
    return;
  }
  let ring = waveformRing.value;
  if (ring.length === 0) {
    ring = chunk.slice();
  } else {
    ring = ring.concat(chunk);
  }
  if (ring.length > ringMaxSamples) {
    ring = ring.slice(ring.length - ringMaxSamples);
  }
  waveformRing.value = ring;
  lastCaptureCount = tick.captureCount;
}

function syncSpectrogramViewRef(): void {
  spectrogramView.value = {
    width: spectrogramWidth,
    height: spectrogramHeight,
    freqStartHz: spectrogramFreqStart,
    freqStepHz: spectrogramFreqStep,
    pixels: spectrogramPixels,
  };
}

function resetSpectrogramBuffer(): void {
  spectrogramPixels = new Uint8Array(0);
  spectrogramWidth = 0;
  spectrogramHeight = 64;
  spectrogramFreqStart = 4000;
  spectrogramFreqStep = 0;
  resetWaveformRing();
  syncSpectrogramViewRef();
}

function applySpectrogramPatch(patch: KnockSpectrogramPatch): void {
  const h = patch.height;
  if (h <= 0) return;
  spectrogramHeight = h;
  spectrogramFreqStart = patch.freqStartHz;
  spectrogramFreqStep = patch.freqStepHz;

  const shift = Math.max(0, patch.shiftLeft);
  if (shift > 0 && spectrogramWidth > 0) {
    const drop = Math.min(shift, spectrogramWidth) * h;
    spectrogramPixels = spectrogramPixels.subarray(drop);
    spectrogramWidth = Math.max(0, spectrogramWidth - Math.min(shift, spectrogramWidth));
  }

  const newBytes = patch.newColumns;
  if (newBytes.length > 0) {
    const merged = new Uint8Array(spectrogramPixels.length + newBytes.length);
    merged.set(spectrogramPixels);
    merged.set(newBytes, spectrogramPixels.length);
    spectrogramPixels = merged;
  }

  if (patch.width > 0) {
    spectrogramWidth = patch.width;
    const expected = spectrogramWidth * h;
    if (spectrogramPixels.length > expected) {
      spectrogramPixels = spectrogramPixels.subarray(spectrogramPixels.length - expected);
    }
  }
  syncSpectrogramViewRef();
}

function loadFullSpectrogram(
  spec: NonNullable<KnockScopeSnapshot["spectrogram"]> | undefined,
): void {
  if (!spec || spec.width < 1 || spec.height < 1 || !spec.pixels?.length) {
    resetSpectrogramBuffer();
    return;
  }
  spectrogramWidth = spec.width;
  spectrogramHeight = spec.height;
  spectrogramFreqStart = spec.freqStartHz;
  spectrogramFreqStep = spec.freqStepHz;
  spectrogramPixels = Uint8Array.from(spec.pixels);
  syncSpectrogramViewRef();
}

function mergeTick(tick: KnockScopeUiTick): void {
  snapshot.value = { ...snapshot.value, ...tick };
  appendWaveformChunk(tick);
  if (tick.spectrogramPatch) {
    applySpectrogramPatch(tick.spectrogramPatch);
  }
}

let unlisten: UnlistenFn | null = null;
let initPromise: Promise<void> | null = null;

/** Подписка на `knock-scope`; опрос ECU — только через `knock_scope_set_enabled`. */
export async function initKnockScope(): Promise<void> {
  if (initPromise) return initPromise;

  initPromise = (async () => {
    try {
      const snap = await invoke<KnockScopeSnapshot>("knock_scope_get_snapshot");
      snapshot.value = snap;
      loadFullSpectrogram(snap.spectrogram);
    } catch {
      /* not in tauri */
    }

    if (!unlisten) {
      unlisten = await listen<KnockScopeUiTick>("knock-scope", (event) => {
        mergeTick(event.payload);
      });
      await listen("knock-scope-reset", () => {
        resetSpectrogramBuffer();
        snapshot.value = emptySnapshot();
      });
    }
  })();

  return initPromise;
}

export async function setKnockScopeEnabled(
  enabled: boolean,
  windowMs?: number,
): Promise<KnockScopeSnapshot> {
  if (windowMs != null) {
    setWaveformWindowMs(windowMs);
  }
  if (!enabled) {
    resetSpectrogramBuffer();
  }
  const snap = await invoke<KnockScopeSnapshot>("knock_scope_set_enabled", {
    enabled,
    windowMs: windowMs ?? 500,
  });
  snapshot.value = snap;
  if (enabled) {
    loadFullSpectrogram(snap.spectrogram);
  } else {
    resetSpectrogramBuffer();
  }
  return snap;
}

export function useKnockScope() {
  return {
    snapshot: readonly(snapshot),
    spectrogramView: readonly(spectrogramView),
    waveformRing: readonly(waveformRing),
    setScopeEnabled: setKnockScopeEnabled,
    resetSpectrogramBuffer,
    setWaveformWindowMs,
  };
}
