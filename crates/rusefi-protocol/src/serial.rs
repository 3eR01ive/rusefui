use std::io::{Read, Write};
use std::time::Duration;

use serialport::{DataBits, FlowControl, Parity, StopBits};

use crate::commands::{
    TS_CHUNK_WRITE_COMMAND, TS_HELLO_COMMAND, TS_IO_TEST_COMMAND, TS_OUTPUT_COMMAND,
    TS_RESPONSE_OK,
};
use crate::error::ProtocolError;
use crate::packet::{make_crc_request, parse_crc_response, CrcResponse};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectionInfo {
    pub port_name: String,
    pub baud_rate: u32,
    pub signature: String,
    /// Handshake command (INI `queryCommand`, CRC envelope).
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
        let request = make_crc_request(&[TS_HELLO_COMMAND]);
        self.port.write_all(&request)?;
        self.port.flush()?;

        let response = self.read_crc_frame()?;
        self.info.handshake_command = 'S';
        response.into_string_payload()
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

    /// `ochGetCommand` — live output block (`O%2o%2c`, big-endian offset/count).
    pub fn read_output_channels(
        &mut self,
        offset: u16,
        count: u16,
    ) -> Result<Vec<u8>, ProtocolError> {
        let payload = [
            TS_OUTPUT_COMMAND,
            (offset >> 8) as u8,
            (offset & 0xFF) as u8,
            (count >> 8) as u8,
            (count & 0xFF) as u8,
        ];
        let response = self.send_request(&payload)?;
        if response.code != TS_RESPONSE_OK {
            return Err(ProtocolError::ErrorResponse(response.code));
        }
        Ok(response.payload)
    }

    /// `C` — запись фрагмента страницы конфигурации (page, offset BE, count BE, data).
    pub fn write_config_chunk(
        &mut self,
        page: u16,
        offset: u16,
        data: &[u8],
    ) -> Result<(), ProtocolError> {
        let count = u16::try_from(data.len())
            .map_err(|_| ProtocolError::InvalidPacket("chunk too large"))?;
        let mut payload = Vec::with_capacity(7 + data.len());
        payload.push(TS_CHUNK_WRITE_COMMAND);
        payload.extend_from_slice(&page.to_be_bytes());
        payload.extend_from_slice(&offset.to_be_bytes());
        payload.extend_from_slice(&count.to_be_bytes());
        payload.extend_from_slice(data);

        let response = self.send_request(&payload)?;
        if response.code != TS_RESPONSE_OK {
            return Err(ProtocolError::ErrorResponse(response.code));
        }
        Ok(())
    }

    /// `Z` — `executeTSCommand(subsystem, index)` (bench, stimulator, ETB, …).
    pub fn execute_ts_command(
        &mut self,
        subsystem: u16,
        index: u16,
    ) -> Result<(), ProtocolError> {
        let payload = [
            TS_IO_TEST_COMMAND,
            (subsystem >> 8) as u8,
            (subsystem & 0xFF) as u8,
            (index >> 8) as u8,
            (index & 0xFF) as u8,
        ];
        let response = self.send_request(&payload)?;
        if response.code != TS_RESPONSE_OK {
            return Err(ProtocolError::ErrorResponse(response.code));
        }
        Ok(())
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
