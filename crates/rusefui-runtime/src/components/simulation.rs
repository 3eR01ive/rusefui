use std::sync::Arc;

use rusefi_protocol::{
    TS_SUBSYSTEM_X14, TS_X14_TRIGGER_STIMULATOR_DISABLE, TS_X14_TRIGGER_STIMULATOR_ENABLE,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::component::{ComponentLogic, ComponentMeta, LogicComponentType};
use crate::session::EcuSession;

const DEFAULT_RPM: u16 = 1500;

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
        Self {
            session,
            rpm: DEFAULT_RPM,
            rpm_min,
            rpm_max,
            active: false,
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
            active: self.active,
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

    /// INI `cmd_enable_self_stim`: `C` triggerSimulatorRpm (без burn) + `Z` subsystem X14.
    fn start(&mut self) -> Result<(), String> {
        self.require_connected()?;
        self.session.output().stop();
        self.busy = true;
        self.message = None;
        self.message_is_error = false;

        let rpm = self.rpm;
        let result = (|| {
            // TS: RPM через `C` (Java всегда шлёт page=0), enable — `Z`. Без burn до enable.
            self.session.with_link(|link| {
                let _ = link.execute_ts_command(TS_SUBSYSTEM_X14, TS_X14_TRIGGER_STIMULATOR_DISABLE);
                Ok(())
            })?;
            self.session.config().write_scalar(
                &self.session,
                "triggerSimulatorRpm",
                f64::from(rpm),
            )?;
            self.session.with_link(|link| {
                link.execute_ts_command(TS_SUBSYSTEM_X14, TS_X14_TRIGGER_STIMULATOR_ENABLE)
            })
        })();

        self.busy = false;
        match result {
            Ok(()) => {
                self.active = true;
                self.message = Some(format!("Стимуляция включена на {} RPM.", self.rpm));
                self.message_is_error = false;
                Ok(())
            }
            Err(e) => {
                self.active = false;
                self.message = Some(e.clone());
                self.message_is_error = true;
                Err(e)
            }
        }
    }

    fn stop(&mut self) -> Result<(), String> {
        self.require_connected()?;
        self.session.output().stop();
        self.busy = true;
        self.message = None;
        self.message_is_error = false;

        let result = self.session.with_link(|link| {
            link.execute_ts_command(TS_SUBSYSTEM_X14, TS_X14_TRIGGER_STIMULATOR_DISABLE)
        });

        self.busy = false;
        match result {
            Ok(()) => {
                self.active = false;
                self.message = Some("Стимуляция выключена.".into());
                self.message_is_error = false;
                Ok(())
            }
            Err(e) => {
                self.message = Some(e.clone());
                self.message_is_error = true;
                Err(e)
            }
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
            "start" => {
                self.start()?;
                Ok(self.to_json())
            }
            "stop" => {
                self.stop()?;
                Ok(self.to_json())
            }
            other => Err(format!("unknown action: {other}")),
        }
    }
}
