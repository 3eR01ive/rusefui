use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};

use crate::component::{ComponentLogic, ComponentMeta, EcuSyncOnMount, LogicComponentType};
use crate::session::EcuSession;

const DEFAULT_RPM: u16 = 1500;

/// Как Java console (`RpmCommand`, `IoUtil`): `E` + disable/rpm/enable — без `C` в конфиг.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SimulatorViewState {
    rpm: u16,
    rpm_min: u16,
    rpm_max: u16,
    connected: bool,
    active: bool,
    busy: bool,
    message: Option<String>,
    message_is_error: bool,
}

pub struct SimulationLogic {
    session: Arc<EcuSession>,
    rpm: u16,
    rpm_min: u16,
    rpm_max: u16,
    active: bool,
    busy: bool,
    message: Option<String>,
    message_is_error: bool,
}

impl SimulationLogic {
    pub fn new(session: Arc<EcuSession>) -> Self {
        let (rpm_min, rpm_max) = (0u16, 30_000u16);
        let active = session.is_stimulation_active();
        Self {
            session,
            rpm: DEFAULT_RPM,
            rpm_min,
            rpm_max,
            active,
            busy: false,
            message: None,
            message_is_error: false,
        }
    }

    fn view_state(&self) -> SimulatorViewState {
        SimulatorViewState {
            rpm: self.rpm,
            rpm_min: self.rpm_min,
            rpm_max: self.rpm_max,
            connected: self.session.is_connected(),
            active: self.active || self.session.is_stimulation_active(),
            busy: self.busy,
            message: self.message.clone(),
            message_is_error: self.message_is_error,
        }
    }

    fn to_json(&self) -> Value {
        serde_json::to_value(self.view_state()).unwrap_or(json!({}))
    }

    fn require_connected(&self) -> Result<(), String> {
        if self.session.is_connected() {
            Ok(())
        } else {
            Err("ECU не подключена".into())
        }
    }

    fn begin_start(&mut self) -> Result<(), String> {
        self.require_connected()?;
        if self.busy {
            return Err("Операция уже выполняется".into());
        }
        self.busy = true;
        self.message = None;
        self.message_is_error = false;
        Ok(())
    }

    fn finish_start(&mut self, ok: bool, error: Option<&str>) -> Result<Value, String> {
        self.busy = false;
        if ok {
            self.active = true;
            self.message = Some(format!("Стимуляция включена на {} RPM.", self.rpm));
            self.message_is_error = false;
            Ok(self.to_json())
        } else {
            self.active = false;
            let msg = error.unwrap_or("не удалось включить стимуляцию");
            self.message = Some(msg.to_string());
            self.message_is_error = true;
            Err(msg.to_string())
        }
    }

    fn begin_stop(&mut self) -> Result<(), String> {
        self.require_connected()?;
        if self.busy {
            return Err("Операция уже выполняется".into());
        }
        self.busy = true;
        self.message = None;
        self.message_is_error = false;
        Ok(())
    }

    fn finish_stop(&mut self, ok: bool, error: Option<&str>) -> Result<Value, String> {
        self.busy = false;
        if ok {
            self.active = false;
            self.message = Some("Стимуляция выключена.".into());
            self.message_is_error = false;
            Ok(self.to_json())
        } else {
            let msg = error.unwrap_or("не удалось выключить стимуляцию");
            self.message = Some(msg.to_string());
            self.message_is_error = true;
            Err(msg.to_string())
        }
    }
}

impl ComponentLogic for SimulationLogic {
    fn meta(&self) -> ComponentMeta {
        ComponentMeta {
            component_type: LogicComponentType::Simulation.as_str().to_string(),
            has_rust_logic: true,
        }
    }

    fn ecu_sync_on_mount(&self) -> EcuSyncOnMount {
        EcuSyncOnMount::OutputPollIfConfigLoaded
    }

    fn state(&self) -> Value {
        self.to_json()
    }

    fn dispatch(&mut self, action: &str, payload: Value) -> Result<Value, String> {
        match action {
            "mount" => Ok(self.to_json()),
            "set_rpm" => {
                if let Some(rpm) = payload.get("rpm").and_then(|v| v.as_u64()) {
                    self.rpm = rpm.clamp(self.rpm_min as u64, self.rpm_max as u64) as u16;
                }
                Ok(self.to_json())
            }
            "begin_start" => {
                self.begin_start()?;
                Ok(self.to_json())
            }
            "finish_start" => {
                let ok = payload
                    .get("ok")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let error = payload.get("error").and_then(|v| v.as_str());
                self.finish_start(ok, error)
            }
            "begin_stop" => {
                self.begin_stop()?;
                Ok(self.to_json())
            }
            "finish_stop" => {
                let ok = payload
                    .get("ok")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let error = payload.get("error").and_then(|v| v.as_str());
                self.finish_stop(ok, error)
            }
            other => Err(format!("unknown action: {other}")),
        }
    }
}
