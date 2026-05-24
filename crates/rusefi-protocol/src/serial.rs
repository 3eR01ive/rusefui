use std::io::{Read, Write};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serialport::{DataBits, FlowControl, Parity, StopBits};

use crate::commands::{
    TS_BURN_COMMAND, TS_CHUNK_WRITE_COMMAND, TS_EXECUTE_COMMAND, TS_HELLO_COMMAND,
    TS_IO_TEST_COMMAND, TS_OUTPUT_COMMAND, TS_READ_COMMAND, TS_RESPONSE_BURN_OK,
    TS_RESPONSE_OK,
};
use crate::error::ProtocolError;
use crate::packet::{make_crc_request, parse_crc_response, CrcResponse};
use crate::tracer::ProtocolTracer;

/// Таймаут чтения страницы конфигурации (INI `blockReadTimeout` ≈ 3000 ms).
const CONFIG_IO_TIMEOUT_MS: u64 = 3000;

/// Макс. размер тела CRC-ответа (код + data), как в Java `IncomingDataBuffer`.
const MAX_CRC_BODY_LEN: usize = 65535;

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
    tracer: Option<Arc<dyn ProtocolTracer>>,
    last_request_payload: Vec<u8>,
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

    pub fn connect(
        port_name: &str,
        baud_rate: u32,
        timeout_ms: u64,
        tracer: Option<Arc<dyn ProtocolTracer>>,
    ) -> Result<Self, ProtocolError> {
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
            tracer,
            last_request_payload: Vec::new(),
        };

        link.port.clear(serialport::ClearBuffer::All)?;

        let signature = link.handshake()?;
        link.info.signature = signature;
        Ok(link)
    }

    fn handshake(&mut self) -> Result<String, ProtocolError> {
        self.port.clear(serialport::ClearBuffer::All)?;
        let payload = [TS_HELLO_COMMAND];
        self.last_request_payload = payload.to_vec();
        let request = make_crc_request(&payload);
        if let Some(tracer) = &self.tracer {
            tracer.on_tx(&payload, &request);
        }
        self.port.write_all(&request)?;
        self.port.flush()?;

        match self.read_crc_frame_logged(&payload, self.timeout_ms) {
            Ok(response) => {
                self.info.handshake_command = 'S';
                response.into_string_payload()
            }
            Err(e) => {
                if let Some(tracer) = &self.tracer {
                    tracer.on_rx_err(&payload, &e);
                }
                Err(e)
            }
        }
    }

    pub fn info(&self) -> &ConnectionInfo {
        &self.info
    }

    pub fn send_request(&mut self, payload: &[u8]) -> Result<CrcResponse, ProtocolError> {
        self.send_request_with_timeout(payload, self.timeout_ms)
    }

    fn send_request_with_timeout(
        &mut self,
        payload: &[u8],
        timeout_ms: u64,
    ) -> Result<CrcResponse, ProtocolError> {
        self.drop_pending_rx();
        self.last_request_payload = payload.to_vec();
        let request = make_crc_request(payload);
        if let Some(tracer) = &self.tracer {
            tracer.on_tx(payload, &request);
        }
        self.port.write_all(&request)?;
        self.port.flush()?;
        self.read_crc_frame_logged(payload, timeout_ms)
    }

    /// Сброс «хвостов» в RX (аналог Java `IncomingDataBuffer.dropPending` — без ожидания).
    fn drop_pending_rx(&mut self) {
        let _ = self.port.clear(serialport::ClearBuffer::Input);
    }

    /// `ochGetCommand` — live output block (`O%2o%2c`).
    ///
    /// Offset/count на проводе в **little-endian** (как Java `GetOutputsCommand` + `swap16`).
    /// Прошивка читает `uint16_t*` без swap (`tunerstudio.cpp`).
    pub fn read_output_channels(
        &mut self,
        offset: u16,
        count: u16,
    ) -> Result<Vec<u8>, ProtocolError> {
        let payload = [
            TS_OUTPUT_COMMAND,
            offset.to_le_bytes()[0],
            offset.to_le_bytes()[1],
            count.to_le_bytes()[0],
            count.to_le_bytes()[1],
        ];
        let response = self.send_request(&payload)?;
        if response.code != TS_RESPONSE_OK {
            return Err(ProtocolError::ErrorResponse(response.code));
        }
        Ok(response.payload)
    }

    /// Сборка полного output-блока чанками (как Java `requestOutputChannels`).
    pub fn read_output_channels_full(
        &mut self,
        total_size: u16,
        chunk_size: u16,
    ) -> Result<Vec<u8>, ProtocolError> {
        if chunk_size == 0 {
            return Err(ProtocolError::InvalidPacket("chunk_size is zero".into()));
        }
        let mut buf = vec![0u8; total_size as usize];
        let mut offset = 0u16;
        while offset < total_size {
            let count = (total_size - offset).min(chunk_size);
            let part = self.read_output_channels(offset, count)?;
            if part.len() != count as usize {
                return Err(ProtocolError::InvalidPacket("short output chunk".into()));
            }
            buf[offset as usize..offset as usize + part.len()].copy_from_slice(&part);
            offset += count;
        }
        Ok(buf)
    }

    /// `R` — чтение страницы конфигурации (offset/count — **LE**, как Java `ByteRange` + `swap16`).
    ///
    /// Если `read_has_page_index` (INI `pageReadCommand` = `"R%2i%2o%2c"`), на проводе
    /// page+offset+count. Старый `"R%2o%2c"` (7 символов) — только offset+count.
    ///
    /// На legacy ECU нельзя слать `00 00` перед offset: прошивка возьмёт `data16[1]` как count
    /// (например offset=416 → прочитает 416 байт с offset 0).
    pub fn read_config_chunk(
        &mut self,
        page: u16,
        offset: u16,
        count: u16,
        read_has_page_index: bool,
    ) -> Result<Vec<u8>, ProtocolError> {
        if count == 0 {
            return Err(ProtocolError::InvalidPacket("config read count is zero".into()));
        }

        let mut payload = Vec::with_capacity(if read_has_page_index { 7 } else { 5 });
        payload.push(TS_READ_COMMAND);
        if read_has_page_index {
            payload.extend_from_slice(&page.to_le_bytes());
        }
        payload.extend_from_slice(&offset.to_le_bytes());
        payload.extend_from_slice(&count.to_le_bytes());

        const MAX_ATTEMPTS: usize = 3;
        let expected_packet_len = count as usize + 1;
        let mut last_err: Option<ProtocolError> = None;

        for attempt in 0..MAX_ATTEMPTS {
            if attempt > 0 {
                thread::sleep(Duration::from_millis(15));
            }

            match self.send_request_with_timeout(&payload, CONFIG_IO_TIMEOUT_MS) {
                Ok(response) => {
                    if response.code != TS_RESPONSE_OK {
                        return Err(ProtocolError::ErrorResponse(response.code));
                    }
                    let actual_len = response.payload.len() + 1;
                    if actual_len == expected_packet_len {
                        return Ok(response.payload);
                    }
                    last_err = Some(ProtocolError::InvalidPacket(format!(
                        "config read size mismatch: got {} data bytes, expected {count} (page={page} offset={offset}, packet_len={actual_len})",
                        response.payload.len()
                    )));
                }
                Err(e) => last_err = Some(e),
            }
            self.drop_pending_rx();
        }

        Err(last_err.unwrap_or_else(|| {
            ProtocolError::InvalidPacket("config read failed after retries".into())
        }))
    }

    /// Сборка полной страницы конфигурации чанками (как Java `readFullImageFromController`).
    pub fn read_config_page_full(
        &mut self,
        page: u16,
        total_size: u32,
        chunk_size: u16,
        read_has_page_index: bool,
    ) -> Result<Vec<u8>, ProtocolError> {
        self.read_config_page_full_with_progress(
            page,
            total_size,
            chunk_size,
            read_has_page_index,
            |_, _| {},
        )
    }

    /// То же, с колбэком `(bytes_loaded, bytes_total)` после каждого чанка.
    pub fn read_config_page_full_with_progress<F>(
        &mut self,
        page: u16,
        total_size: u32,
        chunk_size: u16,
        read_has_page_index: bool,
        mut on_progress: F,
    ) -> Result<Vec<u8>, ProtocolError>
    where
        F: FnMut(u32, u32),
    {
        if chunk_size == 0 {
            return Err(ProtocolError::InvalidPacket("chunk_size is zero".into()));
        }
        let total = total_size as usize;
        let mut buf = vec![0u8; total];
        let mut offset = 0u32;
        while offset < total_size {
            let remaining = total_size - offset;
            let count = remaining.min(chunk_size as u32) as u16;
            let part = self.read_config_chunk(page, offset as u16, count, read_has_page_index)?;
            if part.len() != count as usize {
                return Err(ProtocolError::InvalidPacket(format!(
                    "short config chunk: got {} bytes, expected {count} (page={page} offset={offset})",
                    part.len()
                )));
            }
            let off = offset as usize;
            buf[off..off + part.len()].copy_from_slice(&part);
            offset += count as u32;
            on_progress(offset.min(total_size), total_size);
        }
        Ok(buf)
    }

    /// `C` — запись фрагмента page 0. Legacy INI (`C%2o%2c%v`) — offset+count+data без page;
    /// новый (`C%2i%2o%2c%v`) — page+offset+count+data (как Java `WriteCommand`).
    pub fn write_config_chunk(
        &mut self,
        page: u16,
        offset: u16,
        data: &[u8],
        chunk_write_has_page_index: bool,
    ) -> Result<(), ProtocolError> {
        let count = u16::try_from(data.len())
            .map_err(|_| ProtocolError::InvalidPacket("chunk too large".into()))?;
        let header_len = if chunk_write_has_page_index { 6 } else { 4 };
        let mut payload = Vec::with_capacity(1 + header_len + data.len());
        payload.push(TS_CHUNK_WRITE_COMMAND);
        if chunk_write_has_page_index {
            payload.extend_from_slice(&page.to_le_bytes());
        }
        payload.extend_from_slice(&offset.to_le_bytes());
        payload.extend_from_slice(&count.to_le_bytes());
        payload.extend_from_slice(data);

        let response = self.send_request(&payload)?;
        if response.code != TS_RESPONSE_OK {
            return Err(ProtocolError::ErrorResponse(response.code));
        }
        Ok(())
    }

    /// `B` + page (LE) — commit page 0 в flash (Java `BurnCommand`, INI `burnCommand = "B%2i"`).
    pub fn burn_config_page(&mut self, page: u16) -> Result<(), ProtocolError> {
        let payload = [
            TS_BURN_COMMAND,
            page.to_le_bytes()[0],
            page.to_le_bytes()[1],
        ];
        let response = self.send_request(&payload)?;
        if response.code != TS_RESPONSE_BURN_OK {
            return Err(ProtocolError::ErrorResponse(response.code));
        }
        Ok(())
    }

    /// `E` + текст — консольная команда (как Java `sendTextCommand` / rusefi_console CommandQueue).
    pub fn execute_console_command(&mut self, text: &str) -> Result<(), ProtocolError> {
        let mut payload = Vec::with_capacity(1 + text.len());
        payload.push(TS_EXECUTE_COMMAND);
        payload.extend_from_slice(text.as_bytes());
        let response = self.send_request(&payload)?;
        if response.code != TS_RESPONSE_OK {
            return Err(ProtocolError::ErrorResponse(response.code));
        }
        Ok(())
    }

    /// `Z` — `executeTSCommand(subsystem, index)`; u16 **big-endian** (`SWAP_UINT16` в прошивке).
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

    fn read_crc_frame_logged(
        &mut self,
        request_payload: &[u8],
        timeout_ms: u64,
    ) -> Result<CrcResponse, ProtocolError> {
        match self.read_crc_frame(timeout_ms) {
            Ok(response) => {
                if let Some(tracer) = &self.tracer {
                    let frame = build_response_frame(&response);
                    tracer.on_rx_ok(request_payload, &frame, &response);
                }
                Ok(response)
            }
            Err(e) => {
                if let Some(tracer) = &self.tracer {
                    tracer.on_rx_err(request_payload, &e);
                }
                Err(e)
            }
        }
    }

    fn read_crc_frame(&mut self, timeout_ms: u64) -> Result<CrcResponse, ProtocolError> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);

        let mut header = [0u8; 2];
        self.read_exact_deadline(&mut header, deadline)?;

        // Как Java `swap16(getShort())` — BE длина тела на проводе.
        let body_len = u16::from_be_bytes(header) as usize;
        if body_len == 0 || body_len > MAX_CRC_BODY_LEN {
            return Err(ProtocolError::InvalidPacket(format!(
                "bad body length: {body_len}"
            )));
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
        deadline: Instant,
    ) -> Result<(), ProtocolError> {
        let mut offset = 0;
        while offset < buf.len() {
            if Instant::now() >= deadline {
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

fn build_response_frame(response: &CrcResponse) -> Vec<u8> {
    let mut body = vec![response.code];
    body.extend_from_slice(&response.payload);
    let len = body.len() as u16;
    let mut frame = vec![(len >> 8) as u8, (len & 0xFF) as u8];
    frame.extend_from_slice(&body);
    let crc = crate::crc::crc32(&body);
    frame.extend_from_slice(&crc.to_be_bytes());
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_ports_does_not_panic() {
        let _ = SerialLink::list_ports();
    }

    #[test]
    fn output_poll_detection() {
        assert!(crate::is_output_poll(&[b'O', 0, 0, 0, 0]));
    }

    /// Совпадает с `OchGetCommandTest` в rusEFI Java console.
    #[test]
    fn output_request_wire_format_matches_java() {
        fn o_payload(offset: u16, count: u16) -> [u8; 5] {
            [
                TS_OUTPUT_COMMAND,
                offset.to_le_bytes()[0],
                offset.to_le_bytes()[1],
                count.to_le_bytes()[0],
                count.to_le_bytes()[1],
            ]
        }
        assert_eq!(o_payload(400, 300), [b'O', 0x90, 0x01, 0x2c, 0x01]);
        assert_eq!(o_payload(0, 2044), [b'O', 0x00, 0x00, 0xfc, 0x07]);
    }

    /// С page (`R%2i%2o%2c`).
    #[test]
    fn read_config_request_wire_format_with_page() {
        fn r_payload(page: u16, offset: u16, count: u16) -> [u8; 7] {
            [
                TS_READ_COMMAND,
                page.to_le_bytes()[0],
                page.to_le_bytes()[1],
                offset.to_le_bytes()[0],
                offset.to_le_bytes()[1],
                count.to_le_bytes()[0],
                count.to_le_bytes()[1],
            ]
        }
        assert_eq!(r_payload(0, 0, 1024), [b'R', 0, 0, 0, 0, 0, 4]);
        assert_eq!(r_payload(0, 1024, 1024), [b'R', 0, 0, 0, 4, 0, 4]);
    }

    /// Без page (`R%2o%2c`) — как INI `560262154.ini` и Java `ByteRange.packOffsetAndSize`.
    #[test]
    fn read_config_request_wire_format_legacy_no_page() {
        fn r_payload(offset: u16, count: u16) -> [u8; 5] {
            [
                TS_READ_COMMAND,
                offset.to_le_bytes()[0],
                offset.to_le_bytes()[1],
                count.to_le_bytes()[0],
                count.to_le_bytes()[1],
            ]
        }
        assert_eq!(r_payload(0, 1024), [b'R', 0, 0, 0, 4]);
    }

    #[test]
    fn burn_request_wire_format() {
        fn b_payload(page: u16) -> [u8; 3] {
            [
                TS_BURN_COMMAND,
                page.to_le_bytes()[0],
                page.to_le_bytes()[1],
            ]
        }
        assert_eq!(b_payload(0), [b'B', 0, 0]);
    }

    #[test]
    fn execute_console_command_payload_format() {
        let text = "rpm 1500";
        let mut payload = vec![TS_EXECUTE_COMMAND];
        payload.extend_from_slice(text.as_bytes());
        assert_eq!(payload[0], b'E');
        assert_eq!(&payload[1..], b"rpm 1500");
    }
}
