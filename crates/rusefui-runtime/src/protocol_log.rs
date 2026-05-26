use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rusefi_protocol::{
    command_char, describe_payload, describe_response, hex_preview, is_composite_logger_io,
    is_config_page_read, is_output_poll, CrcResponse, ProtocolError, ProtocolTracer,
};
use serde::{Deserialize, Serialize};

const MAX_MEMORY_ENTRIES: usize = 500;
const PAYLOAD_HEX_MAX: usize = 48;
const FRAME_HEX_MAX: usize = 64;
const OUTPUT_POLL_PAYLOAD_HEX_MAX: usize = 0;

type LogListener = Arc<dyn Fn(&ProtocolLogEntry) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

fn default_log_level() -> LogLevel {
    LogLevel::Info
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolLogFilterSettings {
    pub error: bool,
    pub warn: bool,
    pub info: bool,
    pub debug: bool,
    /// Запись O-poll в файл. В UI trace не показывается никогда.
    pub trace: bool,
}

impl Default for ProtocolLogFilterSettings {
    fn default() -> Self {
        Self {
            error: true,
            warn: true,
            info: true,
            debug: false,
            trace: false,
        }
    }
}

impl ProtocolLogFilterSettings {
    pub fn allows_file(&self, level: LogLevel) -> bool {
        match level {
            LogLevel::Error => self.error,
            LogLevel::Warn => self.warn,
            LogLevel::Info => self.info,
            LogLevel::Debug => self.debug,
            LogLevel::Trace => self.trace,
        }
    }

    pub fn allows_ui(&self, level: LogLevel) -> bool {
        match level {
            LogLevel::Trace => false,
            LogLevel::Error => self.error,
            LogLevel::Warn => self.warn,
            LogLevel::Info => self.info,
            LogLevel::Debug => self.debug,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolLogEntry {
    pub id: u64,
    pub timestamp_ms: u64,
    #[serde(default = "default_log_level")]
    pub level: LogLevel,
    pub direction: String,
    pub command: Option<String>,
    pub summary: String,
    pub payload_hex: String,
    pub frame_hex: String,
    pub response_code: Option<u8>,
}

pub struct ProtocolLogStore {
    path: PathBuf,
    next_id: AtomicU64,
    entries: Mutex<VecDeque<ProtocolLogEntry>>,
    listeners: Mutex<Vec<LogListener>>,
    filters: RwLock<ProtocolLogFilterSettings>,
}

impl ProtocolLogStore {
    pub fn new(path: impl AsRef<Path>) -> Arc<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let store = Arc::new(Self {
            path,
            next_id: AtomicU64::new(1),
            entries: Mutex::new(VecDeque::with_capacity(MAX_MEMORY_ENTRIES)),
            listeners: Mutex::new(Vec::new()),
            filters: RwLock::new(ProtocolLogFilterSettings::default()),
        });
        store.load_tail_from_file();
        store
    }

    fn load_tail_from_file(&self) {
        let Ok(text) = fs::read_to_string(&self.path) else {
            return;
        };
        let filters = self.filters.read().unwrap();
        let mut parsed: Vec<ProtocolLogEntry> = text
            .lines()
            .filter_map(|line| serde_json::from_str::<ProtocolLogEntry>(line).ok())
            .filter(|e| filters.allows_ui(e.level))
            .collect();
        if parsed.is_empty() {
            return;
        }
        let max_id = parsed.iter().map(|e| e.id).max().unwrap_or(0);
        self.next_id.store(max_id.saturating_add(1), Ordering::SeqCst);
        if parsed.len() > MAX_MEMORY_ENTRIES {
            parsed = parsed.split_off(parsed.len() - MAX_MEMORY_ENTRIES);
        }
        *self.entries.lock().unwrap() = parsed.into();
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn filters(&self) -> ProtocolLogFilterSettings {
        self.filters.read().unwrap().clone()
    }

    pub fn set_filters(&self, filters: ProtocolLogFilterSettings) {
        *self.filters.write().unwrap() = filters;
    }

    pub fn add_listener(&self, listener: LogListener) {
        self.listeners.lock().unwrap().push(listener);
    }

    pub fn list(&self, limit: usize) -> Vec<ProtocolLogEntry> {
        let filters = self.filters.read().unwrap();
        let entries = self.entries.lock().unwrap();
        let filtered: Vec<ProtocolLogEntry> = entries
            .iter()
            .filter(|e| filters.allows_ui(e.level))
            .cloned()
            .collect();
        let keep = limit.min(filtered.len());
        filtered[filtered.len().saturating_sub(keep)..].to_vec()
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }

    fn push(&self, entry: ProtocolLogEntry) {
        let filters = self.filters.read().unwrap();

        if filters.allows_file(entry.level) {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
            {
                if let Ok(line) = serde_json::to_string(&entry) {
                    let _ = writeln!(file, "{line}");
                }
            }
        }

        if filters.allows_ui(entry.level) {
            {
                let mut entries = self.entries.lock().unwrap();
                entries.push_back(entry.clone());
                while entries.len() > MAX_MEMORY_ENTRIES {
                    entries.pop_front();
                }
            }

            for listener in self.listeners.lock().unwrap().iter() {
                listener(&entry);
            }
        }
    }

    fn level_for(direction: &str, payload: &[u8]) -> LogLevel {
        match direction {
            "err" => LogLevel::Error,
            "info" => LogLevel::Info,
            _ if is_output_poll(payload) || is_config_page_read(payload) || is_composite_logger_io(payload) => {
                LogLevel::Trace
            }
            _ => LogLevel::Info,
        }
    }

    fn make_entry(
        &self,
        direction: &str,
        command: Option<char>,
        summary: String,
        payload: &[u8],
        frame: &[u8],
        response_code: Option<u8>,
        level: Option<LogLevel>,
    ) -> ProtocolLogEntry {
        let compact =
            is_output_poll(payload) || is_composite_logger_io(payload);
        let payload_max = if compact {
            OUTPUT_POLL_PAYLOAD_HEX_MAX
        } else {
            PAYLOAD_HEX_MAX
        };
        ProtocolLogEntry {
            id: self.next_id.fetch_add(1, Ordering::SeqCst),
            timestamp_ms: now_ms(),
            level: level.unwrap_or_else(|| Self::level_for(direction, payload)),
            direction: direction.into(),
            command: command.map(|c| c.to_string()),
            summary,
            payload_hex: hex_preview(payload, payload_max),
            frame_hex: hex_preview(frame, FRAME_HEX_MAX),
            response_code,
        }
    }

    pub fn log_info(&self, message: &str) {
        ProtocolTracer::on_info(self, message);
    }

    /// События USB / подключения / отключения (UI + `protocol.log`).
    pub fn log_link(&self, summary: impl Into<String>) {
        let entry = self.make_entry(
            "link",
            None,
            summary.into(),
            &[],
            &[],
            None,
            Some(LogLevel::Info),
        );
        self.push(entry);
    }

    pub fn log_usb_detected(&self, entry: &rusefi_protocol::SerialPortEntry) {
        self.log_link(format!(
            "USB rusEFI: {} ({})",
            entry.port_name,
            rusefi_protocol::describe_serial_port(entry)
        ));
    }

    pub fn log_ecu_connected(&self, automatic: bool, port: &str, baud_rate: u32, signature: &str) {
        let source = if automatic { "авто" } else { "вручную" };
        self.log_link(format!(
            "Подключено ({source}): {port} @ {baud_rate} baud, signature={signature}"
        ));
    }

    pub fn log_ecu_disconnected(&self, automatic: bool, port: &str, reason: &str) {
        let source = if automatic { "авто" } else { "вручную" };
        self.log_link(format!(
            "Отключено ({source}): {port} — {reason}"
        ));
    }

    pub fn log_port_busy(&self, port: &str, detail: &str) {
        self.log_link(format!(
            "Порт {port} занят ({detail}) — вероятно TunerStudio или другое приложение"
        ));
    }
}

impl ProtocolTracer for ProtocolLogStore {
    fn on_tx(&self, payload: &[u8], frame: &[u8]) {
        let entry = self.make_entry(
            "tx",
            command_char(payload),
            describe_payload(payload),
            payload,
            frame,
            None,
            None,
        );
        self.push(entry);
    }

    fn on_rx_ok(&self, request_payload: &[u8], frame: &[u8], response: &CrcResponse) {
        let level = Self::level_for("rx", request_payload);
        let entry = self.make_entry(
            "rx",
            command_char(request_payload),
            describe_response(request_payload, response),
            &response.payload,
            frame,
            Some(response.code),
            Some(level),
        );
        self.push(entry);
    }

    fn on_rx_err(&self, request_payload: &[u8], error: &ProtocolError) {
        let entry = self.make_entry(
            "err",
            command_char(request_payload),
            error.to_string(),
            request_payload,
            &[],
            None,
            None,
        );
        self.push(entry);
    }

    fn on_info(&self, message: &str) {
        let entry = self.make_entry("info", None, message.into(), &[], &[], None, None);
        self.push(entry);
    }
}

pub fn default_log_path() -> PathBuf {
    if let Ok(path) = std::env::var("RUSEFUI_LOG_PATH") {
        return PathBuf::from(path);
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rusefui")
        .join("protocol.log")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_list() {
        let dir = std::env::temp_dir().join(format!("rusefui-log-test-{}", now_ms()));
        let path = dir.join("protocol.log");
        let store = ProtocolLogStore::new(&path);
        store.log_info("test line");
        assert_eq!(store.list(10).len(), 1);
    }

    #[test]
    fn trace_not_in_ui() {
        let dir = std::env::temp_dir().join(format!("rusefui-log-trace-{}", now_ms()));
        let path = dir.join("protocol.log");
        let store = ProtocolLogStore::new(&path);
        let mut filters = ProtocolLogFilterSettings::default();
        filters.trace = true;
        store.set_filters(filters);

        let entry = store.make_entry(
            "tx",
            Some('O'),
            "output offset=0 count=1024".into(),
            &[b'O', 0, 0, 0, 4],
            &[],
            None,
            None,
        );
        assert_eq!(entry.level, LogLevel::Trace);
        store.push(entry);

        assert_eq!(store.list(10).len(), 0);
        assert!(fs::read_to_string(&path).unwrap().contains("trace"));
    }
}
