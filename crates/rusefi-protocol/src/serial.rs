use std::io::{Read, Write};
use std::time::Duration;

use serialport::{DataBits, FlowControl, Parity, StopBits};

use crate::commands::{TS_HELLO_COMMAND, TS_QUERY_COMMAND};
use crate::error::ProtocolError;
use crate::packet::{make_crc_request, parse_crc_response, CrcResponse};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectionInfo {
    pub port_name: String,
    pub baud_rate: u32,
    pub signature: String,
    /// Which handshake byte succeeded (`Q` or `S`).
    pub handshake_command: char,
}

pub struct SerialLink {
    port: Box<dyn serialport::SerialPort>,
    timeout_ms: u64,
    info: ConnectionInfo,
}

impl SerialLink {
    pub fn list_ports() -> Result<Vec<String>, ProtocolError> {
        let mut names: Vec<String> = serialport::available_ports()?
            .into_iter()
            .map(|p| p.port_name)
            .collect();
        names.sort();
        Ok(names)
    }

    pub fn connect(port_name: &str, baud_rate: u32, timeout_ms: u64) -> Result<Self, ProtocolError> {
        let timeout = Duration::from_millis(timeout_ms);
        let port = serialport::new(port_name, baud_rate)
            .timeout(timeout)
            .data_bits(DataBits::Eight)
            .parity(Parity::None)
            .stop_bits(StopBits::One)
            .flow_control(FlowControl::None)
            .open()?;

        let mut link = Self {
            port,
            timeout_ms,
            info: ConnectionInfo {
                port_name: port_name.to_string(),
                baud_rate,
                signature: String::new(),
                handshake_command: TS_HELLO_COMMAND as char,
            },
        };

        link.port.clear(serialport::ClearBuffer::All)?;

        let signature = link.handshake()?;
        link.info.signature = signature;
        Ok(link)
    }

    fn handshake(&mut self) -> Result<String, ProtocolError> {
        self.port.clear(serialport::ClearBuffer::All)?;
        if let Ok(sig) = self.try_handshake_crc(TS_HELLO_COMMAND) {
            self.info.handshake_command = 'S';
            return Ok(sig);
        }

        self.port.clear(serialport::ClearBuffer::All)?;
        if let Ok(sig) = self.try_handshake_plain() {
            self.info.handshake_command = 'Q';
            return Ok(sig);
        }

        Err(ProtocolError::Timeout(self.timeout_ms))
    }

    fn try_handshake_crc(&mut self, command: u8) -> Result<String, ProtocolError> {
        let request = make_crc_request(&[command]);
        self.port.write_all(&request)?;
        self.port.flush()?;

        let response = self.read_crc_frame()?;
        response.into_string_payload()
    }

    fn try_handshake_plain(&mut self) -> Result<String, ProtocolError> {
        self.port.write_all(&[TS_QUERY_COMMAND])?;
        self.port.flush()?;

        let deadline = std::time::Instant::now() + Duration::from_millis(self.timeout_ms);
        let mut buf = Vec::with_capacity(128);
        let mut chunk = [0u8; 64];

        while std::time::Instant::now() < deadline {
            match self.port.read(&mut chunk) {
                Ok(0) => std::thread::sleep(Duration::from_millis(2)),
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.iter().any(|&b| b == 0) || buf.len() >= 120 {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    if !buf.is_empty() {
                        break;
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }

        let s = String::from_utf8_lossy(&buf)
            .trim_end_matches('\0')
            .trim()
            .to_string();
        if s.is_empty() {
            Err(ProtocolError::EmptySignature)
        } else {
            Ok(s)
        }
    }

    pub fn info(&self) -> &ConnectionInfo {
        &self.info
    }

    pub fn send_request(&mut self, payload: &[u8]) -> Result<CrcResponse, ProtocolError> {
        self.port.clear(serialport::ClearBuffer::All)?;
        let request = make_crc_request(payload);
        self.port.write_all(&request)?;
        self.port.flush()?;
        self.read_crc_frame()
    }

    fn read_crc_frame(&mut self) -> Result<CrcResponse, ProtocolError> {
        let deadline = std::time::Instant::now() + Duration::from_millis(self.timeout_ms);

        let mut header = [0u8; 2];
        self.read_exact_deadline(&mut header, deadline)?;

        let body_len = u16::from_be_bytes(header) as usize;
        if body_len == 0 || body_len > 16 * 1024 {
            return Err(ProtocolError::InvalidPacket("bad body length"));
        }

        let mut rest = vec![0u8; body_len + 4];
        self.read_exact_deadline(&mut rest, deadline)?;

        let mut frame = Vec::with_capacity(2 + rest.len());
        frame.extend_from_slice(&header);
        frame.extend_from_slice(&rest);

        parse_crc_response(&frame)
    }

    fn read_exact_deadline(
        &mut self,
        buf: &mut [u8],
        deadline: std::time::Instant,
    ) -> Result<(), ProtocolError> {
        let mut offset = 0;
        while offset < buf.len() {
            if std::time::Instant::now() >= deadline {
                return Err(ProtocolError::Timeout(self.timeout_ms));
            }
            match self.port.read(&mut buf[offset..]) {
                Ok(0) => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Ok(n) => offset += n,
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_ports_does_not_panic() {
        let _ = SerialLink::list_ports();
    }
}
