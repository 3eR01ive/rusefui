use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};

use crate::component::{ComponentLogic, ComponentMeta, EcuSyncOnMount, LogicComponentType};
use crate::session::EcuSession;

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
    session: Arc<EcuSession>,
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
    pub fn new(session: Arc<EcuSession>) -> Self {
        Self {
            session,
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
        let connected = self.session.is_connected();
        if connected {
            if let Some(info) = self.session.connection_info_if_available() {
                return ConnectionViewState {
                    ports: self.ports.clone(),
                    selected_port: self.selected_port.clone(),
                    baud_rate: self.baud_rate,
                    baud_rates: BAUD_RATES.to_vec(),
                    loading_ports: self.loading_ports,
                    connecting: self.connecting,
                    message: self.message.clone(),
                    message_is_error: self.message_is_error,
                    connected: true,
                    port_name: Some(info.port_name),
                    signature: Some(info.signature),
                    handshake_command: Some(info.handshake_command),
                    baud_rate_active: Some(info.baud_rate),
                };
            }
        }

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

    fn to_json(&self) -> Value {
        serde_json::to_value(self.view_state()).unwrap_or(json!({}))
    }

    fn refresh_ports(&mut self) -> Result<(), String> {
        self.loading_ports = true;
        self.message = None;
        self.message_is_error = false;

        match rusefi_protocol::SerialLink::list_ports() {
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

        let result = self
            .session
            .connect(&self.selected_port, self.baud_rate);

        self.connecting = false;

        match result {
            Ok(_) => {
                self.last_error = None;
                self.message = Some("Подключено.".into());
                self.message_is_error = false;
                Ok(())
            }
            Err(e) => {
                self.last_error = Some(e.clone());
                self.message = Some(e.clone());
                self.message_is_error = true;
                Err(e)
            }
        }
    }

    fn disconnect(&mut self) {
        self.session.disconnect();
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

    fn ecu_sync_on_mount(&self) -> EcuSyncOnMount {
        // AppShell: initConfig/initOutput; автоподключение: schedule_ecu_notify.
        EcuSyncOnMount::None
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
            "sync_from_session" => {
                if self.session.is_connected() {
                    if let Some(info) = self.session.connection_info_if_available() {
                        self.selected_port = info.port_name.clone();
                        self.baud_rate = info.baud_rate;
                        self.last_error = None;
                        self.message = Some("Подключено.".into());
                        self.message_is_error = false;
                    }
                } else {
                    self.message = None;
                    self.message_is_error = false;
                }
                Ok(self.to_json())
            }
            other => Err(format!("unknown action: {other}")),
        }
    }
}
