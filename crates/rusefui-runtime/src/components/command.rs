use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::component::{ComponentLogic, ComponentMeta, EcuSyncOnMount, LogicComponentType};
use crate::session::EcuSession;

const MAX_EXCHANGES: usize = 80;
const MAX_HISTORY: usize = 40;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickCommandDef {
    pub id: String,
    pub label: String,
    pub text: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QuickCommandsFile {
    commands: Vec<QuickCommandDef>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogExchange {
    id: u64,
    command: String,
    status: &'static str,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    lines: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandViewState {
    connected: bool,
    busy: bool,
    exchanges: Vec<LogExchange>,
    history: Vec<String>,
    quick_commands: Vec<QuickCommandDef>,
}

pub struct CommandLogic {
    session: Arc<EcuSession>,
    exchanges: Vec<LogExchange>,
    history: Vec<String>,
    quick_commands: Vec<QuickCommandDef>,
    busy: bool,
    next_exchange_id: u64,
}

impl CommandLogic {
    pub fn new(session: Arc<EcuSession>) -> Self {
        Self {
            session,
            exchanges: Vec::new(),
            history: Vec::new(),
            quick_commands: Vec::new(),
            busy: false,
            next_exchange_id: 1,
        }
    }

    fn view_state(&self) -> CommandViewState {
        CommandViewState {
            connected: self.session.is_connected(),
            busy: self.busy,
            exchanges: self.exchanges.clone(),
            history: self.history.clone(),
            quick_commands: self.quick_commands.clone(),
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

    fn parse_quick_commands_yaml(yaml: &str) -> Result<Vec<QuickCommandDef>, String> {
        if yaml.trim().is_empty() {
            return Ok(Vec::new());
        }
        let file: QuickCommandsFile =
            serde_yaml::from_str(yaml).map_err(|e| format!("quick-commands.yaml: {e}"))?;
        Ok(file.commands)
    }

    fn trim_exchanges(&mut self) {
        if self.exchanges.len() > MAX_EXCHANGES {
            let drop = self.exchanges.len() - MAX_EXCHANGES;
            self.exchanges.drain(0..drop);
        }
    }

    fn push_pending_exchange(&mut self, command: String) -> u64 {
        let id = self.next_exchange_id;
        self.next_exchange_id += 1;
        self.exchanges.push(LogExchange {
            id,
            command,
            status: "pending",
            lines: Vec::new(),
            error: None,
        });
        self.trim_exchanges();
        id
    }

    fn split_response_lines(response: &str) -> Vec<String> {
        let trimmed = response.trim();
        if trimmed.is_empty() {
            return vec!["(пустой ответ)".into()];
        }
        trimmed
            .lines()
            .map(|line| line.trim_end_matches('\r').to_string())
            .filter(|line| !line.is_empty())
            .collect()
    }

    fn finish_exchange(&mut self, id: u64, ok: bool, error: Option<&str>, response: Option<&str>) {
        let Some(ex) = self.exchanges.iter_mut().find(|e| e.id == id) else {
            return;
        };
        if ok {
            ex.status = "ok";
            ex.lines = response
                .map(Self::split_response_lines)
                .unwrap_or_default();
            if ex.lines.is_empty() {
                ex.lines.push("(пустой ответ)".into());
            }
        } else {
            ex.status = "err";
            ex.error = Some(error.unwrap_or("команда не выполнена").to_string());
        }
    }

    fn remember_history(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.history.retain(|h| h != text);
        self.history.push(text.to_string());
        if self.history.len() > MAX_HISTORY {
            let drop = self.history.len() - MAX_HISTORY;
            self.history.drain(0..drop);
        }
    }

    fn resolve_command_text(&self, payload: &Value) -> Result<String, String> {
        if let Some(text) = payload.get("text").and_then(|v| v.as_str()) {
            let text = text.trim();
            if text.is_empty() {
                return Err("Пустая команда".into());
            }
            return Ok(text.to_string());
        }
        if let Some(id) = payload.get("id").and_then(|v| v.as_str()) {
            return self
                .quick_commands
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.text.clone())
                .ok_or_else(|| format!("Неизвестная быстрая команда: {id}"));
        }
        Err("Нужны text или id".into())
    }

    fn begin_send(&mut self, payload: Value) -> Result<(String, u64), String> {
        self.require_connected()?;
        if self.busy {
            return Err("Команда уже выполняется".into());
        }
        let text = self.resolve_command_text(&payload)?;
        self.busy = true;
        let id = self.push_pending_exchange(text.clone());
        Ok((text, id))
    }

    fn finish_send(
        &mut self,
        exchange_id: u64,
        text: &str,
        ok: bool,
        error: Option<&str>,
        response: Option<&str>,
    ) -> Result<Value, String> {
        self.busy = false;
        self.finish_exchange(exchange_id, ok, error, response);
        if ok {
            self.remember_history(text);
        }
        Ok(self.to_json())
    }
}

impl ComponentLogic for CommandLogic {
    fn meta(&self) -> ComponentMeta {
        ComponentMeta {
            component_type: LogicComponentType::Command.as_str().to_string(),
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
                if let Some(yaml) = payload.get("quickCommandsYaml").and_then(|v| v.as_str()) {
                    self.quick_commands = Self::parse_quick_commands_yaml(yaml)?;
                }
                Ok(self.to_json())
            }
            "begin_send" => {
                let (text, exchange_id) = self.begin_send(payload)?;
                let mut v = self.to_json();
                if let Some(obj) = v.as_object_mut() {
                    obj.insert("pendingText".into(), json!(text));
                    obj.insert("pendingExchangeId".into(), json!(exchange_id));
                }
                Ok(v)
            }
            "finish_send" => {
                let exchange_id = payload
                    .get("exchangeId")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let text = payload
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let ok = payload.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
                let error = payload.get("error").and_then(|v| v.as_str());
                let response = payload.get("response").and_then(|v| v.as_str());
                self.finish_send(exchange_id, &text, ok, error, response)
            }
            "send" | "run_quick" => Err(format!(
                "action {action} выполняется через component_dispatch (async ECU)"
            )),
            "clear_log" => {
                self.exchanges.clear();
                Ok(self.to_json())
            }
            other => Err(format!("unknown action: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quick_commands_yaml_smoke() {
        let yaml = r#"
commands:
  - id: help
    label: Help
    text: help
"#;
        let cmds = CommandLogic::parse_quick_commands_yaml(yaml).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].id, "help");
        assert_eq!(cmds[0].text, "help");
    }

    #[test]
    fn split_response_preserves_lines() {
        let lines = CommandLogic::split_response_lines("line1\r\nline2\n\nline3");
        assert_eq!(lines, vec!["line1", "line2", "line3"]);
    }
}
