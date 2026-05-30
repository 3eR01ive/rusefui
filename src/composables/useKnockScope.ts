import { readonly, ref, shallowRef } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { knockSpectrogramGlStats, registerKnockSpectrogramFullBuffer, resetKnockSpectrogramGlStats } from "./knockSpectrogramGl";

type GpuListener = (b64: string) => void;
const gpuListeners = new Set<GpuListener>();
let pendingGpuB64Queue: string[] = [];
let gpuRaf = 0;

function flushGpuB64(): void {
  gpuRaf = 0;
  if (pendingGpuB64Queue.length === 0) return;
  const batch = pendingGpuB64Queue;
  pendingGpuB64Queue = [];
  for (const b64 of batch) {
    for (const fn of gpuListeners) fn(b64);
  }
}

function scheduleGpuB64(b64: string): void {
  pendingGpuB64Queue.push(b64);
  if (gpuRaf !== 0) return;
  gpuRaf = requestAnimationFrame(flushGpuB64);
}

/** Подписка на бинарный heatmap из Rust; при mount — полный буфер через IPC. */
export function subscribeKnockSpectrogramGpu(listener: GpuListener): () => void {
  gpuListeners.add(listener);
  void invoke<string>("knock_scope_gpu_buffer")
    .then((b64) => {
      if (b64) listener(b64);
    })
    .catch(() => {});
  return () => gpuListeners.delete(listener);
}

async function loadFullBufferArray(): Promise<ArrayBuffer | null> {
  try {
    const b64 = await invoke<string>("knock_scope_gpu_buffer");
    if (!b64) return null;
    const bin = atob(b64);
    const buf = new ArrayBuffer(bin.length);
    const u8 = new Uint8Array(buf);
    for (let i = 0; i < bin.length; i += 1) u8[i] = bin.charCodeAt(i);
    return buf;
  } catch {
    return null;
  }
}

/** Полный GPU-снимок heatmap (row-major) для init / resync текстуры. */
export async function refreshKnockSpectrogramFullBuffer(): Promise<ArrayBuffer | null> {
  return loadFullBufferArray();
}

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
  spectrogramGpuB64?: string | null;
  spectrogramWidth?: number;
  spectrogramHeight?: number;
  spectrogramPeakHz?: number | null;
  spectrogramPatchPixelMax?: number;
  waveformChunk?: number[];
}

export interface KnockScopeSnapshot extends KnockScopeUiTick {
  samples?: number[];
  spectrogram?: {
    width: number;
    height: number;
    pixels?: number[];
  };
}

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
  spectrogramWidth: 0,
  spectrogramHeight: 0,
});

const snapshot = shallowRef<KnockScopeSnapshot>(emptySnapshot());
const spectrogramWidth = ref(0);
const spectrogramHeight = ref(0);
const spectrogramPeakHz = ref<number | null>(null);
const spectrogramPatchPixelMax = ref(0);
const waveformRing = shallowRef<number[]>([]);

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

const gpuResetListeners = new Set<() => void>();

function mergeTick(tick: KnockScopeUiTick): void {
  snapshot.value = { ...snapshot.value, ...tick };
  if (tick.spectrogramWidth != null) spectrogramWidth.value = tick.spectrogramWidth;
  if (tick.spectrogramHeight != null) spectrogramHeight.value = tick.spectrogramHeight;
  if (tick.spectrogramPeakHz != null) spectrogramPeakHz.value = tick.spectrogramPeakHz;
  if (tick.spectrogramPatchPixelMax != null) {
    spectrogramPatchPixelMax.value = tick.spectrogramPatchPixelMax;
  }
  appendWaveformChunk(tick);
  if (tick.spectrogramGpuB64) {
    scheduleGpuB64(tick.spectrogramGpuB64);
  }
}

function resetSpectrogramBuffer(): void {
  pendingGpuB64Queue = [];
  if (gpuRaf !== 0) {
    cancelAnimationFrame(gpuRaf);
    gpuRaf = 0;
  }
  spectrogramWidth.value = 0;
  spectrogramHeight.value = 0;
  spectrogramPeakHz.value = null;
  spectrogramPatchPixelMax.value = 0;
  resetWaveformRing();
  resetKnockSpectrogramGlStats();
  for (const fn of gpuResetListeners) fn();
}

/** Сброс WebGL-текстуры (knock-scope-reset / stop scope). */
export function onKnockSpectrogramGlReset(listener: () => void): () => void {
  gpuResetListeners.add(listener);
  return () => gpuResetListeners.delete(listener);
}

let unlisten: UnlistenFn | null = null;
let initPromise: Promise<void> | null = null;

/** Подписка на `knock-scope`; FFT heatmap — инкрементальные patch-пакеты из Rust. */
export async function initKnockScope(): Promise<void> {
  if (initPromise) return initPromise;

  initPromise = (async () => {
    registerKnockSpectrogramFullBuffer(loadFullBufferArray);
    try {
      const snap = await invoke<KnockScopeSnapshot>("knock_scope_get_snapshot");
      snapshot.value = snap;
      spectrogramWidth.value = snap.spectrogram?.width ?? snap.spectrogramWidth ?? 0;
      spectrogramHeight.value = snap.spectrogram?.height ?? snap.spectrogramHeight ?? 0;
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
  spectrogramWidth.value = snap.spectrogram?.width ?? snap.spectrogramWidth ?? 0;
  spectrogramHeight.value = snap.spectrogram?.height ?? snap.spectrogramHeight ?? 0;
  if (!enabled) {
    resetSpectrogramBuffer();
  }
  return snap;
}

export function useKnockScope() {
  return {
    snapshot: readonly(snapshot),
    spectrogramWidth: readonly(spectrogramWidth),
    spectrogramHeight: readonly(spectrogramHeight),
    spectrogramPeakHz: readonly(spectrogramPeakHz),
    spectrogramPatchPixelMax: readonly(spectrogramPatchPixelMax),
    spectrogramGlStats: readonly(knockSpectrogramGlStats),
    waveformRing: readonly(waveformRing),
    setScopeEnabled: setKnockScopeEnabled,
    resetSpectrogramBuffer,
    setWaveformWindowMs,
  };
}
