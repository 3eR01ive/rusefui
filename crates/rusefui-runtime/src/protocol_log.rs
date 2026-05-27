use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rusefi_protocol::{
    command_char, describe_payload, describe_response, hex_preview, is_high_volume_log_io,
    protocol_log_source, CrcResponse, ProtocolError, ProtocolLogSource, ProtocolTracer,
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

fn default_log_source() -> ProtocolLogSource {
    ProtocolLogSource::Command
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolLogFilterSettings {
    pub error: bool,
    pub warn: bool,
    pub info: bool,
    pub debug: bool,
    pub trace: bool,
    /// S/Q/C/B/Z/E, подключение, ошибки протокола, …
    #[serde(default = "default_true")]
    pub commands: bool,
    /// Опрос `O` (output channels).
    #[serde(default)]
    pub output: bool,
    /// Composite tooth logger + trigger scope (`8`, `l`+1…6).
    #[serde(default)]
    pub trigger: bool,
    /// Knock scope / спектрограмма (`l`+8…10).
    #[serde(default)]
    pub spectrogram: bool,
    /// Массовое чтение config (`R`).
    #[serde(default)]
    pub config: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ProtocolLogFilterSettings {
    fn default() -> Self {
        Self {
            error: true,
            warn: true,
            info: true,
            debug: false,
            trace: false,
            commands: true,
            output: false,
            trigger: false,
            spectrogram: false,
            config: false,
        }
    }
}

impl ProtocolLogFilterSettings {
    pub fn allows_level(&self, level: LogLevel) -> bool {
        match level {
            LogLevel::Error => self.error,
            LogLevel::Warn => self.warn,
            LogLevel::Info => self.info,
            LogLevel::Debug => self.debug,
            LogLevel::Trace => self.trace,
        }
    }

    pub fn allows_source(&self, source: ProtocolLogSource) -> bool {
        match source {
            ProtocolLogSource::Command => self.commands,
            ProtocolLogSource::Output => self.output,
            ProtocolLogSource::Trigger => self.trigger,
            ProtocolLogSource::Spectrogram => self.spectrogram,
            ProtocolLogSource::Config => self.config,
        }
    }

    pub fn allows_file(&self, entry: &ProtocolLogEntry) -> bool {
        if !self.allows_source(entry.source) {
            return false;
        }
        if entry.source.is_data_stream() {
            return true;
        }
        self.allows_level(entry.level)
    }

    pub fn allows_ui(&self, entry: &ProtocolLogEntry) -> bool {
        if !self.allows_source(entry.source) {
            return false;
        }
        if entry.source.is_data_stream() {
            return true;
        }
        self.allows_level(entry.level)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolLogEntry {
    pub id: u64,
    pub timestamp_ms: u64,
    #[serde(default = "default_log_level")]
    pub level: LogLevel,
    #[serde(default = "default_log_source")]
    pub source: ProtocolLogSource,
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
        let mut parsed: Vec<ProtocolLogEntry> = text
            .lines()
            .filter_map(|line| serde_json::from_str::<ProtocolLogEntry>(line).ok())
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
            .filter(|e| filters.allows_ui(e))
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

        {
            let mut entries = self.entries.lock().unwrap();
            entries.push_back(entry.clone());
            while entries.len() > MAX_MEMORY_ENTRIES {
                entries.pop_front();
            }
        }

        if filters.allows_file(&entry) {
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

        if filters.allows_ui(&entry) {
            for listener in self.listeners.lock().unwrap().iter() {
                listener(&entry);
            }
        }
    }

    fn level_for(direction: &str, payload: &[u8]) -> LogLevel {
        match direction {
            "err" => LogLevel::Error,
            "info" => LogLevel::Info,
            _ if protocol_log_source(payload, direction).is_data_stream() => LogLevel::Trace,
            _ => LogLevel::Info,
        }
    }

    fn make_entry(
        &self,
        direction: &str,
        command: Option<char>,
        summary: String,
        classify_payload: &[u8],
        display_payload: &[u8],
        frame: &[u8],
        response_code: Option<u8>,
        level: Option<LogLevel>,
    ) -> ProtocolLogEntry {
        let source = protocol_log_source(classify_payload, direction);
        let compact = is_high_volume_log_io(classify_payload);
        let payload_max = if compact {
            OUTPUT_POLL_PAYLOAD_HEX_MAX
        } else {
            PAYLOAD_HEX_MAX
        };
        ProtocolLogEntry {
            id: self.next_id.fetch_add(1, Ordering::SeqCst),
            timestamp_ms: now_ms(),
            level: level.unwrap_or_else(|| Self::level_for(direction, classify_payload)),
            source,
            direction: direction.into(),
            command: command.map(|c| c.to_string()),
            summary,
            payload_hex: hex_preview(display_payload, payload_max),
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
            request_payload,
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
            request_payload,
            &[],
            None,
            None,
        );
        self.push(entry);
    }

    fn on_info(&self, message: &str) {
        let entry = self.make_entry("info", None, message.into(), &[], &[], &[], None, None);
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
    use rusefi_protocol::is_knock_scope_io;

    #[test]
    fn append_and_list() {
        let dir = std::env::temp_dir().join(format!("rusefui-log-test-{}", now_ms()));
        let path = dir.join("protocol.log");
        let store = ProtocolLogStore::new(&path);
        store.log_info("test line");
        assert_eq!(store.list(10).len(), 1);
    }

    #[test]
    fn output_hidden_by_default() {
        let dir = std::env::temp_dir().join(format!("rusefui-log-output-{}", now_ms()));
        let path = dir.join("protocol.log");
        let store = ProtocolLogStore::new(&path);

        let entry = store.make_entry(
            "tx",
            Some('O'),
            "output offset=0 count=1024".into(),
            &[b'O', 0, 0, 0, 4],
            &[b'O', 0, 0, 0, 4],
            &[],
            None,
            None,
        );
        assert_eq!(entry.source, ProtocolLogSource::Output);
        assert_eq!(entry.level, LogLevel::Trace);
        store.push(entry);

        assert_eq!(store.list(10).len(), 0);

        let mut filters = ProtocolLogFilterSettings::default();
        filters.output = true;
        store.set_filters(filters);
        assert_eq!(store.list(10).len(), 1);
    }

    #[test]
    fn knock_scope_is_spectrogram_source() {
        assert!(is_knock_scope_io(&[b'l', 8]));
        let src = protocol_log_source(&[b'l', 10], "tx");
        assert_eq!(src, ProtocolLogSource::Spectrogram);

        let dir = std::env::temp_dir().join(format!("rusefui-log-knock-{}", now_ms()));
        let store = ProtocolLogStore::new(dir.join("protocol.log"));
        let entry = store.make_entry(
            "tx",
            Some('l'),
            "knock".into(),
            &[b'l', 8],
            &[b'l', 8],
            &[],
            None,
            None,
        );
        assert_eq!(entry.source, ProtocolLogSource::Spectrogram);
        store.push(entry);
        assert_eq!(store.list(10).len(), 0);

        let mut filters = ProtocolLogFilterSettings::default();
        filters.spectrogram = true;
        store.set_filters(filters);
        assert_eq!(store.list(10).len(), 1);
    }

    #[test]
    fn trigger_separate_from_spectrogram() {
        let tooth = protocol_log_source(&[b'8'], "tx");
        let trig = protocol_log_source(&[b'l', 4], "tx");
        let knock = protocol_log_source(&[b'l', 8], "tx");
        assert_eq!(tooth, ProtocolLogSource::Trigger);
        assert_eq!(trig, ProtocolLogSource::Trigger);
        assert_eq!(knock, ProtocolLogSource::Spectrogram);
    }
}
