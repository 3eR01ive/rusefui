use serde::Serialize;

use super::threshold::rpm_bin_index;

const TPS_MIN_FOR_RUN: f64 = 30.0;
const TPS_DROP_RESET: f64 = 10.0;
const RPM_MIN_STEP: i32 = 40;
const SAMPLE_MIN_INTERVAL_SEC: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KnockRunMode {
    Idle,
    ThresholdAutotune,
    SpectrumCapture,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnockRunOptions {
    pub ignore_tps_min: bool,
    pub min_rpm: u16,
    pub cutoff_rpm: u16,
}

impl Default for KnockRunOptions {
    fn default() -> Self {
        Self {
            ignore_tps_min: true,
            min_rpm: 800,
            cutoff_rpm: 6500,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnockRunPoint {
    pub rpm: i32,
    pub knock_level: f64,
    pub threshold: f64,
}

#[derive(Debug, Clone)]
pub struct KnockRunRecorder {
    pub mode: KnockRunMode,
    options: KnockRunOptions,
    points: Vec<KnockRunPoint>,
    /// Максимальный knock level по индексу RPM-бина (для autotune).
    pub bin_peak_level: Vec<f64>,
    last_sample_sec: f64,
    last_rpm: i32,
    last_tps: f64,
    active: bool,
}

impl KnockRunRecorder {
    pub fn new(mode: KnockRunMode, options: KnockRunOptions, bin_count: usize) -> Self {
        Self {
            mode,
            options,
            points: Vec::new(),
            bin_peak_level: vec![f64::NEG_INFINITY; bin_count.max(1)],
            last_sample_sec: -1.0,
            last_rpm: 0,
            last_tps: 0.0,
            active: false,
        }
    }

    pub fn points(&self) -> &[KnockRunPoint] {
        &self.points
    }

    pub fn clear_points(&mut self) {
        self.points.clear();
        for v in &mut self.bin_peak_level {
            *v = f64::NEG_INFINITY;
        }
        self.last_sample_sec = -1.0;
        self.last_rpm = 0;
        self.last_tps = 0.0;
        self.active = false;
    }

    pub fn start(&mut self) {
        self.clear_points();
        self.last_sample_sec = -SAMPLE_MIN_INTERVAL_SEC;
        self.active = true;
    }

    pub fn stop(&mut self) {
        self.active = false;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// `true` — достигнута отсечка RPM, прогон завершён автоматически.
    pub fn on_sample(
        &mut self,
        time_sec: f64,
        rpm: i32,
        tps: f64,
        knock_level: f64,
        threshold: f64,
        rpm_bins: &[f64],
    ) -> bool {
        if !self.active {
            return false;
        }

        let opts = self.options;
        if !opts.ignore_tps_min {
            if tps < TPS_MIN_FOR_RUN {
                // Ждём газ, не обрываем прогон (стимулятор часто без TPS).
                return false;
            }
            if !self.points.is_empty() && self.last_tps - tps > TPS_DROP_RESET {
                self.active = false;
                return false;
            }
        }

        if opts.min_rpm > 0 && rpm < opts.min_rpm as i32 {
            return false;
        }

        if opts.cutoff_rpm > 0 && rpm >= opts.cutoff_rpm as i32 {
            self.active = false;
            return true;
        }

        if let Some(idx) = rpm_bin_index(rpm_bins, rpm) {
            if idx < self.bin_peak_level.len() {
                let peak = &mut self.bin_peak_level[idx];
                if knock_level.is_finite() && knock_level > *peak {
                    *peak = knock_level;
                }
            }
        }

        if time_sec - self.last_sample_sec < SAMPLE_MIN_INTERVAL_SEC {
            return false;
        }

        if self.last_rpm > 0 && (rpm - self.last_rpm).abs() < RPM_MIN_STEP {
            return false;
        }

        self.last_sample_sec = time_sec;
        self.last_rpm = rpm;
        self.last_tps = tps;

        self.points.push(KnockRunPoint {
            rpm,
            knock_level,
            threshold,
        });

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_accumulates_samples_with_relative_time() {
        let mut rec = KnockRunRecorder::new(KnockRunMode::ThresholdAutotune, KnockRunOptions::default(), 16);
        rec.start();
        let bins = [800.0, 1200.0, 1600.0, 2000.0];
        assert!(!rec.on_sample(0.0, 900, 100.0, -40.0, 0.0, &bins));
        assert!(!rec.on_sample(0.06, 950, 100.0, -39.0, 0.0, &bins));
        assert!(!rec.on_sample(0.12, 1000, 100.0, -38.0, 0.0, &bins));
        assert_eq!(rec.points().len(), 3);
    }

    #[test]
    fn bin_peak_updates_even_when_scatter_sample_is_skipped() {
        let mut rec = KnockRunRecorder::new(KnockRunMode::ThresholdAutotune, KnockRunOptions::default(), 4);
        rec.start();
        let bins = [800.0, 1200.0, 1600.0, 2000.0];
        rec.on_sample(0.0, 900, 100.0, -42.0, 0.0, &bins);
        rec.on_sample(0.02, 910, 100.0, -35.0, 0.0, &bins);
        assert_eq!(rec.points().len(), 1);
        assert_eq!(rec.bin_peak_level[0], -35.0);
    }
}
