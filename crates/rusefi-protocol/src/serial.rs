use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serialport::{DataBits, FlowControl, Parity, StopBits};

use crate::commands::{
    TS_BURN_COMMAND, TS_CHUNK_WRITE_COMMAND, TS_COMPOSITE_DISABLE, TS_COMPOSITE_READ,
    TS_EXECUTE_COMMAND, TS_GET_TEXT, TS_HELLO_COMMAND, TS_IO_TEST_COMMAND, TS_OUTPUT_COMMAND,
    TS_READ_COMMAND, TS_RESPONSE_BURN_OK, TS_RESPONSE_OK, TS_SET_LOGGER_SWITCH,
};
use crate::error::ProtocolError;
use crate::packet::{make_crc_request, parse_crc_response, CrcResponse};
use crate::tracer::ProtocolTracer;

/// Таймаут чтения страницы конфигурации (INI `blockReadTimeout` ≈ 3000 ms).
const CONFIG_IO_TIMEOUT_MS: u64 = 3000;

/// Макс. размер тела CRC-ответа (код + data), как в Java `IncomingDataBuffer`.
const MAX_CRC_BODY_LEN: usize = 65535;

/// Собрать тело CRC-запроса `R` (page опционален — см. INI `pageReadCommand`).
pub fn pack_config_read_request(
    page: u16,
    offset: u16,
    count: u16,
    include_page_index: bool,
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(if include_page_index { 7 } else { 5 });
    payload.push(TS_READ_COMMAND);
    if include_page_index {
        payload.extend_from_slice(&page.to_le_bytes());
    }
    payload.extend_from_slice(&offset.to_le_bytes());
    payload.extend_from_slice(&count.to_le_bytes());
    payload
}

/// Собрать тело CRC-запроса `C` (page опционален — см. INI `pageChunkWrite`).
pub fn pack_config_write_request(
    page: u16,
    offset: u16,
    data: &[u8],
    include_page_index: bool,
) -> Vec<u8> {
    let count = data.len() as u16;
    let mut payload = Vec::with_capacity(
        1 + usize::from(include_page_index) * 2 + 4 + data.len(),
    );
    payload.push(TS_CHUNK_WRITE_COMMAND);
    if include_page_index {
        payload.extend_from_slice(&page.to_le_bytes());
    }
    payload.extend_from_slice(&offset.to_le_bytes());
    payload.extend_from_slice(&count.to_le_bytes());
    payload.extend_from_slice(data);
    payload
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectionInfo {
    pub port_name: String,
    pub baud_rate: u32,
    pub signature: String,
    /// Handshake command (INI `queryCommand`, CRC envelope).
    pub handshake_command: char,
}

/// Байтовый транспорт под TS-протоколом. Абстрагирует физический канал к ECU:
/// последовательный порт (UART / USB-CDC) или TCP (например Wi-Fi мост ESP32,
/// который прозрачно проксирует байты между сокетом и UART ECU).
///
/// `EcuLink` реализует весь протокол TunerStudio поверх этого трейта и ничего
/// не знает о том, serial это или сеть — добавление нового канала сводится к
/// новой реализации `Transport`.
pub trait Transport: Send {
    /// Прочитать доступные байты (семантика `Read::read`); `Ok(0)` — данных пока нет.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    /// Записать буфер целиком.
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()>;
    /// Протолкнуть исходящий буфер.
    fn flush(&mut self) -> io::Result<()>;
    /// Сбросить входящий буфер (drop pending RX перед новым запросом).
    fn clear_input(&mut self) -> io::Result<()>;
    /// Сбросить оба буфера (перед handshake).
    fn clear_all(&mut self) -> io::Result<()>;
}

fn serial_clear_err(e: serialport::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e)
}

/// Транспорт поверх последовательного порта (`serialport`).
struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
}

impl Transport for SerialTransport {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.port.read(buf)
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.port.write_all(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.port.flush()
    }
    fn clear_input(&mut self) -> io::Result<()> {
        self.port
            .clear(serialport::ClearBuffer::Input)
            .map_err(serial_clear_err)
    }
    fn clear_all(&mut self) -> io::Result<()> {
        self.port
            .clear(serialport::ClearBuffer::All)
            .map_err(serial_clear_err)
    }
}

/// Транспорт поверх TCP-сокета (Wi-Fi мост ESP32 ↔ UART ECU).
///
/// Чтение работает с таймаутом опроса `read_timeout`: при отсутствии данных
/// возвращается `Ok(0)` (как у не-блокирующего serial), чтобы внешний цикл
/// `read_exact_deadline` сам отслеживал общий дедлайн.
struct TcpTransport {
    stream: TcpStream,
    read_timeout: Duration,
}

impl TcpTransport {
    /// Гранулярность опроса чтения: read() блокируется максимум на столько.
    const POLL_INTERVAL: Duration = Duration::from_millis(50);

    /// Слить накопившиеся входящие байты (аналог `clear` для serial).
    fn drain(&mut self) -> io::Result<()> {
        self.stream.set_read_timeout(Some(Duration::from_millis(2)))?;
        let mut scratch = [0u8; 512];
        let result = loop {
            match self.stream.read(&mut scratch) {
                Ok(0) => break Ok(()),
                Ok(_) => continue,
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    break Ok(())
                }
                Err(e) => break Err(e),
            }
        };
        self.stream.set_read_timeout(Some(self.read_timeout))?;
        result
    }
}

impl Transport for TcpTransport {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.stream.read(buf) {
            Ok(n) => Ok(n),
            // Таймаут опроса — данных пока нет, не ошибка.
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                Ok(0)
            }
            Err(e) => Err(e),
        }
    }
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.stream.write_all(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
    fn clear_input(&mut self) -> io::Result<()> {
        self.drain()
    }
    fn clear_all(&mut self) -> io::Result<()> {
        self.drain()
    }
}

/// Линк к ECU поверх произвольного [`Transport`] (serial или TCP).
///
/// Реализует протокол TunerStudio (handshake, чтение output-каналов, страниц
/// конфигурации, консольные команды и т.д.) транспорт-агностично.
pub struct EcuLink {
    transport: Box<dyn Transport>,
    timeout_ms: u64,
    info: ConnectionInfo,
    tracer: Option<Arc<dyn ProtocolTracer>>,
    last_request_payload: Vec<u8>,
}

/// Историческое имя — линк больше не привязан к serial, но многие вызовы
/// (`session`, `port_discovery`, UI-компонент) используют это имя.
pub type SerialLink = EcuLink;

impl EcuLink {
    pub fn list_ports() -> Result<Vec<String>, ProtocolError> {
        let mut names: Vec<String> = serialport::available_ports()?
            .into_iter()
            .map(|p| p.port_name)
            .collect();
        names.sort();
        Ok(names)
    }

    /// Подключение по последовательному порту (UART / USB-CDC).
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
            .open()
            .map_err(|e| crate::port_discovery::map_serial_open_error(port_name, e))?;

        let info = ConnectionInfo {
            port_name: port_name.to_string(),
            baud_rate,
            signature: String::new(),
            handshake_command: TS_HELLO_COMMAND as char,
        };
        Self::establish(Box::new(SerialTransport { port }), info, timeout_ms, tracer)
    }

    /// Подключение по TCP (Wi-Fi мост ESP32 ↔ UART ECU).
    ///
    /// `port_name` в [`ConnectionInfo`] = `host:port`, `baud_rate` = 0 (baud к
    /// сетевому каналу неприменим — он задаётся на мосте/ECU).
    pub fn connect_tcp(
        host: &str,
        tcp_port: u16,
        timeout_ms: u64,
        tracer: Option<Arc<dyn ProtocolTracer>>,
    ) -> Result<Self, ProtocolError> {
        let connect_timeout = Duration::from_millis(timeout_ms.max(2000));
        let addrs = (host, tcp_port)
            .to_socket_addrs()
            .map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidInput, format!("{host}:{tcp_port}: {e}"))
            })?
            .collect::<Vec<_>>();
        if addrs.is_empty() {
            return Err(ProtocolError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("не удалось разрешить адрес {host}:{tcp_port}"),
            )));
        }

        let mut last_err: Option<io::Error> = None;
        let stream = addrs
            .iter()
            .find_map(|addr| match TcpStream::connect_timeout(addr, connect_timeout) {
                Ok(stream) => Some(stream),
                Err(e) => {
                    last_err = Some(e);
                    None
                }
            })
            .ok_or_else(|| {
                ProtocolError::Io(last_err.unwrap_or_else(|| {
                    io::Error::new(io::ErrorKind::Other, "TCP connect failed")
                }))
            })?;

        stream.set_nodelay(true)?;
        let read_timeout = TcpTransport::POLL_INTERVAL;
        stream.set_read_timeout(Some(read_timeout))?;
        stream.set_write_timeout(Some(Duration::from_millis(timeout_ms.max(2000))))?;

        let info = ConnectionInfo {
            port_name: format!("{host}:{tcp_port}"),
            baud_rate: 0,
            signature: String::new(),
            handshake_command: TS_HELLO_COMMAND as char,
        };
        Self::establish(
            Box::new(TcpTransport {
                stream,
                read_timeout,
            }),
            info,
            timeout_ms,
            tracer,
        )
    }

    /// Общий путь: собрать линк поверх готового транспорта, сбросить буферы и
    /// выполнить handshake `S` для получения signature.
    fn establish(
        transport: Box<dyn Transport>,
        info: ConnectionInfo,
        timeout_ms: u64,
        tracer: Option<Arc<dyn ProtocolTracer>>,
    ) -> Result<Self, ProtocolError> {
        let mut link = Self {
            transport,
            timeout_ms,
            info,
            tracer,
            last_request_payload: Vec::new(),
        };

        link.transport.clear_all()?;

        let signature = link.handshake()?;
        link.info.signature = signature;
        Ok(link)
    }

    fn handshake(&mut self) -> Result<String, ProtocolError> {
        self.transport.clear_all()?;
        let payload = [TS_HELLO_COMMAND];
        self.last_request_payload = payload.to_vec();
        let request = make_crc_request(&payload);
        if let Some(tracer) = &self.tracer {
            tracer.on_tx(&payload, &request);
        }
        self.transport.write_all(&request)?;
        self.transport.flush()?;

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
        self.transport.write_all(&request)?;
        self.transport.flush()?;
        self.read_crc_frame_logged(payload, timeout_ms)
    }

    /// Сброс «хвостов» в RX (аналог Java `IncomingDataBuffer.dropPending` — без ожидания).
    fn drop_pending_rx(&mut self) {
        let _ = self.transport.clear_input();
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

    /// `R` — чтение страницы конфигурации (offset/count — **LE**).
    ///
    /// Формат кадра — из INI `pageReadCommand` (`page_read_has_page_index`).
    /// На legacy (`R%2o%2c`) **все** `R` без page: и загрузка page, и verify после `C`.
    /// Нельзя подмешивать page=0 перед offset — ECU прочитает offset=0, count=416 и т.п.
    pub fn read_config_chunk(
        &mut self,
        page: u16,
        offset: u16,
        count: u16,
        include_page_index: bool,
    ) -> Result<Vec<u8>, ProtocolError> {
        if count == 0 {
            return Err(ProtocolError::InvalidPacket("config read count is zero".into()));
        }

        let payload = pack_config_read_request(page, offset, count, include_page_index);

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
        page_read_has_page_index: bool,
    ) -> Result<Vec<u8>, ProtocolError> {
        self.read_config_page_full_with_progress(
            page,
            total_size,
            chunk_size,
            page_read_has_page_index,
            |_, _| {},
        )
    }

    /// То же, с колбэком `(bytes_loaded, bytes_total)` после каждого чанка.
    pub fn read_config_page_full_with_progress<F>(
        &mut self,
        page: u16,
        total_size: u32,
        chunk_size: u16,
        page_read_has_page_index: bool,
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
            let part =
                self.read_config_chunk(page, offset as u16, count, page_read_has_page_index)?;
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

    /// `C` — запись фрагмента settings page (`include_page_index` из INI `pageChunkWrite`).
    pub fn write_config_chunk(
        &mut self,
        page: u16,
        offset: u16,
        data: &[u8],
        include_page_index: bool,
    ) -> Result<(), ProtocolError> {
        if data.len() > u16::MAX as usize {
            return Err(ProtocolError::InvalidPacket("chunk too large".into()));
        }
        let payload = pack_config_write_request(page, offset, data, include_page_index);

        let response = self.send_request(&payload)?;
        if response.code != TS_RESPONSE_OK {
            return Err(ProtocolError::ErrorResponse(response.code));
        }
        Ok(())
    }

    /// Запись большого блока данных порциями по `chunk_size` байт.
    ///
    /// ECU scratchBuffer = BLOCKING_FACTOR + 30 (~1054 байт); каждый write-пакет занимает
    /// 7 байт overhead (C + page + offset + count) + данные, поэтому chunk_size должен
    /// быть ≤ blocking_factor ECU (обычно 1024), т.е. ≤ ~1043 байт данных.
    pub fn write_config_chunks(
        &mut self,
        page: u16,
        base_offset: u16,
        data: &[u8],
        chunk_size: usize,
        include_page_index: bool,
    ) -> Result<(), ProtocolError> {
        let chunk_size = chunk_size.max(1);
        let mut off = 0usize;
        while off < data.len() {
            let end = (off + chunk_size).min(data.len());
            let chunk = &data[off..end];
            let chunk_offset = (base_offset as usize)
                .checked_add(off)
                .and_then(|v| u16::try_from(v).ok())
                .ok_or_else(|| ProtocolError::InvalidPacket("offset overflow in chunked write".into()))?;
            self.write_config_chunk(page, chunk_offset, chunk, include_page_index)?;
            off = end;
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

    /// `l` + `TS_COMPOSITE_READ` — чтение буфера (TunerStudio `dataReadCommand`).
    ///
    /// Логгер должен быть включён заранее (`set_composite_logger_enabled(true)`).
    /// `0x84` — буфер ещё не готов. Команда `8` не используется: она сама включает логгер.
    pub fn read_composite_buffer(&mut self) -> Result<Vec<u8>, ProtocolError> {
        let payload = [TS_SET_LOGGER_SWITCH, TS_COMPOSITE_READ];
        let response = self.send_request(&payload)?;
        if response.code != TS_RESPONSE_OK {
            return Err(ProtocolError::ErrorResponse(response.code));
        }
        Ok(response.payload)
    }

    /// `l` + subcommand — вкл/выкл tooth logger (TunerStudio `startCommand` / `stopCommand`).
    pub fn set_composite_logger_enabled(&mut self, enabled: bool) -> Result<(), ProtocolError> {
        let sub = if enabled {
            crate::commands::TS_COMPOSITE_ENABLE
        } else {
            TS_COMPOSITE_DISABLE
        };
        let payload = [TS_SET_LOGGER_SWITCH, sub];
        let response = self.send_request(&payload)?;
        if response.code != TS_RESPONSE_OK {
            return Err(ProtocolError::ErrorResponse(response.code));
        }
        Ok(())
    }

    /// `l` + `TS_KNOCK_SCOPE_ENABLE` — непрерывный захват сырого KNOCK_ADC (`knock_scope.cpp`).
    pub fn set_knock_scope_enabled(&mut self, enabled: bool) -> Result<(), ProtocolError> {
        let sub = if enabled {
            crate::commands::TS_KNOCK_SCOPE_ENABLE
        } else {
            crate::commands::TS_KNOCK_SCOPE_DISABLE
        };
        let payload = [TS_SET_LOGGER_SWITCH, sub];
        let response = self.send_request(&payload)?;
        if response.code != TS_RESPONSE_OK {
            return Err(ProtocolError::ErrorResponse(response.code));
        }
        Ok(())
    }

    /// `l` + `TS_KNOCK_SCOPE_READ` — batch v2 + кадры (размер по факту, не фикс. 8192).
    pub fn read_knock_scope_buffer(&mut self) -> Result<Vec<u8>, ProtocolError> {
        const KNOCK_SCOPE_READ_TIMEOUT_MS: u64 = 300;
        let payload = [
            TS_SET_LOGGER_SWITCH,
            crate::commands::TS_KNOCK_SCOPE_READ,
        ];
        let response = self.send_request_with_timeout(&payload, KNOCK_SCOPE_READ_TIMEOUT_MS)?;
        if response.code != TS_RESPONSE_OK {
            return Err(ProtocolError::ErrorResponse(response.code));
        }
        Ok(response.payload)
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

    /// `G` — текстовый буфер после `execute_console_command` (Java `requestPendingTextMessages`).
    pub fn get_console_text(&mut self) -> Result<String, ProtocolError> {
        let response = self.send_request(&[TS_GET_TEXT])?;
        if response.code != TS_RESPONSE_OK {
            return Err(ProtocolError::ErrorResponse(response.code));
        }
        Ok(decode_console_text_payload(&response.payload))
    }

    /// Сырой `G`-буфер без фильтрации `msg` — для разбора `wave_chart`
    /// (engine sniffer). Записи остаются мультиплексированными, как на проводе.
    pub fn get_console_raw(&mut self) -> Result<String, ProtocolError> {
        let response = self.send_request(&[TS_GET_TEXT])?;
        if response.code != TS_RESPONSE_OK {
            return Err(ProtocolError::ErrorResponse(response.code));
        }
        Ok(String::from_utf8_lossy(&response.payload)
            .trim_end_matches('\0')
            .to_string())
    }

    /// `E` + текст, затем `G` — одна консольная команда с ответом.
    pub fn execute_console_command_with_response(&mut self, text: &str) -> Result<String, ProtocolError> {
        self.execute_console_command(text)?;
        self.get_console_text()
    }

    /// Сырой CRC-payload из INI `[ControllerCommands]` (например `cmd_enable_self_stim`).
    pub fn send_binary_command(&mut self, payload: &[u8]) -> Result<(), ProtocolError> {
        if payload.is_empty() {
            return Err(ProtocolError::InvalidPacket("empty command payload".into()));
        }
        let response = self.send_request(payload)?;
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
            match self.transport.read(&mut buf[offset..]) {
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
        assert!(crate::is_composite_logger_io(&[b'8']));
        assert!(crate::is_composite_logger_io(&[b'l', 3]));
        assert!(crate::is_trigger_scope_io(&[b'l', 4]));
        assert!(!crate::is_composite_logger_io(&[b'l', 8]));
        assert!(crate::is_knock_scope_io(&[b'l', 8]));
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

    /// С page (`R%2i%2o%2c`) — не legacy Proteus.
    #[test]
    fn read_config_request_wire_format_with_page() {
        assert_eq!(
            pack_config_read_request(0, 0, 1024, true),
            vec![b'R', 0, 0, 0, 0, 0, 4]
        );
    }

    /// Без page (`R%2o%2c`) — загрузка page и verify после `C`, INI `560262154.ini`.
    #[test]
    fn read_config_request_wire_format_legacy_no_page() {
        assert_eq!(
            pack_config_read_request(0, 0, 1024, false),
            vec![b'R', 0, 0, 0, 4]
        );
        assert_eq!(
            pack_config_read_request(0, 416, 4, false),
            vec![b'R', 0xa0, 0x01, 0x04, 0x00]
        );
        assert_eq!(
            pack_config_read_request(0, 1024, 1024, false),
            vec![b'R', 0, 4, 0, 4]
        );
    }

    #[test]
    fn write_config_request_wire_format_with_page() {
        assert_eq!(
            pack_config_write_request(0, 412, &[0xdc, 0x05], true),
            vec![b'C', 0, 0, 0x9c, 0x01, 0x02, 0x00, 0xdc, 0x05]
        );
    }

    #[test]
    fn write_config_request_wire_format_legacy_no_page() {
        assert_eq!(
            pack_config_write_request(0, 416, &[0x09, 0x00, 0x00, 0x00], false),
            vec![b'C', 0xa0, 0x01, 0x04, 0x00, 0x09, 0x00, 0x00, 0x00]
        );
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

    #[test]
    fn get_console_text_payload_format() {
        assert_eq!([TS_GET_TEXT], [b'G']);
    }

    #[test]
    fn console_text_keeps_only_msg_records() {
        // Поток G: msg-сообщения вперемешку с engine sniffer / версией / outpin.
        let raw = "msg`first line`wave_chart`u|d|123|456`msg`second line`\
                   rusEfiVersion`rusEFI 2024@abcdef`outpin`PA0@led`";
        assert_eq!(
            super::extract_console_messages(raw),
            "first line\nsecond line"
        );
    }

    #[test]
    fn console_text_passthrough_without_frames() {
        assert_eq!(super::extract_console_messages("plain text\0"), "plain text\0");
        assert_eq!(super::extract_console_messages("  plain text  "), "plain text");
    }
}

/// Разделитель записей текстового протокола rusEFI (`LOG_DELIMITER`).
const LOG_DELIMITER: char = '`';
/// Префикс консольных сообщений (`PROTOCOL_MSG`).
const PROTOCOL_MSG: &str = "msg";

/// Буфер `G` — это мультиплексированный поток записей `<prefix>`+`` ` ``+payload+`` ` ``:
/// `msg` (консоль), `wave_chart` (engine sniffer), `outpin`, `rusEfiVersion` и т.д.
/// Консоль показывает только `msg` (как rusEFI `EngineState` → `MessagesCentral`),
/// иначе ответ забивается повторяющимся дампом engine sniffer.
fn extract_console_messages(raw: &str) -> String {
    if !raw.contains(LOG_DELIMITER) {
        // Нефреймленный текст (пустой буфер / иная прошивка) — отдаём как есть.
        return raw.trim().to_string();
    }
    let tokens: Vec<&str> = raw.split(LOG_DELIMITER).collect();
    let mut messages: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].trim() == PROTOCOL_MSG && i + 1 < tokens.len() {
            messages.push(tokens[i + 1]);
            i += 2;
        } else {
            i += 1;
        }
    }
    messages.join("\n")
}

fn decode_console_text_payload(payload: &[u8]) -> String {
    let raw = String::from_utf8_lossy(payload);
    extract_console_messages(raw.trim_end_matches('\0'))
}
