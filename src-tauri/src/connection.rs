use rusefi_protocol::{ConnectionInfo, ProtocolError, SerialLink, DEFAULT_IO_TIMEOUT_MS};
use serde::{Deserialize, Serialize};

pub struct AppConnection {
    link: Option<SerialLink>,
    last_error: Option<String>,
}

impl Default for AppConnection {
    fn default() -> Self {
        Self {
            link: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectParams {
    pub port: String,
    #[serde(default = "default_baud")]
    pub baud_rate: u32,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_baud() -> u32 {
    115_200
}

fn default_timeout_ms() -> u64 {
    DEFAULT_IO_TIMEOUT_MS
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionStatusDto {
    pub connected: bool,
    pub port_name: Option<String>,
    pub baud_rate: Option<u32>,
    pub signature: Option<String>,
    pub handshake_command: Option<char>,
    pub last_error: Option<String>,
}

impl AppConnection {
    pub fn connect(&mut self, params: ConnectParams) -> Result<ConnectionStatusDto, String> {
        self.disconnect_internal();

        match SerialLink::connect(&params.port, params.baud_rate, params.timeout_ms) {
            Ok(link) => {
                self.last_error = None;
                self.link = Some(link);
                Ok(self.status_dto())
            }
            Err(e) => {
                self.last_error = Some(format_error(&e));
                Err(self.last_error.clone().unwrap())
            }
        }
    }

    pub fn disconnect(&mut self) -> Result<ConnectionStatusDto, String> {
        self.disconnect_internal();
        Ok(self.status_dto())
    }

    fn disconnect_internal(&mut self) {
        self.link = None;
    }

    pub fn status_dto(&self) -> ConnectionStatusDto {
        match &self.link {
            Some(link) => {
                let info: &ConnectionInfo = link.info();
                ConnectionStatusDto {
                    connected: true,
                    port_name: Some(info.port_name.clone()),
                    baud_rate: Some(info.baud_rate),
                    signature: Some(info.signature.clone()),
                    handshake_command: Some(info.handshake_command),
                    last_error: self.last_error.clone(),
                }
            }
            None => ConnectionStatusDto {
                connected: false,
                port_name: None,
                baud_rate: None,
                signature: None,
                handshake_command: None,
                last_error: self.last_error.clone(),
            },
        }
    }
}

fn format_error(e: &ProtocolError) -> String {
    e.to_string()
}
