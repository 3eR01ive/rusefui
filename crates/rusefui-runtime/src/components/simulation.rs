use std::sync::Arc;

use rusefi_protocol::{
    TS_PAGE_SETTINGS, TS_SUBSYSTEM_X14, TS_X14_TRIGGER_STIMULATOR_DISABLE,
    TS_X14_TRIGGER_STIMULATOR_ENABLE,
};
use rusefi_ini::{ScalarField, ScalarType};
use serde::Serialize;
use serde_json::{json, Value};

use crate::component::{ComponentLogic, ComponentMeta, LogicComponentType};
use crate::session::EcuSession;

const TRIGGER_RPM_FIELD: &str = "triggerSimulatorRpm";
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
    trigger_rpm_field: Option<ScalarField>,
}

impl SimulationLogic {
    pub fn new(session: Arc<EcuSession>) -> Self {
        let trigger_rpm_field = session
            .ini_context()
            .config_scalars
            .get(TRIGGER_RPM_FIELD)
            .cloned();
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
            trigger_rpm_field,
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

    fn write_trigger_rpm(&self) -> Result<(), String> {
        let field = self
            .trigger_rpm_field
            .as_ref()
            .ok_or_else(|| format!("поле {TRIGGER_RPM_FIELD} не найдено в INI"))?;
        if field.ty != ScalarType::U16 {
            return Err(format!(
                "{TRIGGER_RPM_FIELD}: ожидался U16, получен {:?}",
                field.ty
            ));
        }
        let raw = (self.rpm as f64 - field.translate) / field.scale;
        let raw = raw.round().clamp(0.0, u16::MAX as f64) as u16;
        let bytes = raw.to_le_bytes();
        self.session.with_link(|link| {
            link.write_config_chunk(TS_PAGE_SETTINGS, field.offset as u16, &bytes)
        })?;
        Ok(())
    }

    fn run_ts_command(&self, index: u16) -> Result<(), String> {
        self.session.with_link(|link| link.execute_ts_command(TS_SUBSYSTEM_X14, index))
    }

    fn start(&mut self) -> Result<(), String> {
        self.require_connected()?;
        self.busy = true;
        self.message = None;
        self.message_is_error = false;

        let result: Result<(), String> = (|| {
            self.write_trigger_rpm()?;
            self.run_ts_command(TS_X14_TRIGGER_STIMULATOR_ENABLE)?;
            Ok(())
        })();

        self.busy = false;
        match result {
            Ok(()) => {
                self.active = true;
                self.message = Some(format!(
                    "Стимуляция включена на {} RPM.",
                    self.rpm
                ));
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
        self.busy = true;
        self.message = None;
        self.message_is_error = false;

        let result = self.run_ts_command(TS_X14_TRIGGER_STIMULATOR_DISABLE);

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
