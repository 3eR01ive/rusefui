//! Редактор Lua-скрипта ECU (поле string в config page, как Java `LuaScriptPanel`).

use std::sync::Arc;

use rusefi_ini::ConfigFieldKind;
use serde::Serialize;
use serde_json::{json, Value};

use crate::component::{ComponentLogic, ComponentMeta, EcuSyncOnMount, LogicComponentType};
use crate::session::EcuSession;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LuaScriptViewState {
    connected: bool,
    busy: bool,
    script: String,
    max_bytes: usize,
    script_field: Option<String>,
    message: Option<String>,
}

pub struct LuaScriptLogic {
    session: Arc<EcuSession>,
    script_field: Option<String>,
    script: String,
    max_bytes: usize,
    busy: bool,
    message: Option<String>,
}

impl LuaScriptLogic {
    pub fn new(session: Arc<EcuSession>) -> Self {
        Self {
            session,
            script_field: None,
            script: String::new(),
            max_bytes: 0,
            busy: false,
            message: None,
        }
    }

    fn config(&self) -> &crate::sources::config::ConfigSource {
        self.session.config()
    }

    fn set_field_from_payload(&mut self, payload: &Value) {
        if let Some(v) = payload.get("scriptField").and_then(|v| v.as_str()) {
            if !v.is_empty() {
                self.script_field = Some(v.to_string());
            }
        }
        self.refresh_max_bytes();
    }

    fn refresh_max_bytes(&mut self) {
        let Some(ref name) = self.script_field else {
            self.max_bytes = 0;
            return;
        };
        let ini = self.session.ini_context();
        self.max_bytes = ini
            .config_fields
            .get(name)
            .and_then(|f| match f {
                ConfigFieldKind::String(s) => Some(s.length as usize),
                _ => None,
            })
            .unwrap_or(0);
    }

    fn require_field(&self) -> Result<&str, String> {
        self.script_field
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "scriptField не задан (YAML props)".into())
    }

    fn read_from_snapshot(&mut self) -> Result<(), String> {
        let field = self.require_field()?;
        let snap = self.config().snapshot();
        if !snap.loaded {
            self.script.clear();
            return Err("Config не загружен".into());
        }
        self.script = snap
            .string_values
            .get(field)
            .cloned()
            .unwrap_or_default();
        self.message = Some("Прочитано из снимка config".into());
        Ok(())
    }

    fn read_from_ecu(&mut self) -> Result<(), String> {
        self.require_field()?;
        if !self.session.is_connected() {
            return self.read_from_snapshot();
        }
        self.busy = true;
        self.message = None;
        let result = match self.config().reload_page_from_ecu(&self.session) {
            Ok(()) => self.read_from_snapshot(),
            Err(e) => Err(e),
        };
        self.busy = false;
        result
    }

    fn write_to_ecu(&mut self, text: &str) -> Result<(), String> {
        let field = self
            .script_field
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "scriptField не задан (YAML props)".to_string())?;
        if self.max_bytes > 0 && text.len() >= self.max_bytes {
            return Err(format!(
                "Скрипт {} байт — лимит поля {} байт (нужен запас под \\0)",
                text.len(),
                self.max_bytes
            ));
        }
        if !self.session.is_connected() {
            return Err("ECU не подключена".into());
        }
        let snap = self.config().snapshot();
        if snap.read_only {
            return Err("Config только для чтения".into());
        }

        self.busy = true;
        self.message = None;
        let write_result = self
            .config()
            .write_string(&self.session, &field, text)
            .and_then(|_| self.session.run_console_command("luareset"));
        self.busy = false;
        write_result?;
        self.script = text.to_string();
        self.message = Some("Записано в RAM ECU, luareset выполнен".into());
        Ok(())
    }

    fn burn(&mut self) -> Result<(), String> {
        if !self.session.is_connected() {
            return Err("ECU не подключена".into());
        }
        self.busy = true;
        self.message = None;
        let result = self.config().burn_to_flash(&self.session).map(|_| {
            self.message = Some("Burn во flash выполнен".into());
        });
        self.busy = false;
        result
    }

    fn reset_lua(&mut self) -> Result<(), String> {
        if !self.session.is_connected() {
            return Err("ECU не подключена".into());
        }
        self.busy = true;
        let result = self.session.run_console_command("luareset").map(|_| {
            self.message = Some("luareset выполнен".into());
        });
        self.busy = false;
        result
    }

    fn view_state(&self) -> LuaScriptViewState {
        LuaScriptViewState {
            connected: self.session.is_connected(),
            busy: self.busy,
            script: self.script.clone(),
            max_bytes: self.max_bytes,
            script_field: self.script_field.clone(),
            message: self.message.clone(),
        }
    }

    fn to_json(&self) -> Value {
        serde_json::to_value(self.view_state()).unwrap_or(json!({}))
    }
}

// ---------------------------------------------------------------------------
// Standalone ECU helpers (used by project script commands)
// ---------------------------------------------------------------------------

pub fn ecu_script_read(session: &Arc<EcuSession>, script_field: &str) -> Result<String, String> {
    if session.is_connected() {
        session.config().reload_page_from_ecu(session)?;
    }
    let snap = session.config().snapshot();
    if !snap.loaded {
        return Err("Config не загружен".into());
    }
    Ok(snap.string_values.get(script_field).cloned().unwrap_or_default())
}

pub fn ecu_script_write(
    session: &Arc<EcuSession>,
    script_field: &str,
    content: &str,
) -> Result<(), String> {
    let ini = session.ini_context();
    let max_bytes = ini
        .config_fields
        .get(script_field)
        .and_then(|f| match f {
            ConfigFieldKind::String(s) => Some(s.length as usize),
            _ => None,
        })
        .unwrap_or(0);

    if max_bytes > 0 && content.len() >= max_bytes {
        return Err(format!(
            "Скрипт {} байт — лимит поля {} байт (нужен запас под \\0)",
            content.len(),
            max_bytes
        ));
    }
    if !session.is_connected() {
        return Err("ECU не подключена".into());
    }
    let snap = session.config().snapshot();
    if snap.read_only {
        return Err("Config только для чтения".into());
    }
    session.config().write_string(session, script_field, content)?;
    session.run_console_command("luareset")?;
    Ok(())
}

pub fn ecu_script_burn(session: &Arc<EcuSession>) -> Result<(), String> {
    if !session.is_connected() {
        return Err("ECU не подключена".into());
    }
    session.config().burn_to_flash(session)?;
    Ok(())
}

impl ComponentLogic for LuaScriptLogic {
    fn meta(&self) -> ComponentMeta {
        ComponentMeta {
            component_type: LogicComponentType::LuaScript.as_str().to_string(),
            has_rust_logic: true,
        }
    }

    fn state(&self) -> Value {
        self.to_json()
    }

    fn ecu_sync_on_mount(&self) -> EcuSyncOnMount {
        EcuSyncOnMount::OutputPollIfConfigLoaded
    }

    fn dispatch(&mut self, action: &str, payload: Value) -> Result<Value, String> {
        match action {
            "mount" => {
                if !payload.is_null() {
                    self.set_field_from_payload(&payload);
                }
                let _ = self.read_from_snapshot();
            }
            "read" => {
                self.read_from_ecu()?;
            }
            "write" => {
                let text = payload
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| self.script.clone());
                self.write_to_ecu(&text)?;
            }
            "burn" => {
                self.burn()?;
            }
            "reset_lua" => {
                self.reset_lua()?;
            }
            _ => return Err(format!("unknown action: {action}")),
        }
        Ok(self.to_json())
    }
}
