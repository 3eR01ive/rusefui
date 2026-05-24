use serialport::{DataBits, FlowControl, Parity, SerialPort, SerialPortType, StopBits};

use crate::error::ProtocolError;
use crate::serial::SerialLink;

/// STM32 virtual COM port (ChibiOS CDC), см. `usbcfg.cpp`.
pub const RUSEFI_USB_VID: u16 = 0x0483;
pub const RUSEFI_USB_PID: u16 = 0x5740;

pub const RUSEFI_SIGNATURE_PREFIX: &str = "rusEFI ";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortEntry {
    pub port_name: String,
    pub is_rusefi_candidate: bool,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub product: Option<String>,
    pub manufacturer: Option<String>,
}

fn text_contains_rusefi(value: &str) -> bool {
    value.to_ascii_lowercase().contains("rusefi")
}

pub fn is_rusefi_usb_match(
    vid: u16,
    pid: u16,
    product: Option<&str>,
    manufacturer: Option<&str>,
) -> bool {
    if vid == RUSEFI_USB_VID && pid == RUSEFI_USB_PID {
        return true;
    }
    if let Some(product) = product.filter(|s| !s.is_empty()) {
        if text_contains_rusefi(product) {
            return true;
        }
    }
    if let Some(manufacturer) = manufacturer.filter(|s| !s.is_empty()) {
        if text_contains_rusefi(manufacturer) {
            return true;
        }
    }
    false
}

pub fn is_rusefi_candidate(entry: &SerialPortEntry) -> bool {
    entry.is_rusefi_candidate
}

pub fn is_rusefi_signature(signature: &str) -> bool {
    signature.starts_with(RUSEFI_SIGNATURE_PREFIX)
}

pub fn list_serial_ports() -> Result<Vec<SerialPortEntry>, ProtocolError> {
    let mut entries: Vec<SerialPortEntry> = serialport::available_ports()?
        .into_iter()
        .map(|info| {
            let (vid, pid, product, manufacturer, is_rusefi_candidate) = match info.port_type {
                SerialPortType::UsbPort(usb) => {
                    let is_match = is_rusefi_usb_match(
                        usb.vid,
                        usb.pid,
                        usb.product.as_deref(),
                        usb.manufacturer.as_deref(),
                    );
                    (
                        Some(usb.vid),
                        Some(usb.pid),
                        usb.product,
                        usb.manufacturer,
                        is_match,
                    )
                }
                _ => (None, None, None, None, false),
            };
            SerialPortEntry {
                port_name: info.port_name,
                is_rusefi_candidate,
                vid,
                pid,
                product,
                manufacturer,
            }
        })
        .collect();
    entries.sort_by(|a, b| a.port_name.cmp(&b.port_name));
    Ok(entries)
}

pub fn port_exists(port_name: &str) -> bool {
    serialport::available_ports()
        .map(|ports| ports.iter().any(|p| p.port_name == port_name))
        .unwrap_or(false)
}

/// Попытка эксклюзивного открытия без handshake (для проверки «занят / свободен»).
pub fn try_open_serial_port(
    port_name: &str,
    baud_rate: u32,
    timeout_ms: u64,
) -> Result<Box<dyn SerialPort>, ProtocolError> {
    let timeout = std::time::Duration::from_millis(timeout_ms);
    serialport::new(port_name, baud_rate)
        .timeout(timeout)
        .data_bits(DataBits::Eight)
        .parity(Parity::None)
        .stop_bits(StopBits::One)
        .flow_control(FlowControl::None)
        .open()
        .map_err(|e| map_serial_open_error(port_name, e))
}

/// `true`, если порт занят другим процессом (например TunerStudio).
pub fn is_port_busy(port_name: &str, baud_rate: u32) -> bool {
    matches!(
        try_open_serial_port(port_name, baud_rate, 200),
        Err(ProtocolError::PortBusy { .. })
    )
}

/// Классификация ошибки `serialport` при `open()`.
pub fn is_serial_port_busy(err: &serialport::Error) -> bool {
    use serialport::ErrorKind;

    match err.kind() {
        // Linux: EBUSY при TIOCEXCL / flock, если порт уже открыт эксклюзивно.
        ErrorKind::NoDevice => true,
        ErrorKind::Io(kind) => matches!(
            kind,
            std::io::ErrorKind::PermissionDenied
                | std::io::ErrorKind::AddrInUse
                | std::io::ErrorKind::WouldBlock
        ),
        _ => {
            let msg = err.to_string().to_ascii_lowercase();
            msg.contains("busy")
                || msg.contains("resource busy")
                || msg.contains("in use")
                || msg.contains("could not open")
                || msg.contains("access denied")
        }
    }
}

pub fn map_serial_open_error(port_name: &str, err: serialport::Error) -> ProtocolError {
    if is_serial_port_busy(&err) {
        ProtocolError::PortBusy {
            port_name: port_name.to_string(),
            detail: err.to_string(),
        }
    } else {
        ProtocolError::Serial(err)
    }
}

/// Быстрая проверка: открыть порт, handshake `S`, проверить signature rusEFI.
pub fn probe_rusefi_signature(
    port_name: &str,
    baud_rate: u32,
    timeout_ms: u64,
) -> Result<String, ProtocolError> {
    let link = SerialLink::connect(port_name, baud_rate, timeout_ms, None)?;
    let signature = link.info().signature.clone();
    if !is_rusefi_signature(&signature) {
        return Err(ProtocolError::InvalidPacket(format!(
            "unexpected signature: {signature}"
        )));
    }
    Ok(signature)
}

/// Кандидаты rusEFI по USB-метаданным; если таких нет — физические COM (без встроенных ttyS на Linux).
pub fn rusefi_port_candidates(entries: &[SerialPortEntry]) -> Vec<String> {
    let usb_matches: Vec<String> = entries
        .iter()
        .filter(|entry| entry.is_rusefi_candidate)
        .map(|entry| entry.port_name.clone())
        .collect();
    if !usb_matches.is_empty() {
        return usb_matches;
    }
    entries
        .iter()
        .filter(|entry| is_serial_probe_candidate(&entry.port_name))
        .map(|entry| entry.port_name.clone())
        .collect()
}

fn is_serial_probe_candidate(port_name: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        if port_name.starts_with("/dev/ttyS") {
            return false;
        }
    }
    let _ = port_name;
    true
}

/// Человекочитаемое описание порта для лога.
pub fn describe_serial_port(entry: &SerialPortEntry) -> String {
    let mut parts = Vec::new();
    if let (Some(vid), Some(pid)) = (entry.vid, entry.pid) {
        parts.push(format!("VID={vid:04X} PID={pid:04X}"));
    }
    if let Some(ref m) = entry.manufacturer {
        if !m.is_empty() {
            parts.push(format!("mfg={m}"));
        }
    }
    if let Some(ref p) = entry.product {
        if !p.is_empty() {
            parts.push(format!("product={p}"));
        }
    }
    if parts.is_empty() {
        "без USB-метаданных".into()
    } else {
        parts.join(", ")
    }
}

/// Снимок списка rusEFI USB-портов для дедупликации логов.
pub fn rusefi_usb_fingerprints(entries: &[SerialPortEntry]) -> Vec<String> {
    let mut fingerprints: Vec<String> = entries
        .iter()
        .filter(|e| e.is_rusefi_candidate)
        .map(|e| format!("{}|{}", e.port_name, describe_serial_port(e)))
        .collect();
    fingerprints.sort();
    fingerprints
}

#[cfg(test)]
mod tests {
    use super::*;
    use serialport::ErrorKind;

    #[test]
    fn busy_error_classification() {
        let busy = serialport::Error::new(ErrorKind::NoDevice, "Device or resource busy");
        assert!(is_serial_port_busy(&busy));

        let denied = serialport::Error::new(
            ErrorKind::Io(std::io::ErrorKind::PermissionDenied),
            "Permission denied",
        );
        assert!(is_serial_port_busy(&denied));

        let other = serialport::Error::new(ErrorKind::InvalidInput, "bad baud");
        assert!(!is_serial_port_busy(&other));
    }

    #[test]
    fn linux_excludes_builtin_ttys() {
        #[cfg(target_os = "linux")]
        {
            assert!(!is_serial_probe_candidate("/dev/ttyS0"));
            assert!(is_serial_probe_candidate("/dev/ttyACM0"));
        }
    }
}
