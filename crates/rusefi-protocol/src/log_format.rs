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
        Some(b'C' | b'Z' | b'E' | b'B') => "OK".into(),
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

/// Чтение page 0 (`R`) — сотни чанков при загрузке; в UI не показываем.
pub fn is_config_page_read(payload: &[u8]) -> bool {
    payload.first() == Some(&b'R')
}
