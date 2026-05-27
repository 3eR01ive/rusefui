pub fn command_char(payload: &[u8]) -> Option<char> {
    payload.first().map(|&b| b as char)
}

pub fn describe_payload(payload: &[u8]) -> String {
    if payload.is_empty() {
        return "empty".into();
    }
    match payload[0] {
        b'S' | b'Q' => "hello/query".into(),
        b'O' if payload.len() >= 5 => {
            let offset = u16::from_le_bytes([payload[1], payload[2]]);
            let count = u16::from_le_bytes([payload[3], payload[4]]);
            format!("output offset={offset} count={count}")
        }
        b'R' if payload.len() >= 5 => {
            if payload.len() >= 7 {
                let page = u16::from_le_bytes([payload[1], payload[2]]);
                let offset = u16::from_le_bytes([payload[3], payload[4]]);
                let count = u16::from_le_bytes([payload[5], payload[6]]);
                format!("read page={page} offset={offset} count={count}")
            } else {
                let offset = u16::from_le_bytes([payload[1], payload[2]]);
                let count = u16::from_le_bytes([payload[3], payload[4]]);
                format!("read offset={offset} count={count}")
            }
        }
        b'C' if payload.len() >= 5 => {
            let offset = u16::from_le_bytes([payload[1], payload[2]]);
            let count = u16::from_le_bytes([payload[3], payload[4]]);
            // Legacy `C%2o%2c%v`: len = 5 + count. Paged `C%2i%2o%2c%v`: len = 7 + count.
            if payload.len() == 5 + count as usize {
                format!("write offset={offset} count={count}")
            } else if payload.len() >= 7 {
                let page = u16::from_le_bytes([payload[1], payload[2]]);
                let off = u16::from_le_bytes([payload[3], payload[4]]);
                let cnt = u16::from_le_bytes([payload[5], payload[6]]);
                format!("write page={page} offset={off} count={cnt}")
            } else {
                format!("write len={}", payload.len())
            }
        }
        b'B' if payload.len() >= 3 => {
            let page = u16::from_le_bytes([payload[1], payload[2]]);
            format!("burn page={page}")
        }
        b'Z' if payload.len() >= 5 => {
            let subsystem = u16::from_be_bytes([payload[1], payload[2]]);
            let index = u16::from_be_bytes([payload[3], payload[4]]);
            format!("io_test subsystem={subsystem} index={index}")
        }
        b'E' if payload.len() > 1 => {
            let text = String::from_utf8_lossy(&payload[1..]);
            format!("execute \"{text}\"")
        }
        b'l' if payload.len() >= 2 => {
            let sub = payload[1];
            let name = match sub {
                1 => "composite enable",
                2 => "composite disable",
                3 => "composite read",
                4 => "trigger scope enable",
                5 => "trigger scope disable",
                6 => "trigger scope read",
                8 => "knock scope enable",
                9 => "knock scope disable",
                10 => "knock scope read",
                _ => "logger sub",
            };
            format!("logger {name}")
        }
        b'8' => "composite buffer".into(),
        other => format!("cmd 0x{other:02X} len={}", payload.len()),
    }
}

pub fn describe_response(request_payload: &[u8], response: &crate::packet::CrcResponse) -> String {
    if response.code == crate::commands::TS_RESPONSE_BURN_OK {
        return "BURN OK".into();
    }
    if response.code != crate::commands::TS_RESPONSE_OK {
        return format!("error code=0x{:02X}", response.code);
    }
    match request_payload.first() {
        Some(b'S' | b'Q') => {
            let sig = String::from_utf8_lossy(&response.payload)
                .trim_end_matches('\0')
                .to_string();
            if sig.len() > 80 {
                format!("OK signature={}…", &sig[..80])
            } else {
                format!("OK signature={sig}")
            }
        }
        Some(b'O' | b'R') => format!("OK {} bytes", response.payload.len()),
        Some(b'C' | b'Z' | b'E' | b'B' | b'l') => "OK".into(),
        Some(b'8') => format!("OK composite {} bytes", response.payload.len()),
        _ => format!("OK payload={} bytes", response.payload.len()),
    }
}

pub fn hex_preview(data: &[u8], max_bytes: usize) -> String {
    if data.is_empty() {
        return String::new();
    }
    let shown = data.len().min(max_bytes);
    let hex: String = data[..shown]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if data.len() > max_bytes {
        format!("{hex}… (+{} B)", data.len() - max_bytes)
    } else {
        hex
    }
}

pub fn is_output_poll(payload: &[u8]) -> bool {
    payload.first() == Some(&b'O')
}

/// Knock spectrogram / raw ADC (`l` + 8/9/10, `knock_scope.cpp`).
pub fn is_knock_scope_io(payload: &[u8]) -> bool {
    matches!(payload, [b'l', 8] | [b'l', 9] | [b'l', 10])
}

/// Trigger scope (`l` + 4/5/6, `trigger_scope.cpp`).
pub fn is_trigger_scope_io(payload: &[u8]) -> bool {
    matches!(payload, [b'l', 4] | [b'l', 5] | [b'l', 6])
}

/// Tooth / composite logger (`8`, `l` + 1/2/3).
pub fn is_composite_tooth_io(payload: &[u8]) -> bool {
    match payload.first() {
        Some(b'8') => true,
        Some(b'l') => matches!(payload.get(1), Some(1) | Some(2) | Some(3)),
        _ => false,
    }
}

/// Tooth + trigger scope (без knock spectrogram).
pub fn is_composite_logger_io(payload: &[u8]) -> bool {
    is_composite_tooth_io(payload) || is_trigger_scope_io(payload)
}

/// Чтение page 0 (`R`) — сотни чанков при загрузке config.
pub fn is_config_page_read(payload: &[u8]) -> bool {
    payload.first() == Some(&b'R')
}

/// Источник записи в протокольном логе (фильтр UI / файла).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProtocolLogSource {
    /// Обычные команды: S/Q/R/C/B/Z/E, link, info, …
    Command,
    /// Опрос `O` (output channels).
    Output,
    /// Composite + trigger scope.
    Trigger,
    /// Knock scope (сырой ADC для спектрограммы).
    Spectrogram,
    /// Массовое чтение config (`R` page 0).
    Config,
}

impl ProtocolLogSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Output => "output",
            Self::Trigger => "trigger",
            Self::Spectrogram => "spectrogram",
            Self::Config => "config",
        }
    }

    /// Высокочастотный поток — в UI в основном по галочке источника, не по level trace.
    pub fn is_data_stream(self) -> bool {
        matches!(
            self,
            Self::Output | Self::Trigger | Self::Spectrogram | Self::Config
        )
    }
}

pub fn protocol_log_source(payload: &[u8], direction: &str) -> ProtocolLogSource {
    if matches!(direction, "link" | "info") {
        return ProtocolLogSource::Command;
    }
    if is_output_poll(payload) {
        return ProtocolLogSource::Output;
    }
    if is_knock_scope_io(payload) {
        return ProtocolLogSource::Spectrogram;
    }
    if is_trigger_scope_io(payload) || is_composite_tooth_io(payload) {
        return ProtocolLogSource::Trigger;
    }
    if is_config_page_read(payload) {
        return ProtocolLogSource::Config;
    }
    ProtocolLogSource::Command
}

pub fn is_high_volume_log_io(payload: &[u8]) -> bool {
    protocol_log_source(payload, "tx").is_data_stream()
}
