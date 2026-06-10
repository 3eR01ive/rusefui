use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};

use crate::component::{ComponentLogic, ComponentMeta, EcuSyncOnMount, LogicComponentType};
use crate::session::EcuSession;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct IniCommandButtonViewState {
    label: String,
    command: String,
    connected: bool,
    busy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    message_is_error: bool,
}

pub struct IniCommandButtonLogic {
    session: Arc<EcuSession>,
    label: String,
    command: String,
    busy: bool,
    message: Option<String>,
    message_is_error: bool,
}

impl IniCommandButtonLogic {
    pub fn new(session: Arc<EcuSession>) -> Self {
        Self {
            session,
            label: String::new(),
            command: String::new(),
            busy: false,
            message: None,
            message_is_error: false,
        }
    }

    fn view_state(&self) -> IniCommandButtonViewState {
        IniCommandButtonViewState {
            label: self.label.clone(),
            command: self.command.clone(),
            connected: self.session.is_connected(),
            busy: self.busy,
            message: self.message.clone(),
            message_is_error: self.message_is_error,
        }
    }

    fn to_json(&self) -> Value {
        serde_json::to_value(self.view_state()).unwrap_or(json!({}))
    }

    fn apply_mount_props(&mut self, payload: &Value) {
        if let Some(label) = payload.get("label").and_then(|v| v.as_str()) {
            self.label = label.to_string();
        }
        if let Some(command) = payload.get("command").and_then(|v| v.as_str()) {
            self.command = command.to_string();
        }
    }

    fn begin_run(&mut self) -> Result<(), String> {
        if !self.session.is_connected() {
            return Err("ECU не подключена".into());
        }
        if self.command.trim().is_empty() {
            return Err("Не задана INI-команда".into());
        }
        if self.busy {
            return Err("Команда уже выполняется".into());
        }
        self.busy = true;
        self.message = None;
        self.message_is_error = false;
        Ok(())
    }

    fn finish_run(&mut self, ok: bool, error: Option<&str>) -> Result<Value, String> {
        self.busy = false;
        if ok {
            self.message = Some("Команда отправлена.".into());
            self.message_is_error = false;
            Ok(self.to_json())
        } else {
            let msg = error.unwrap_or("не удалось выполнить команду");
            self.message = Some(msg.to_string());
            self.message_is_error = true;
            Err(msg.to_string())
        }
    }
}

impl ComponentLogic for IniCommandButtonLogic {
    fn meta(&self) -> ComponentMeta {
        ComponentMeta {
            component_type: LogicComponentType::IniCommandButton.as_str().to_string(),
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
                self.apply_mount_props(&payload);
                Ok(self.to_json())
            }
            "begin_run" => {
                self.begin_run()?;
                Ok(self.to_json())
            }
            "finish_run" => {
                let ok = payload
                    .get("ok")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let error = payload.get("error").and_then(|v| v.as_str());
                self.finish_run(ok, error)
            }
            other => Err(format!("unknown action: {other}")),
        }
    }
}
