//! Knock tuning: прогон уровня/порога, autotune кривой, спектр частоты, momentum knock.

mod run;
mod threshold;

pub use run::{KnockRunMode, KnockRunOptions, KnockRunPoint, KnockRunRecorder};
pub use threshold::{
    apply_threshold_autotune, build_run_peak_curve, build_threshold_preview_curve, rpm_bin_index,
    ThresholdCurvePoint,
};
