use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdCurvePoint {
    pub rpm: f64,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdAutotuneResult {
    pub updates: Vec<(usize, f64)>,
    pub applied: usize,
}

/// Ближайший индекс RPM-бина кривой knockBaseNoise.
pub fn rpm_bin_index(rpm_bins: &[f64], rpm: i32) -> Option<usize> {
    if rpm_bins.is_empty() {
        return None;
    }
    let rpm_f = rpm as f64;
    let mut best = 0usize;
    let mut best_dist = f64::MAX;
    for (i, &b) in rpm_bins.iter().enumerate() {
        if !b.is_finite() {
            continue;
        }
        let d = (b - rpm_f).abs();
        if d < best_dist {
            best_dist = d;
            best = i;
        }
    }
    Some(best)
}

/// Поднять knockBaseNoise чуть выше записанного шума (gap в тех же единицах, что output level).
pub fn apply_threshold_autotune(
    rpm_bins: &[f64],
    bin_peak_level: &[f64],
    gap: f64,
) -> ThresholdAutotuneResult {
    let n = rpm_bins.len().min(bin_peak_level.len());
    let mut updates = Vec::new();
    for i in 0..n {
        let peak = bin_peak_level[i];
        if !peak.is_finite() || peak == f64::NEG_INFINITY {
            continue;
        }
        let thr = peak + gap.max(0.0);
        if thr.is_finite() {
            updates.push((i, thr));
        }
    }
    ThresholdAutotuneResult {
        applied: updates.len(),
        updates,
    }
}

/// Кривая knock level по RPM-бинам (пик шума на прогоне).
pub fn build_run_peak_curve(rpm_bins: &[f64], bin_peaks: &[f64]) -> Vec<ThresholdCurvePoint> {
    let n = rpm_bins.len().min(bin_peaks.len());
    let mut out = Vec::new();
    for i in 0..n {
        let rpm = rpm_bins[i];
        let peak = bin_peaks[i];
        if !rpm.is_finite() || rpm <= 0.0 {
            continue;
        }
        if peak.is_finite() && peak != f64::NEG_INFINITY {
            out.push(ThresholdCurvePoint { rpm, value: peak });
        }
    }
    out
}

/// Кривая threshold для графика: bin с пиком шума → peak+gap, остальные — из config.
pub fn build_threshold_preview_curve(
    rpm_bins: &[f64],
    bin_peaks: &[f64],
    config_base: &[f64],
    gap: f64,
) -> Vec<ThresholdCurvePoint> {
    let n = rpm_bins.len().max(bin_peaks.len()).max(config_base.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let rpm = rpm_bins.get(i).copied().unwrap_or(0.0);
        if !rpm.is_finite() || rpm <= 0.0 {
            continue;
        }
        let peak = bin_peaks.get(i).copied().unwrap_or(f64::NEG_INFINITY);
        let base = config_base.get(i).copied().unwrap_or(0.0);
        let value = if peak.is_finite() && peak != f64::NEG_INFINITY {
            peak + gap.max(0.0)
        } else {
            base
        };
        if value.is_finite() {
            out.push(ThresholdCurvePoint { rpm, value });
        }
    }
    out
}
