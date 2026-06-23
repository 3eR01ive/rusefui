use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};

use crate::component::{ComponentLogic, ComponentMeta, EcuSyncOnMount, LogicComponentType};
use crate::session::EcuSession;

const BAUD_RATES: &[u32] = &[115_200, 230_400, 460_800, 921_600];

/// Способ подключения: COM-порт или TCP (Wi-Fi мост ESP32).
const MODE_SERIAL: &str = "serial";
const MODE_TCP: &str = "tcp";

/// Значения по умолчанию для TCP — USB-host/Wi-Fi мост ESP32-S3 (SoftAP IP
/// 192.168.4.1, TCP-порт 29001 — штатный порт TCP-прокси rusEFI).
const DEFAULT_TCP_HOST: &str = "192.168.4.1";
const DEFAULT_TCP_PORT: u16 = 29001;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionViewState {
    /// `"serial"` | `"tcp"`.
    mode: String,
    ports: Vec<String>,
    selected_port: String,
    baud_rate: u32,
    baud_rates: Vec<u32>,
    tcp_host: String,
    tcp_port: u16,
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
    mode: String,
    ports: Vec<String>,
    selected_port: String,
    baud_rate: u32,
    tcp_host: String,
    tcp_port: u16,
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
            mode: MODE_SERIAL.to_string(),
            ports: Vec::new(),
            selected_port: String::new(),
            baud_rate: 115_200,
            tcp_host: DEFAULT_TCP_HOST.to_string(),
            tcp_port: DEFAULT_TCP_PORT,
            loading_ports: false,
            connecting: false,
            message: None,
            message_is_error: false,
            last_error: None,
        }
    }

    fn view_state(&self) -> ConnectionViewState {
        let connection = self
            .session
            .is_connected()
            .then(|| self.session.connection_info_if_available())
            .flatten();

        ConnectionViewState {
            mode: self.mode.clone(),
            ports: self.ports.clone(),
            selected_port: self.selected_port.clone(),
            baud_rate: self.baud_rate,
            baud_rates: BAUD_RATES.to_vec(),
            tcp_host: self.tcp_host.clone(),
            tcp_port: self.tcp_port,
            loading_ports: self.loading_ports,
            connecting: self.connecting,
            message: self.message.clone(),
            message_is_error: self.message_is_error,
            connected: connection.is_some(),
            port_name: connection.as_ref().map(|i| i.port_name.clone()),
            signature: connection.as_ref().map(|i| i.signature.clone()),
            handshake_command: connection.as_ref().map(|i| i.handshake_command),
            baud_rate_active: connection.as_ref().map(|i| i.baud_rate),
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
        let is_tcp = self.mode == MODE_TCP;
        if is_tcp {
            if self.tcp_host.trim().is_empty() {
                return Err("Не задан адрес TCP (host)".into());
            }
        } else if self.selected_port.is_empty() {
            return Err("Порт не выбран".into());
        }

        self.connecting = true;
        self.message = None;
        self.message_is_error = false;

        let result = if is_tcp {
            self.session
                .connect_tcp(self.tcp_host.trim(), self.tcp_port)
        } else {
            self.session.connect(&self.selected_port, self.baud_rate)
        };

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
            "set_mode" => {
                if let Some(mode) = payload.get("mode").and_then(|v| v.as_str()) {
                    self.mode = if mode == MODE_TCP {
                        MODE_TCP.to_string()
                    } else {
                        MODE_SERIAL.to_string()
                    };
                    self.message = None;
                    self.message_is_error = false;
                }
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
            "set_tcp_host" => {
                if let Some(host) = payload.get("host").and_then(|v| v.as_str()) {
                    self.tcp_host = host.to_string();
                }
                Ok(self.to_json())
            }
            "set_tcp_port" => {
                if let Some(port) = payload.get("port").and_then(|v| v.as_u64()) {
                    self.tcp_port = port.min(u16::MAX as u64) as u16;
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
                        // TCP-линк отдаёт port_name = "host:port" и baud_rate = 0 —
                        // не затираем настройки serial.
                        if info.baud_rate == 0 && info.port_name.contains(':') {
                            self.mode = MODE_TCP.to_string();
                            if let Some((host, port)) = info.port_name.rsplit_once(':') {
                                self.tcp_host = host.to_string();
                                if let Ok(p) = port.parse::<u16>() {
                                    self.tcp_port = p;
                                }
                            }
                        } else {
                            self.mode = MODE_SERIAL.to_string();
                            self.selected_port = info.port_name.clone();
                            self.baud_rate = info.baud_rate;
                        }
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
