import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type RampCurve = "linear" | "smooth";

export interface StimulatorRampParams {
  idleRpm: number;
  peakRpm: number;
  rampUpSec: number;
  rampDownSec: number;
  curve: RampCurve;
  /** Интервал между шагами, мс */
  stepMs?: number;
  rpmMin: number;
  rpmMax: number;
}

interface StimulatorRampStepEvent {
  rpm: number;
  phase: "up" | "down";
}

interface StimulatorRampFinishedEvent {
  ok: boolean;
  cancelled: boolean;
  error?: string | null;
}

/**
 * Разгон/сброс RPM в фоновом потоке Rust — шаг по wall-clock, не зависит от UI.
 */
export function runStimulatorRamp(
  params: StimulatorRampParams,
  signal: AbortSignal,
  onStep: (rpm: number, phase: "up" | "down") => void,
): Promise<void> {
  if (signal.aborted) {
    return Promise.reject(new DOMException("Aborted", "AbortError"));
  }

  return new Promise((resolve, reject) => {
    const unlisteners: UnlistenFn[] = [];
    let settled = false;

    const finish = (fn: () => void) => {
      if (settled) return;
      settled = true;
      for (const u of unlisteners) u();
      signal.removeEventListener("abort", onAbort);
      fn();
    };

    const onAbort = () => {
      void invoke("stimulator_ramp_cancel").catch(() => {});
    };
    signal.addEventListener("abort", onAbort, { once: true });

    void (async () => {
      try {
        unlisteners.push(
          await listen<StimulatorRampStepEvent>("stimulator-ramp-step", (ev) => {
            onStep(ev.payload.rpm, ev.payload.phase);
          }),
        );
        unlisteners.push(
          await listen<StimulatorRampFinishedEvent>("stimulator-ramp-finished", (ev) => {
            const { ok, cancelled, error } = ev.payload;
            if (cancelled || signal.aborted) {
              finish(() =>
                reject(new DOMException("Aborted", "AbortError")),
              );
              return;
            }
            if (!ok || error) {
              finish(() => reject(new Error(error ?? "Ошибка разгона")));
              return;
            }
            finish(() => resolve());
          }),
        );

        if (signal.aborted) {
          finish(() => reject(new DOMException("Aborted", "AbortError")));
          return;
        }

        await invoke("stimulator_ramp_start", {
          params: {
            idleRpm: params.idleRpm,
            peakRpm: params.peakRpm,
            rampUpSec: params.rampUpSec,
            rampDownSec: params.rampDownSec,
            curve: params.curve,
            stepMs: params.stepMs,
            rpmMin: params.rpmMin,
            rpmMax: params.rpmMax,
          },
        });
      } catch (e) {
        finish(() => reject(e));
      }
    })();
  });
}

export function cancelStimulatorRamp(): Promise<void> {
  return invoke("stimulator_ramp_cancel");
}
