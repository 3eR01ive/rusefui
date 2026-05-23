use rusefi_protocol::{ProtocolError, SerialLink, DEFAULT_IO_TIMEOUT_MS};
use serde::Serialize;
use serde_json::{json, Value};

use crate::component::{ComponentLogic, ComponentMeta, LogicComponentType};

const BAUD_RATES: &[u32] = &[115_200, 230_400, 460_800, 921_600];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionViewState {
    ports: Vec<String>,
    selected_port: String,
    baud_rate: u32,
    baud_rates: Vec<u32>,
    loading_ports: bool,
    connecting: bool,
    message: Option<String>,
    message_is_error: bool,
    connected: bool,
    port_name: Option<String>,
    signature: Option<String>,
    handshake_command: Option<char>,
    baud_rate_active: Option<u32>,
}

pub struct ConnectionLogic {
    link: Option<SerialLink>,
    ports: Vec<String>,
    selected_port: String,
    baud_rate: u32,
    loading_ports: bool,
    connecting: bool,
    message: Option<String>,
    message_is_error: bool,
    last_error: Option<String>,
}

impl ConnectionLogic {
    pub fn new() -> Self {
        Self {
            link: None,
            ports: Vec::new(),
            selected_port: String::new(),
            baud_rate: 115_200,
            loading_ports: false,
            connecting: false,
            message: None,
            message_is_error: false,
            last_error: None,
        }
    }

    fn view_state(&self) -> ConnectionViewState {
        if let Some(link) = &self.link {
            let info = link.info();
            ConnectionViewState {
                ports: self.ports.clone(),
                selected_port: self.selected_port.clone(),
                baud_rate: self.baud_rate,
                baud_rates: BAUD_RATES.to_vec(),
                loading_ports: self.loading_ports,
                connecting: self.connecting,
                message: self.message.clone(),
                message_is_error: self.message_is_error,
                connected: true,
                port_name: Some(info.port_name.clone()),
                signature: Some(info.signature.clone()),
                handshake_command: Some(info.handshake_command),
                baud_rate_active: Some(info.baud_rate),
            }
        } else {
            ConnectionViewState {
                ports: self.ports.clone(),
                selected_port: self.selected_port.clone(),
                baud_rate: self.baud_rate,
                baud_rates: BAUD_RATES.to_vec(),
                loading_ports: self.loading_ports,
                connecting: self.connecting,
                message: self.message.clone(),
                message_is_error: self.message_is_error,
                connected: false,
                port_name: None,
                signature: None,
                handshake_command: None,
                baud_rate_active: None,
            }
        }
    }

    fn to_json(&self) -> Value {
        serde_json::to_value(self.view_state()).unwrap_or(json!({}))
    }

    fn refresh_ports(&mut self) -> Result<(), String> {
        self.loading_ports = true;
        self.message = None;
        self.message_is_error = false;

        match SerialLink::list_ports() {
            Ok(ports) => {
                self.ports = ports;
                if self.selected_port.is_empty() {
                    if let Some(first) = self.ports.first() {
                        self.selected_port = first.clone();
                    }
                }
                if self.ports.is_empty() {
                    self.message = Some("Последовательные порты не найдены.".into());
                }
                self.loading_ports = false;
                Ok(())
            }
            Err(e) => {
                self.loading_ports = false;
                self.message = Some(e.to_string());
                self.message_is_error = true;
                Err(self.message.clone().unwrap())
            }
        }
    }

    fn connect(&mut self) -> Result<(), String> {
        if self.selected_port.is_empty() {
            return Err("Порт не выбран".into());
        }
        self.connecting = true;
        self.message = None;
        self.message_is_error = false;
        self.link = None;

        let result = SerialLink::connect(
            &self.selected_port,
            self.baud_rate,
            DEFAULT_IO_TIMEOUT_MS,
        );

        self.connecting = false;

        match result {
            Ok(link) => {
                self.last_error = None;
                self.link = Some(link);
                self.message = Some("Подключено.".into());
                self.message_is_error = false;
                Ok(())
            }
            Err(e) => {
                let msg = format_error(&e);
                self.last_error = Some(msg.clone());
                self.message = Some(msg.clone());
                self.message_is_error = true;
                Err(msg)
            }
        }
    }

    fn disconnect(&mut self) {
        self.link = None;
        self.message = Some("Отключено.".into());
        self.message_is_error = false;
    }
}

impl ComponentLogic for ConnectionLogic {
    fn meta(&self) -> ComponentMeta {
        ComponentMeta {
            component_type: LogicComponentType::Connection.as_str().to_string(),
            has_rust_logic: true,
        }
    }

    fn state(&self) -> Value {
        self.to_json()
    }

    fn dispatch(&mut self, action: &str, payload: Value) -> Result<Value, String> {
        match action {
            "mount" | "refresh_ports" => {
                self.refresh_ports()?;
                Ok(self.to_json())
            }
            "set_selected_port" => {
                if let Some(port) = payload.get("port").and_then(|v| v.as_str()) {
                    self.selected_port = port.to_string();
                }
                Ok(self.to_json())
            }
            "set_baud_rate" => {
                if let Some(baud) = payload.get("baud_rate").and_then(|v| v.as_u64()) {
                    self.baud_rate = baud as u32;
                }
                Ok(self.to_json())
            }
            "connect" => {
                self.connect()?;
                Ok(self.to_json())
            }
            "disconnect" => {
                self.disconnect();
                Ok(self.to_json())
            }
            other => Err(format!("unknown action: {other}")),
        }
    }
}

fn format_error(e: &ProtocolError) -> String {
    e.to_string()
}
