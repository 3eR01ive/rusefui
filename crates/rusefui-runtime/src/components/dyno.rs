use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};

use crate::component::{ComponentLogic, ComponentMeta, EcuSyncOnMount, LogicComponentType};
use crate::dyno::{
    dyno_config_from_values, DynoConfig, DynoRunOptions, DynoRunPoint, DynoView, DEFAULT_DYNO_CONFIG,
};
use crate::session::EcuSession;

/// Параметры расчёта приходят из настроек компонента (DynoUiSettings), а НЕ из
/// config MCU. Ключи payload совпадают с именами полей INI (dynoRpmStep и т.д.).
fn dyno_config_from_payload(payload: &Value) -> DynoConfig {
    let mut map = std::collections::HashMap::new();
    if let Some(obj) = payload.as_object() {
        for (k, v) in obj {
            if let Some(n) = v.as_f64() {
                map.insert(k.clone(), n);
            }
        }
    }
    dyno_config_from_values(&map)
}
use crate::sources::output_channels::OutputSnapshot;

const DEFAULT_RPM_FIELD: &str = "RPMValue";
const DEFAULT_TPS_FIELD: &str = "TPSValue";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DynoViewState {
    connected: bool,
    config_loaded: bool,
    recording: bool,
    run_points: Vec<DynoRunPoint>,
    previous_run_points: Vec<DynoRunPoint>,
    current_torque: f64,
    current_hp: f64,
    ignore_tps_min: bool,
    min_rpm: u16,
    rpm_field: String,
    tps_field: String,
    message: Option<String>,
}

pub struct DynoLogic {
    session: Arc<EcuSession>,
    view: DynoView,
    /// Текущие параметры расчёта (из настроек компонента).
    config: DynoConfig,
    recording: bool,
    run_points: Vec<DynoRunPoint>,
    previous_run_points: Vec<DynoRunPoint>,
    run_options: DynoRunOptions,
    rpm_field: String,
    tps_field: String,
    time_offset_sec: f64,
    last_sample_sec: f64,
    message: Option<String>,
    dirty: bool,
}

impl DynoLogic {
    pub fn new(session: Arc<EcuSession>) -> Self {
        // Параметры по умолчанию; реальные приходят из настроек компонента
        // (set_dyno_config). Config MCU намеренно не читаем.
        let cfg = DEFAULT_DYNO_CONFIG;
        Self {
            session,
            view: DynoView::new(cfg),
            config: cfg,
            recording: false,
            run_points: Vec::new(),
            previous_run_points: Vec::new(),
            run_options: DynoRunOptions::default(),
            rpm_field: DEFAULT_RPM_FIELD.into(),
            tps_field: DEFAULT_TPS_FIELD.into(),
            time_offset_sec: 0.0,
            last_sample_sec: -1.0,
            message: None,
            dirty: true,
        }
    }

    fn apply_config(&mut self) {
        self.view.update_config(self.config);
    }

    fn view_state(&self) -> DynoViewState {
        DynoViewState {
            connected: self.session.is_connected(),
            config_loaded: self.session.config().snapshot().loaded,
            recording: self.recording,
            run_points: self.run_points.clone(),
            previous_run_points: self.previous_run_points.clone(),
            current_torque: self.view.current_torque,
            current_hp: self.view.current_hp,
            ignore_tps_min: self.run_options.ignore_tps_min,
            min_rpm: self.run_options.min_rpm,
            rpm_field: self.rpm_field.clone(),
            tps_field: self.tps_field.clone(),
            message: self.message.clone(),
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

    /// Лёгкий патч при записи: одна новая точка без клонирования всего `runPoints`.
    fn take_recording_delta(&mut self) -> Option<Value> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        let last = self.run_points.last()?;
        Some(json!({
            "dynoDelta": true,
            "connected": self.session.is_connected(),
            "configLoaded": self.session.config().snapshot().loaded,
            "recording": true,
            "runPointsLen": self.run_points.len(),
            "lastRunPoint": last,
            "currentTorque": self.view.current_torque,
            "currentHp": self.view.current_hp,
            "ignoreTpsMin": self.run_options.ignore_tps_min,
            "minRpm": self.run_options.min_rpm,
            "rpmField": self.rpm_field,
            "tpsField": self.tps_field,
            "message": self.message,
        }))
    }

    fn set_options_from_payload(&mut self, payload: &Value) {
        if let Some(v) = payload.get("ignoreTpsMin").and_then(|v| v.as_bool()) {
            self.run_options.ignore_tps_min = v;
        }
        if let Some(v) = payload.get("minRpm").and_then(|v| v.as_u64()) {
            self.run_options.min_rpm = v.min(u16::MAX as u64) as u16;
        }
        self.view.set_run_options(self.run_options);
        self.dirty = true;
    }

    fn recording_hint(&self) -> String {
        let mut parts = Vec::new();
        if !self.run_options.ignore_tps_min {
            parts.push("TPS ≥ 30%".to_string());
        }
        if self.run_options.min_rpm > 0 {
            parts.push(format!("RPM ≥ {}", self.run_options.min_rpm));
        }
        if parts.is_empty() {
            "Запись: разгон по RPM (ограничения TPS/RPM сняты).".into()
        } else {
            format!("Запись: {}, без резкого сброса газа.", parts.join(", "))
        }
    }

    fn process_output(&mut self, snap: &OutputSnapshot) -> bool {
        if !self.recording || !snap.connected {
            return false;
        }

        let rpm = snap.values.get(&self.rpm_field).copied();
        let tps = snap.values.get(&self.tps_field).copied();
        let (Some(rpm), Some(tps)) = (rpm, tps) else {
            return false;
        };

        let time_sec = snap.timeline_live_sec - self.time_offset_sec;
        if time_sec <= self.last_sample_sec {
            return false;
        }
        self.last_sample_sec = time_sec;

        if let Some(point) = self.view.on_rpm(rpm.round() as i32, time_sec, tps) {
            self.run_points.push(point);
            self.dirty = true;
            return true;
        }
        false
    }
}

impl ComponentLogic for DynoLogic {
    fn meta(&self) -> ComponentMeta {
        ComponentMeta {
            component_type: LogicComponentType::Dyno.as_str().to_string(),
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
                self.dirty = true;
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
                self.dirty = true;
                Ok(self.to_json())
            }
            "set_options" => {
                self.set_options_from_payload(&payload);
                if self.recording {
                    self.message = Some(self.recording_hint());
                }
                Ok(self.to_json())
            }
            "set_dyno_config" => {
                self.config = dyno_config_from_payload(&payload);
                // Во время записи кривую не пересчитываем — применим на старте.
                if !self.recording {
                    self.apply_config();
                }
                self.dirty = true;
                Ok(self.to_json())
            }
            "reload_config" => {
                if !self.recording {
                    self.apply_config();
                }
                self.dirty = true;
                Ok(self.to_json())
            }
            "start_run" => {
                if !self.session.is_connected() {
                    return Err("ECU не подключена".into());
                }
                if !self.session.config().snapshot().loaded {
                    return Err("Config не загружен".into());
                }
                if self.recording {
                    return Err("Запись уже идёт".into());
                }
                self.apply_config();
                self.view.set_run_options(self.run_options);
                self.view.reset();
                if !self.run_points.is_empty() {
                    self.previous_run_points = std::mem::take(&mut self.run_points);
                } else {
                    self.run_points.clear();
                }
                self.time_offset_sec = self.session.output_timeline_live_sec();
                self.last_sample_sec = -1.0;
                self.recording = true;
                self.message = Some(self.recording_hint());
                self.dirty = true;
                Ok(self.to_json())
            }
            "stop_run" => {
                if !self.recording {
                    return Ok(self.to_json());
                }
                self.recording = false;
                self.message = Some(if self.run_points.is_empty() {
                    format!(
                        "Запись остановлена без точек ({}).",
                        self.recording_hint().trim_start_matches("Запись: ")
                    )
                } else {
                    format!("Готово: {} точек.", self.run_points.len())
                });
                self.dirty = true;
                Ok(self.to_json())
            }
            "clear" => {
                self.view.reset();
                self.run_points.clear();
                self.previous_run_points.clear();
                self.last_sample_sec = -1.0;
                self.message = None;
                self.dirty = true;
                Ok(self.to_json())
            }
            other => Err(format!("unknown action: {other}")),
        }
    }

    fn feed_output(&mut self, snap: &OutputSnapshot) -> Option<Value> {
        if self.process_output(snap) {
            if self.recording {
                return self.take_recording_delta();
            }
            return self.take_dirty_json();
        }
        None
    }
}
