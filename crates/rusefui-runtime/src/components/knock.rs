use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use crate::component::{ComponentLogic, ComponentMeta, EcuSyncOnMount, LogicComponentType};
use crate::knock::{
    apply_threshold_autotune, build_run_peak_curve, build_threshold_preview_curve, KnockRunMode,
    KnockRunOptions, KnockRunPoint, KnockRunRecorder, ThresholdCurvePoint,
};
use crate::session::EcuSession;
use crate::sources::config::ConfigSource;
use crate::sources::knock_scope::KnockScopeSnapshot;
use crate::sources::output_channels::OutputSnapshot;

const DEFAULT_RPM_FIELD: &str = "RPMValue";
const DEFAULT_TPS_FIELD: &str = "TPSValue";
const DEFAULT_KNOCK_LEVEL_FIELD: &str = "m_knockLevel";
const DEFAULT_KNOCK_THRESHOLD_FIELD: &str = "m_knockThreshold";
const DEFAULT_LOAD_FIELD: &str = "ignitionLoad";
const DEFAULT_ADVANCE_FIELD: &str = "ignitionAdvanceCyl1";
const KNOCK_RPM_BINS_FIELD: &str = "knockNoiseRpmBins";
const KNOCK_BASE_NOISE_FIELD: &str = "knockBaseNoise";
const KNOCK_FREQUENCY_FIELD: &str = "knockFrequency";
const UI_EMIT_MIN: Duration = Duration::from_millis(50);

fn peaks_changed(prev: &[f64], new: &[f64]) -> bool {
    if prev.len() != new.len() {
        return true;
    }
    for (a, b) in prev.iter().zip(new.iter()) {
        if (*a - *b).abs() > 0.05 {
            return true;
        }
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum MomentumPhase {
    Idle,
    Waiting,
    Active,
    Done,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TempPatchState {
    active: bool,
    backups: Vec<(String, f64)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct KnockViewState {
    connected: bool,
    config_loaded: bool,
    mode: KnockRunMode,
    recording: bool,
    run_points: Vec<KnockRunPoint>,
    previous_run_points: Vec<KnockRunPoint>,
    live_knock_level: f64,
    live_threshold: f64,
    live_rpm: f64,
    threshold_gap_db: f64,
    ignore_tps_min: bool,
    min_rpm: u16,
    cutoff_rpm: u16,
    temp_detune_active: bool,
    temp_target_lambda: f64,
    temp_ignition_retard_deg: f64,
    momentum_phase: MomentumPhase,
    momentum_safe_rpm_min: u16,
    momentum_safe_rpm_max: u16,
    momentum_min_load: f64,
    momentum_advance_add_deg: f64,
    momentum_duration_ms: u32,
    detected_frequency_hz: Option<f64>,
    spectrogram_enabled: bool,
    rpm_field: String,
    tps_field: String,
    knock_level_field: String,
    knock_threshold_field: String,
    load_field: String,
    advance_field: String,
    message: Option<String>,
    preview_threshold_curve: Vec<ThresholdCurvePoint>,
    run_peak_curve: Vec<ThresholdCurvePoint>,
    previous_run_peak_curve: Vec<ThresholdCurvePoint>,
}

pub struct KnockLogic {
    session: std::sync::Arc<EcuSession>,
    recorder: Option<KnockRunRecorder>,
    run_options: KnockRunOptions,
    run_points: Vec<KnockRunPoint>,
    previous_run_points: Vec<KnockRunPoint>,
    mode: KnockRunMode,
    threshold_gap_db: f64,
    temp_target_lambda: f64,
    temp_ignition_retard_deg: f64,
    temp_patch: TempPatchState,
    momentum_phase: MomentumPhase,
    momentum_safe_rpm_min: u16,
    momentum_safe_rpm_max: u16,
    momentum_min_load: f64,
    momentum_advance_add_deg: f64,
    momentum_duration_ms: u32,
    momentum_deadline: Option<Instant>,
    detected_frequency_hz: Option<f64>,
    spectrogram_window_ms: u32,
    scope_for_run: bool,
    rpm_field: String,
    tps_field: String,
    knock_level_field: String,
    knock_threshold_field: String,
    load_field: String,
    advance_field: String,
    time_offset_sec: f64,
    live_knock_level: f64,
    live_threshold: f64,
    live_rpm: f64,
    message: Option<String>,
    dirty: bool,
    pending_stop: bool,
    pending_stop_apply_threshold: bool,
    pending_config_emit: bool,
    preview_threshold_curve: Vec<ThresholdCurvePoint>,
    run_peak_curve: Vec<ThresholdCurvePoint>,
    previous_run_peak_curve: Vec<ThresholdCurvePoint>,
    last_ui_emit: Option<Instant>,
    last_emitted_bin_peaks: Vec<f64>,
}

impl KnockLogic {
    pub fn new(session: std::sync::Arc<EcuSession>) -> Self {
        Self {
            session,
            recorder: None,
            run_options: KnockRunOptions::default(),
            run_points: Vec::new(),
            previous_run_points: Vec::new(),
            mode: KnockRunMode::Idle,
            threshold_gap_db: 3.0,
            temp_target_lambda: 0.85,
            temp_ignition_retard_deg: 8.0,
            temp_patch: TempPatchState {
                active: false,
                backups: Vec::new(),
            },
            momentum_phase: MomentumPhase::Idle,
            momentum_safe_rpm_min: 2000,
            momentum_safe_rpm_max: 3500,
            momentum_min_load: 40.0,
            momentum_advance_add_deg: 6.0,
            momentum_duration_ms: 800,
            momentum_deadline: None,
            detected_frequency_hz: None,
            spectrogram_window_ms: 500,
            scope_for_run: false,
            rpm_field: DEFAULT_RPM_FIELD.into(),
            tps_field: DEFAULT_TPS_FIELD.into(),
            knock_level_field: DEFAULT_KNOCK_LEVEL_FIELD.into(),
            knock_threshold_field: DEFAULT_KNOCK_THRESHOLD_FIELD.into(),
            load_field: DEFAULT_LOAD_FIELD.into(),
            advance_field: DEFAULT_ADVANCE_FIELD.into(),
            time_offset_sec: 0.0,
            live_knock_level: 0.0,
            live_threshold: 0.0,
            live_rpm: 0.0,
            message: None,
            dirty: true,
            pending_stop: false,
            pending_stop_apply_threshold: false,
            pending_config_emit: false,
            preview_threshold_curve: Vec::new(),
            run_peak_curve: Vec::new(),
            previous_run_peak_curve: Vec::new(),
            last_ui_emit: None,
            last_emitted_bin_peaks: Vec::new(),
        }
    }

    fn is_recording(&self) -> bool {
        self.recorder.as_ref().is_some_and(|r| r.is_active())
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_ui_emit = Some(Instant::now());
    }

    /// Не чаще ~20 Hz — live-метрики и кривые на прогоне.
    fn mark_dirty_live(&mut self) {
        let now = Instant::now();
        if self
            .last_ui_emit
            .is_some_and(|t| now.duration_since(t) < UI_EMIT_MIN)
        {
            return;
        }
        self.last_ui_emit = Some(now);
        self.dirty = true;
    }

    fn refresh_run_peak_curve(&mut self, rpm_bins: &[f64], bin_peaks: &[f64]) {
        self.run_peak_curve = build_run_peak_curve(rpm_bins, bin_peaks);
    }

    fn refresh_preview_threshold_curve(&mut self, rpm_bins: &[f64], bin_peaks: &[f64]) {
        let config_base = self
            .config()
            .get_array(KNOCK_BASE_NOISE_FIELD)
            .unwrap_or_default();
        self.preview_threshold_curve =
            build_threshold_preview_curve(rpm_bins, bin_peaks, &config_base, self.threshold_gap_db);
    }

    fn config(&self) -> &ConfigSource {
        self.session.config()
    }

    fn rpm_bins(&self) -> Vec<f64> {
        self.config()
            .get_array(KNOCK_RPM_BINS_FIELD)
            .unwrap_or_default()
    }

    fn view_state(&self) -> KnockViewState {
        KnockViewState {
            connected: self.session.is_connected(),
            config_loaded: self.config().snapshot().loaded,
            mode: self.mode,
            recording: self.recorder.as_ref().is_some_and(|r| r.is_active()),
            run_points: if self.is_recording() {
                Vec::new()
            } else {
                self.run_points.clone()
            },
            previous_run_points: self.previous_run_points.clone(),
            live_knock_level: self.live_knock_level,
            live_threshold: self.live_threshold,
            live_rpm: self.live_rpm,
            threshold_gap_db: self.threshold_gap_db,
            ignore_tps_min: self.run_options.ignore_tps_min,
            min_rpm: self.run_options.min_rpm,
            cutoff_rpm: self.run_options.cutoff_rpm,
            temp_detune_active: self.temp_patch.active,
            temp_target_lambda: self.temp_target_lambda,
            temp_ignition_retard_deg: self.temp_ignition_retard_deg,
            momentum_phase: self.momentum_phase,
            momentum_safe_rpm_min: self.momentum_safe_rpm_min,
            momentum_safe_rpm_max: self.momentum_safe_rpm_max,
            momentum_min_load: self.momentum_min_load,
            momentum_advance_add_deg: self.momentum_advance_add_deg,
            momentum_duration_ms: self.momentum_duration_ms,
            detected_frequency_hz: self.detected_frequency_hz,
            spectrogram_enabled: self.scope_for_run,
            rpm_field: self.rpm_field.clone(),
            tps_field: self.tps_field.clone(),
            knock_level_field: self.knock_level_field.clone(),
            knock_threshold_field: self.knock_threshold_field.clone(),
            load_field: self.load_field.clone(),
            advance_field: self.advance_field.clone(),
            message: self.message.clone(),
            preview_threshold_curve: self.preview_threshold_curve.clone(),
            run_peak_curve: self.run_peak_curve.clone(),
            previous_run_peak_curve: self.previous_run_peak_curve.clone(),
        }
    }

    fn to_json(&self) -> Value {
        serde_json::to_value(self.view_state()).unwrap_or(json!({}))
    }

    fn take_dirty_json(&mut self) -> Option<Value> {
        if self.dirty {
            self.dirty = false;
            Some(self.to_json())
        } else {
            None
        }
    }

    fn set_options_from_payload(&mut self, payload: &Value) {
        if let Some(v) = payload.get("ignoreTpsMin").and_then(|v| v.as_bool()) {
            self.run_options.ignore_tps_min = v;
        }
        if let Some(v) = payload.get("minRpm").and_then(|v| v.as_u64()) {
            self.run_options.min_rpm = v.min(u16::MAX as u64) as u16;
        }
        if let Some(v) = payload.get("cutoffRpm").and_then(|v| v.as_u64()) {
            self.run_options.cutoff_rpm = v.min(u16::MAX as u64) as u16;
        }
        if let Some(v) = payload.get("thresholdGapDb").and_then(|v| v.as_f64()) {
            if v.is_finite() {
                self.threshold_gap_db = v.max(0.0);
            }
        }
        if let Some(v) = payload.get("tempTargetLambda").and_then(|v| v.as_f64()) {
            if v.is_finite() {
                self.temp_target_lambda = v;
            }
        }
        if let Some(v) = payload.get("tempIgnitionRetardDeg").and_then(|v| v.as_f64()) {
            if v.is_finite() {
                self.temp_ignition_retard_deg = v;
            }
        }
        if let Some(v) = payload.get("momentumSafeRpmMin").and_then(|v| v.as_u64()) {
            self.momentum_safe_rpm_min = v.min(u16::MAX as u64) as u16;
        }
        if let Some(v) = payload.get("momentumSafeRpmMax").and_then(|v| v.as_u64()) {
            self.momentum_safe_rpm_max = v.min(u16::MAX as u64) as u16;
        }
        if let Some(v) = payload.get("momentumMinLoad").and_then(|v| v.as_f64()) {
            if v.is_finite() {
                self.momentum_min_load = v;
            }
        }
        if let Some(v) = payload.get("momentumAdvanceAddDeg").and_then(|v| v.as_f64()) {
            if v.is_finite() {
                self.momentum_advance_add_deg = v;
            }
        }
        if let Some(v) = payload.get("momentumDurationMs").and_then(|v| v.as_u64()) {
            self.momentum_duration_ms = v.min(u32::MAX as u64) as u32;
        }
        if let Some(v) = payload.get("spectrogramWindowMs").and_then(|v| v.as_u64()) {
            self.spectrogram_window_ms = v.max(50).min(5000) as u32;
        }
        self.mark_dirty();
    }

    fn backup_field(values: &HashMap<String, f64>, field: &str) -> Option<f64> {
        values.get(field).copied()
    }

    fn apply_temp_detune(&mut self) -> Result<(), String> {
        if self.temp_patch.active {
            return Ok(());
        }
        if !self.session.is_connected() {
            return Err("ECU не подключена".into());
        }
        let snap = self.config().snapshot();
        if !snap.loaded {
            return Err("Config не загружен".into());
        }

        let advance = snap
            .values
            .get(self.advance_field.as_str())
            .copied()
            .unwrap_or(15.0);
        let fixed = (advance - self.temp_ignition_retard_deg).clamp(-20.0, 60.0);

        let mut backups = Vec::new();
        for field in ["timingMode", "fixedTiming", "targetLambda"] {
            if let Some(v) = Self::backup_field(&snap.values, field) {
                backups.push((field.to_string(), v));
            }
        }

        self.config()
            .write_scalar(&self.session, "timingMode", 1.0)?;
        self.config()
            .write_scalar(&self.session, "fixedTiming", fixed)?;
        self.config().write_scalar(
            &self.session,
            "targetLambda",
            self.temp_target_lambda,
        )?;

        self.temp_patch = TempPatchState {
            active: true,
            backups,
        };
        self.message = Some(format!(
            "Временные бездетоновые настройки: λ≈{:.2}, УОЗ {fixed:.1}°",
            self.temp_target_lambda,
            fixed = fixed
        ));
        self.mark_dirty();
        Ok(())
    }

    fn restore_temp_detune(&mut self) -> Result<(), String> {
        if !self.temp_patch.active {
            return Ok(());
        }
        if self.session.is_connected() {
            for (field, value) in &self.temp_patch.backups {
                let _ = self.config().write_scalar(&self.session, field, *value);
            }
        }
        self.temp_patch = TempPatchState {
            active: false,
            backups: Vec::new(),
        };
        self.message = Some("Временные настройки восстановлены.".into());
        self.mark_dirty();
        Ok(())
    }

    fn apply_momentum_knock(&mut self, snap: &OutputSnapshot) -> Result<(), String> {
        if self.momentum_phase != MomentumPhase::Idle && self.momentum_phase != MomentumPhase::Done
        {
            return Err("Momentum knock уже выполняется".into());
        }
        if !self.session.is_connected() {
            return Err("ECU не подключена".into());
        }
        let cfg = self.config().snapshot();
        if !cfg.loaded {
            return Err("Config не загружен".into());
        }

        let rpm = snap.values.get(&self.rpm_field).copied().unwrap_or(0.0);
        let load = snap.values.get(&self.load_field).copied().unwrap_or(0.0);
        let in_zone = rpm >= self.momentum_safe_rpm_min as f64
            && rpm <= self.momentum_safe_rpm_max as f64
            && load >= self.momentum_min_load;

        if in_zone {
            self.fire_momentum_knock(snap)?;
        } else {
            self.momentum_phase = MomentumPhase::Waiting;
            self.message = Some(format!(
                "Momentum knock: ждём зону RPM {}–{} и нагрузку ≥ {:.0}%",
                self.momentum_safe_rpm_min, self.momentum_safe_rpm_max, self.momentum_min_load
            ));
        }
        self.mark_dirty();
        Ok(())
    }

    fn fire_momentum_knock(&mut self, snap: &OutputSnapshot) -> Result<(), String> {
        let advance = snap
            .values
            .get(&self.advance_field)
            .copied()
            .unwrap_or(15.0);
        let target = advance + self.momentum_advance_add_deg;

        if !self.temp_patch.active {
            let cfg = self.config().snapshot();
            let mut backups = Vec::new();
            for field in ["timingMode", "fixedTiming"] {
                if let Some(v) = Self::backup_field(&cfg.values, field) {
                    backups.push((field.to_string(), v));
                }
            }
            self.temp_patch = TempPatchState {
                active: true,
                backups,
            };
        }

        self.config()
            .write_scalar(&self.session, "timingMode", 1.0)?;
        self.config()
            .write_scalar(&self.session, "fixedTiming", target)?;

        self.momentum_phase = MomentumPhase::Active;
        self.momentum_deadline =
            Some(Instant::now() + Duration::from_millis(self.momentum_duration_ms as u64));
        self.message = Some(format!(
            "Momentum knock: УОЗ {target:.1}° на {} ms",
            self.momentum_duration_ms,
            target = target
        ));
        self.mark_dirty();
        Ok(())
    }

    fn finish_momentum(&mut self) {
        if self.momentum_phase == MomentumPhase::Active {
            let _ = self.restore_temp_detune();
        }
        self.momentum_phase = MomentumPhase::Done;
        self.momentum_deadline = None;
        self.message = Some("Momentum knock завершён.".into());
        self.mark_dirty();
    }

    fn start_scope(&mut self) -> Result<(), String> {
        if self.scope_for_run {
            return Ok(());
        }
        let session = std::sync::Arc::clone(&self.session);
        self.session.knock_scope().start(
            session,
            self.spectrogram_window_ms,
            |_| {},
        )?;
        self.scope_for_run = true;
        Ok(())
    }

    fn stop_scope(&mut self) {
        if self.scope_for_run {
            // Сначала останавливаем poll-поток, затем l+8 на ECU (не держать serial, пока thread жив).
            self.session.knock_scope().stop();
            self.session.knock_scope().disable_on_ecu(&self.session);
            self.scope_for_run = false;
        }
    }

    fn begin_run(&mut self, mode: KnockRunMode) -> Result<(), String> {
        if !self.session.is_connected() {
            return Err("ECU не подключена".into());
        }
        if !self.config().snapshot().loaded {
            return Err("Config не загружен".into());
        }
        if self.recorder.as_ref().is_some_and(|r| r.is_active()) {
            return Err("Прогон уже идёт".into());
        }

        let bins = self.rpm_bins();
        let mut recorder =
            KnockRunRecorder::new(mode, self.run_options, bins.len().max(16));
        recorder.start();
        self.time_offset_sec = self.session.output_timeline_live_sec();

        if !self.run_points.is_empty() {
            self.previous_run_points = std::mem::take(&mut self.run_points);
        } else {
            self.run_points.clear();
        }
        if !self.run_peak_curve.is_empty() {
            self.previous_run_peak_curve = std::mem::take(&mut self.run_peak_curve);
        } else {
            self.run_peak_curve.clear();
        }

        self.mode = mode;
        self.recorder = Some(recorder);
        self.detected_frequency_hz = None;
        if mode == KnockRunMode::ThresholdAutotune {
            self.preview_threshold_curve.clear();
        }
        self.last_emitted_bin_peaks.clear();
        self.last_ui_emit = None;

        if mode == KnockRunMode::SpectrumCapture {
            self.start_scope()?;
        }

        self.message = Some(match mode {
            KnockRunMode::ThresholdAutotune => {
                "Autotune threshold: запись прогона до отсечки RPM.".into()
            }
            KnockRunMode::SpectrumCapture => {
                "Запись спектрограммы на прогоне до отсечки RPM.".into()
            }
            KnockRunMode::Idle => "Прогон.".into(),
        });
        self.mark_dirty();
        Ok(())
    }

    fn apply_knock_threshold_updates(
        &mut self,
        rpm_bins: &[f64],
        peaks: &[f64],
        pairs: &[(usize, f64)],
    ) -> Result<(), String> {
        if pairs.is_empty() {
            return Ok(());
        }
        let snap = self.config().snapshot();
        if !snap.loaded {
            return Err("Config не загружен".into());
        }

        let live = self.session.is_connected() && !snap.read_only;
        if snap.read_only {
            self.config()
                .set_array_values_local(KNOCK_BASE_NOISE_FIELD, pairs)?;
        } else {
            self.config()
                .patch_array_values_snapshot(KNOCK_BASE_NOISE_FIELD, pairs)?;
            if live {
                if let Err(e) = self.config().write_array_values(
                    &self.session,
                    KNOCK_BASE_NOISE_FIELD,
                    pairs,
                ) {
                    self.refresh_preview_threshold_curve(rpm_bins, peaks);
                    self.message = Some(format!(
                        "Threshold autotune: {} точек в RAM; запись в ECU не удалась ({e}). \
                         Проверьте Burn или повторите запись.",
                        pairs.len()
                    ));
                    self.pending_config_emit = true;
                    self.mark_dirty();
                    return Ok(());
                }
            }
        }

        self.refresh_preview_threshold_curve(rpm_bins, peaks);
        self.message = Some(format!(
            "Threshold autotune: обновлено {} точек knockBaseNoise.",
            pairs.len()
        ));
        self.pending_config_emit = true;
        self.mark_dirty();
        Ok(())
    }

    fn stop_run(&mut self, apply_threshold: bool) -> Result<(), String> {
        if let Some(mut rec) = self.recorder.take() {
            rec.stop();
            self.run_points = rec.points().to_vec();
            let rpm_bins = self.rpm_bins();
            self.refresh_run_peak_curve(&rpm_bins, &rec.bin_peak_level);

            if rec.mode == KnockRunMode::ThresholdAutotune && apply_threshold {
                let mut peaks = rec.bin_peak_level.clone();
                for pt in rec.points() {
                    if let Some(idx) = crate::knock::rpm_bin_index(&rpm_bins, pt.rpm) {
                        if idx < peaks.len() && pt.knock_level.is_finite() {
                            let cur = peaks[idx];
                            if !cur.is_finite() || pt.knock_level > cur {
                                peaks[idx] = pt.knock_level;
                            }
                        }
                    }
                }
                let result = apply_threshold_autotune(&rpm_bins, &peaks, self.threshold_gap_db);
                if result.applied > 0 {
                    let pairs: Vec<(usize, f64)> = result.updates.clone();
                    self.apply_knock_threshold_updates(&rpm_bins, &peaks, &pairs)?;
                } else {
                    self.message = Some(format!(
                        "Threshold autotune: нет данных для кривой ({} точек прогона). \
                         Проверьте enableSoftwareKnock и m_knockLevel в output.",
                        self.run_points.len()
                    ));
                }
            } else if rec.mode == KnockRunMode::SpectrumCapture {
                let snap = self.session.knock_scope().snapshot();
                if let Some(hz) = snap.spectrogram_peak_hz {
                    self.detected_frequency_hz = Some(f64::from(hz));
                    self.message = Some(format!(
                        "Пик шума: {:.0} Hz (примените кнопкой «Применить частоту»).",
                        hz
                    ));
                }
            } else {
                self.message = Some(format!("Прогон: {} точек.", self.run_points.len()));
            }
        }

        if self.mode == KnockRunMode::SpectrumCapture {
            self.stop_scope();
        }
        self.mode = KnockRunMode::Idle;
        let _ = self.restore_temp_detune();
        self.mark_dirty();
        Ok(())
    }

    fn apply_detected_frequency(&mut self) -> Result<(), String> {
        let Some(hz) = self.detected_frequency_hz else {
            return Err("Частота ещё не определена — выполните прогон со спектрограммой.".into());
        };
        if !self.session.is_connected() {
            return Err("ECU не подключена".into());
        }
        self.config()
            .write_scalar(&self.session, KNOCK_FREQUENCY_FIELD, hz)?;
        self.message = Some(format!("knockFrequency = {hz:.0} Hz записано в config."));
        self.mark_dirty();
        Ok(())
    }

    fn process_output(&mut self, snap: &OutputSnapshot) -> bool {
        let rpm = snap.values.get(&self.rpm_field).copied();
        let tps = snap.values.get(&self.tps_field).copied();
        let level = snap.values.get(&self.knock_level_field).copied();
        let thr = snap.values.get(&self.knock_threshold_field).copied();

        if let Some(v) = level {
            if v.is_finite() {
                self.live_knock_level = v;
            }
        }
        if let Some(v) = thr {
            if v.is_finite() {
                self.live_threshold = v;
            }
        }
        if let Some(v) = rpm {
            if v.is_finite() {
                self.live_rpm = v;
            }
        }

        if self.momentum_phase == MomentumPhase::Waiting {
            if let (Some(r), Some(l)) = (rpm, snap.values.get(&self.load_field).copied()) {
                if r >= self.momentum_safe_rpm_min as f64
                    && r <= self.momentum_safe_rpm_max as f64
                    && l >= self.momentum_min_load
                {
                    let _ = self.fire_momentum_knock(snap);
                }
            }
        }

        if self.momentum_phase == MomentumPhase::Active {
            if self
                .momentum_deadline
                .is_some_and(|d| Instant::now() >= d)
            {
                self.finish_momentum();
            }
        }

        let (Some(rpm), Some(level)) = (rpm, level) else {
            self.mark_dirty_live();
            return false;
        };

        // TPS нужен только если включена проверка газа.
        if !self.run_options.ignore_tps_min && tps.is_none() {
            self.mark_dirty_live();
            return false;
        }
        let tps = tps.unwrap_or(100.0);
        let thr = thr.unwrap_or(0.0);

        let time_sec = snap.timeline_live_sec - self.time_offset_sec;
        let bins = self.rpm_bins();

        let Some(rec) = self.recorder.as_mut() else {
            self.mark_dirty_live();
            return false;
        };
        if !rec.is_active() {
            self.mark_dirty_live();
            return false;
        }

        let cutoff = rec.on_sample(
            time_sec,
            rpm.round() as i32,
            tps,
            level,
            thr,
            &bins,
        );
        let preview_peaks = if rec.mode == KnockRunMode::ThresholdAutotune {
            Some(rec.bin_peak_level.clone())
        } else {
            None
        };

        if cutoff {
            self.pending_stop = true;
            self.pending_stop_apply_threshold = self
                .recorder
                .as_ref()
                .is_some_and(|r| r.mode == KnockRunMode::ThresholdAutotune);
        }
        if let Some(peaks) = preview_peaks {
            if peaks_changed(&self.last_emitted_bin_peaks, &peaks) {
                self.refresh_preview_threshold_curve(&bins, &peaks);
                self.refresh_run_peak_curve(&bins, &peaks);
                self.last_emitted_bin_peaks = peaks;
            }
        }
        self.mark_dirty_live();

        true
    }

    fn flush_pending_stop(&mut self) -> bool {
        if !self.pending_stop {
            return false;
        }
        self.pending_stop = false;
        let apply = self.pending_stop_apply_threshold;
        self.pending_stop_apply_threshold = false;
        if let Err(e) = self.stop_run(apply) {
            self.message = Some(format!("Остановка прогона: {e}"));
            self.mark_dirty();
        }
        true
    }

    fn process_knock_scope(&mut self, snap: &KnockScopeSnapshot) -> bool {
        if self.mode != KnockRunMode::SpectrumCapture {
            return false;
        }
        let Some(hz) = snap.spectrogram_peak_hz else {
            return false;
        };
        let prev = self.detected_frequency_hz;
        self.detected_frequency_hz = Some(f64::from(hz));
        prev != self.detected_frequency_hz
    }
}

impl ComponentLogic for KnockLogic {
    fn meta(&self) -> ComponentMeta {
        ComponentMeta {
            component_type: LogicComponentType::Knock.as_str().to_string(),
            has_rust_logic: true,
        }
    }

    fn ecu_sync_on_mount(&self) -> EcuSyncOnMount {
        EcuSyncOnMount::None
    }

    fn state(&self) -> Value {
        self.to_json()
    }

    fn dispatch(&mut self, action: &str, payload: Value) -> Result<Value, String> {
        match action {
            "mount" => {
                self.mark_dirty();
                Ok(self.to_json())
            }
            "unmount" => {
                let _ = self.stop_run(false);
                self.stop_scope();
                let _ = self.restore_temp_detune();
                Ok(self.to_json())
            }
            "set_channels" => {
                if let Some(f) = payload.get("rpmField").and_then(|v| v.as_str()) {
                    if !f.is_empty() {
                        self.rpm_field = f.to_string();
                    }
                }
                if let Some(f) = payload.get("tpsField").and_then(|v| v.as_str()) {
                    if !f.is_empty() {
                        self.tps_field = f.to_string();
                    }
                }
                if let Some(f) = payload.get("knockLevelField").and_then(|v| v.as_str()) {
                    if !f.is_empty() {
                        self.knock_level_field = f.to_string();
                    }
                }
                if let Some(f) = payload.get("knockThresholdField").and_then(|v| v.as_str()) {
                    if !f.is_empty() {
                        self.knock_threshold_field = f.to_string();
                    }
                }
                if let Some(f) = payload.get("loadField").and_then(|v| v.as_str()) {
                    if !f.is_empty() {
                        self.load_field = f.to_string();
                    }
                }
                if let Some(f) = payload.get("advanceField").and_then(|v| v.as_str()) {
                    if !f.is_empty() {
                        self.advance_field = f.to_string();
                    }
                }
                self.mark_dirty();
                Ok(self.to_json())
            }
            "set_options" => {
                self.set_options_from_payload(&payload);
                Ok(self.to_json())
            }
            "apply_temp_detune" => {
                self.apply_temp_detune()?;
                Ok(self.to_json())
            }
            "restore_temp_detune" => {
                self.restore_temp_detune()?;
                Ok(self.to_json())
            }
            "start_threshold_autotune" => {
                self.begin_run(KnockRunMode::ThresholdAutotune)?;
                Ok(self.to_json())
            }
            "stop_run" => {
                let apply = payload
                    .get("applyThreshold")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(self.mode == KnockRunMode::ThresholdAutotune);
                self.stop_run(apply)?;
                self.pending_config_emit = false;
                Ok(self.to_json())
            }
            "start_spectrum_run" => {
                self.begin_run(KnockRunMode::SpectrumCapture)?;
                Ok(self.to_json())
            }
            "start_momentum_knock" => {
                let snap = self.session.output().snapshot();
                self.apply_momentum_knock(&snap)?;
                Ok(self.to_json())
            }
            "apply_frequency" => {
                self.apply_detected_frequency()?;
                Ok(self.to_json())
            }
            "clear" => {
                self.run_points.clear();
                self.previous_run_points.clear();
                self.run_peak_curve.clear();
                self.previous_run_peak_curve.clear();
                self.preview_threshold_curve.clear();
                self.detected_frequency_hz = None;
                self.message = None;
                self.mark_dirty();
                Ok(self.to_json())
            }
            other => Err(format!("unknown action: {other}")),
        }
    }

    fn feed_output(&mut self, snap: &OutputSnapshot) -> Option<Value> {
        self.process_output(snap);
        let stopped = self.flush_pending_stop();
        let emit_config = self.pending_config_emit;
        if emit_config {
            self.pending_config_emit = false;
        }
        if stopped || self.dirty {
            let mut json = self.take_dirty_json()?;
            if emit_config {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("configUpdated".into(), Value::Bool(true));
                }
            }
            return Some(json);
        }
        None
    }

    fn feed_knock_scope(&mut self, snap: &KnockScopeSnapshot) -> Option<Value> {
        if self.process_knock_scope(snap) {
            return self.take_dirty_json();
        }
        None
    }
}
