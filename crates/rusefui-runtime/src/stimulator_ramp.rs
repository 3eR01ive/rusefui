use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::session::EcuSession;
use crate::ui_persist::RampCurveKind;

pub const DEFAULT_RAMP_STEP_MS: u64 = 100;
const MIN_RAMP_STEP_MS: u64 = 50;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StimulatorRampParams {
    pub idle_rpm: u16,
    pub peak_rpm: u16,
    pub ramp_up_sec: f32,
    pub ramp_down_sec: f32,
    pub curve: RampCurveKind,
    pub step_ms: u64,
    pub rpm_min: u16,
    pub rpm_max: u16,
}

impl StimulatorRampParams {
    pub fn normalized(mut self) -> Result<Self, String> {
        self.step_ms = self.step_ms.max(MIN_RAMP_STEP_MS);
        self.ramp_up_sec = self.ramp_up_sec.clamp(0.1, 120.0);
        self.ramp_down_sec = self.ramp_down_sec.clamp(0.1, 120.0);
        self.idle_rpm = clamp_rpm(self.idle_rpm, self.rpm_min, self.rpm_max);
        self.peak_rpm = clamp_rpm(self.peak_rpm, self.rpm_min, self.rpm_max);
        if self.peak_rpm == self.idle_rpm {
            return Err("Конечные RPM должны отличаться от холостых".into());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StimulatorRampPhase {
    Up,
    Down,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StimulatorRampStep {
    pub rpm: u16,
    pub phase: StimulatorRampPhase,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StimulatorRampResult {
    pub ok: bool,
    pub cancelled: bool,
    pub error: Option<String>,
}

pub struct StimulatorRampRunner {
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl StimulatorRampRunner {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn start(
        &self,
        session: Arc<EcuSession>,
        params: StimulatorRampParams,
        on_step: impl Fn(StimulatorRampStep) + Send + Sync + 'static,
        on_done: impl Fn(StimulatorRampResult) + Send + Sync + 'static,
    ) -> Result<(), String> {
        if !session.is_connected() {
            return Err("ECU не подключена".into());
        }
        if !session.is_stimulation_active() {
            return Err("Стимуляция не включена".into());
        }
        if self.is_running() {
            return Err("Разгон уже выполняется".into());
        }

        let params = params.normalized()?;
        self.cancel.store(false, Ordering::SeqCst);
        self.running.store(true, Ordering::SeqCst);

        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }

        let cancel = Arc::clone(&self.cancel);
        let running = Arc::clone(&self.running);

        let handle = thread::spawn(move || {
            let result = run_stimulator_ramp(&session, &params, &cancel, &on_step);
            running.store(false, Ordering::SeqCst);
            on_done(result);
        });

        *self.thread.lock().unwrap() = Some(handle);
        Ok(())
    }

    /// Отмена и ожидание завершения фонового потока (disconnect / unmount).
    pub fn cancel_and_join(&self) {
        self.cancel();
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        self.running.store(false, Ordering::SeqCst);
        self.cancel.store(false, Ordering::SeqCst);
    }
}

enum RampFail {
    Cancelled,
    Error(String),
}

fn run_stimulator_ramp(
    session: &EcuSession,
    params: &StimulatorRampParams,
    cancel: &AtomicBool,
    on_step: &dyn Fn(StimulatorRampStep),
) -> StimulatorRampResult {
    if let Err(e) = ramp_segment(
        session,
        params.idle_rpm,
        params.peak_rpm,
        params.ramp_up_sec,
        params.curve,
        params.step_ms,
        params.rpm_min,
        params.rpm_max,
        StimulatorRampPhase::Up,
        cancel,
        on_step,
    ) {
        return fail_result(e);
    }

    match ramp_segment(
        session,
        params.peak_rpm,
        params.idle_rpm,
        params.ramp_down_sec,
        params.curve,
        params.step_ms,
        params.rpm_min,
        params.rpm_max,
        StimulatorRampPhase::Down,
        cancel,
        on_step,
    ) {
        Ok(()) => StimulatorRampResult {
            ok: true,
            cancelled: false,
            error: None,
        },
        Err(e) => fail_result(e),
    }
}

fn fail_result(e: RampFail) -> StimulatorRampResult {
    match e {
        RampFail::Cancelled => StimulatorRampResult {
            ok: false,
            cancelled: true,
            error: None,
        },
        RampFail::Error(msg) => StimulatorRampResult {
            ok: false,
            cancelled: false,
            error: Some(msg),
        },
    }
}

fn ramp_segment(
    session: &EcuSession,
    from: u16,
    to: u16,
    duration_sec: f32,
    curve: RampCurveKind,
    step_ms: u64,
    rpm_min: u16,
    rpm_max: u16,
    phase: StimulatorRampPhase,
    cancel: &AtomicBool,
    on_step: &dyn Fn(StimulatorRampStep),
) -> Result<(), RampFail> {
    let steps = ((duration_sec * 1000.0) / step_ms as f32)
        .round()
        .max(1.0) as u32;
    let step_dur = Duration::from_millis(step_ms);
    let t0 = Instant::now();

    for i in 1..=steps {
        if cancel.load(Ordering::Relaxed) {
            return Err(RampFail::Cancelled);
        }
        if !session.is_connected() {
            return Err(RampFail::Error("ECU отключена".into()));
        }
        if !session.is_stimulation_active() {
            return Err(RampFail::Error("Стимуляция выключена".into()));
        }

        let t = i as f32 / steps as f32;
        let rpm = clamp_rpm(interpolate(from, to, t, curve), rpm_min, rpm_max);
        session
            .run_stimulator_set_rpm(rpm)
            .map_err(RampFail::Error)?;
        on_step(StimulatorRampStep { rpm, phase });

        if i < steps {
            let deadline = t0 + step_dur * i;
            let now = Instant::now();
            if deadline > now {
                thread::sleep(deadline - now);
            }
        }
    }
    Ok(())
}

fn clamp_rpm(n: u16, min: u16, max: u16) -> u16 {
    n.clamp(min, max)
}

fn interpolate(from: u16, to: u16, t: f32, curve: RampCurveKind) -> u16 {
    let u = ease_t(t, curve);
    let v = f32::from(from) + (f32::from(to) - f32::from(from)) * u;
    v.round() as u16
}

fn ease_t(t: f32, curve: RampCurveKind) -> f32 {
    let x = t.clamp(0.0, 1.0);
    match curve {
        RampCurveKind::Smooth => x * x * (3.0 - 2.0 * x),
        RampCurveKind::Linear => x,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothstep_endpoints() {
        assert_eq!(ease_t(0.0, RampCurveKind::Smooth), 0.0);
        assert_eq!(ease_t(1.0, RampCurveKind::Smooth), 1.0);
    }

    #[test]
    fn interpolate_linear_midpoint() {
        assert_eq!(interpolate(1000, 2000, 0.5, RampCurveKind::Linear), 1500);
    }

    #[test]
    fn params_reject_equal_idle_peak() {
        let p = StimulatorRampParams {
            idle_rpm: 1000,
            peak_rpm: 1000,
            ramp_up_sec: 1.0,
            ramp_down_sec: 1.0,
            curve: RampCurveKind::Linear,
            step_ms: DEFAULT_RAMP_STEP_MS,
            rpm_min: 0,
            rpm_max: 8000,
        };
        assert!(p.normalized().is_err());
    }
}
