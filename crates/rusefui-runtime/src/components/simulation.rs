use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use rusefi_ini::{encode_config_value, ConfigFieldKind};
use serde::Serialize;
use serde_json::{json, Value};

use crate::component::{ComponentLogic, ComponentMeta, EcuSyncOnMount, LogicComponentType};
use crate::session::EcuSession;

const TRIGGER_RPM_FIELD: &str = "triggerSimulatorRpm";
const CMD_SELF_STIMULATION: &str = "self_stimulation";
const DEFAULT_RPM: u16 = 1500;

/// Как Java console (`RpmCommand`, `IoUtil.getEnableCommand` / `getDisableCommand`):
/// `E` + `rpm N`, `E` + `enable self_stimulation` — без `C` в конфиг (см. `settings.cpp` / `trigger_emulator_algo.cpp`).
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

    /// Локальный кэш `triggerSimulatorRpm` после успешной консольной `rpm`.
    fn patch_local_trigger_rpm(&self) -> Result<(), String> {
        let ini = self.session.ini_context();
        let field = ini
            .config_fields
            .get(TRIGGER_RPM_FIELD)
            .ok_or_else(|| format!("поле {TRIGGER_RPM_FIELD} не найдено в INI"))?;
        let offset = match field {
            ConfigFieldKind::Scalar(s) => s.offset,
            _ => return Err(format!("{TRIGGER_RPM_FIELD}: ожидался scalar")),
        };
        let raw = self.session.config().page_raw();
        let encoded = encode_config_value(field, f64::from(self.rpm), &raw)
            .ok_or_else(|| format!("не удалось закодировать {TRIGGER_RPM_FIELD}"))?;
        self.session
            .config()
            .patch_page_raw(offset as usize, &encoded);
        Ok(())
    }

    fn start(&mut self) -> Result<(), String> {
        self.require_connected()?;
        self.busy = true;
        self.message = None;
        self.message_is_error = false;

        let inter_write_delay =
            Duration::from_millis(u64::from(self.session.ini_context().inter_write_delay_ms));
        let rpm = self.rpm;
        let session = Arc::clone(&self.session);

        // disable → rpm → enable: сброс hasInitTriggerEmulator и новый RPM до enable.
        let result = session.run_without_output_poll(|session| {
            session.with_link(|link| {
                link.execute_console_command(&format!("disable {CMD_SELF_STIMULATION}"))?;
                sleep(inter_write_delay);
                link.execute_console_command(&format!("rpm {rpm}"))?;
                sleep(inter_write_delay);
                link.execute_console_command(&format!("enable {CMD_SELF_STIMULATION}"))
            })
        });

        self.busy = false;
        match result {
            Ok(()) => {
                let _ = self.patch_local_trigger_rpm();
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
        self.busy = true;
        self.message = None;
        self.message_is_error = false;

        let session = Arc::clone(&self.session);
        let result = session.run_without_output_poll(|session| {
            session.with_link(|link| {
                link.execute_console_command(&format!("disable {CMD_SELF_STIMULATION}"))
            })
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
