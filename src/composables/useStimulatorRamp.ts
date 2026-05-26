import { invoke } from "@tauri-apps/api/core";

export type RampCurve = "linear" | "smooth";

export interface StimulatorRampParams {
  idleRpm: number;
  peakRpm: number;
  rampUpSec: number;
  rampDownSec: number;
  curve: RampCurve;
  /** Интервал между шагами, мс */
  stepMs?: number;
}

function clampRpm(n: number, min: number, max: number): number {
  return Math.round(Math.min(max, Math.max(min, n)));
}

/** 0..1 → easing для кривой smooth (smoothstep). */
function easeT(t: number, curve: RampCurve): number {
  const x = Math.min(1, Math.max(0, t));
  if (curve === "smooth") {
    return x * x * (3 - 2 * x);
  }
  return x;
}

function interpolate(
  from: number,
  to: number,
  t: number,
  curve: RampCurve,
): number {
  const u = easeT(t, curve);
  return from + (to - from) * u;
}

function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(new DOMException("Aborted", "AbortError"));
      return;
    }
    const id = window.setTimeout(() => {
      signal.removeEventListener("abort", onAbort);
      if (signal.aborted) {
        reject(new DOMException("Aborted", "AbortError"));
      } else {
        resolve();
      }
    }, ms);
    const onAbort = () => {
      window.clearTimeout(id);
      reject(new DOMException("Aborted", "AbortError"));
    };
    signal.addEventListener("abort", onAbort, { once: true });
  });
}

async function rampSegment(
  from: number,
  to: number,
  durationSec: number,
  curve: RampCurve,
  stepMs: number,
  rpmMin: number,
  rpmMax: number,
  signal: AbortSignal,
  onStep: (rpm: number) => void,
): Promise<void> {
  const steps = Math.max(1, Math.round((durationSec * 1000) / stepMs));
  for (let i = 1; i <= steps; i += 1) {
    if (signal.aborted) throw new DOMException("Aborted", "AbortError");
    const rpm = clampRpm(interpolate(from, to, i / steps, curve), rpmMin, rpmMax);
    await invoke("stimulator_set_rpm", { rpm });
    onStep(rpm);
    if (i < steps) {
      await sleep(stepMs, signal);
    }
  }
}

/**
 * Разгон/сброс RPM при активной стимуляции (только `rpm N`, без disable/enable).
 */
export async function runStimulatorRamp(
  params: StimulatorRampParams,
  rpmMin: number,
  rpmMax: number,
  signal: AbortSignal,
  onStep: (rpm: number, phase: "up" | "down") => void,
): Promise<void> {
  const idle = clampRpm(params.idleRpm, rpmMin, rpmMax);
  const peak = clampRpm(params.peakRpm, rpmMin, rpmMax);
  const rampUp = Math.max(0.1, params.rampUpSec);
  const rampDown = Math.max(0.1, params.rampDownSec ?? params.rampUpSec);
  const stepMs = Math.max(50, params.stepMs ?? 100);
  const curve = params.curve;

  if (peak === idle) {
    throw new Error("Конечные RPM должны отличаться от холостых");
  }

  await rampSegment(idle, peak, rampUp, curve, stepMs, rpmMin, rpmMax, signal, (rpm) =>
    onStep(rpm, "up"),
  );
  await rampSegment(peak, idle, rampDown, curve, stepMs, rpmMin, rpmMax, signal, (rpm) =>
    onStep(rpm, "down"),
  );
}
