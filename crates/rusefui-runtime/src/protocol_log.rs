use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusefi_protocol::{
    command_char, describe_payload, describe_response, hex_preview, is_output_poll, CrcResponse,
    ProtocolError, ProtocolTracer,
};
use serde::{Deserialize, Serialize};

const MAX_MEMORY_ENTRIES: usize = 500;
const PAYLOAD_HEX_MAX: usize = 48;
const FRAME_HEX_MAX: usize = 64;
const OUTPUT_POLL_PAYLOAD_HEX_MAX: usize = 0;

type LogListener = Arc<dyn Fn(&ProtocolLogEntry) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolLogEntry {
    pub id: u64,
    pub timestamp_ms: u64,
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
            .filter_map(|line| serde_json::from_str(line).ok())
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

    pub fn add_listener(&self, listener: LogListener) {
        self.listeners.lock().unwrap().push(listener);
    }

    pub fn list(&self, limit: usize) -> Vec<ProtocolLogEntry> {
        let entries = self.entries.lock().unwrap();
        let take = limit.min(entries.len());
        entries.iter().rev().take(take).cloned().rev().collect()
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }

    fn push(&self, entry: ProtocolLogEntry) {
        {
            let mut entries = self.entries.lock().unwrap();
            entries.push_back(entry.clone());
            while entries.len() > MAX_MEMORY_ENTRIES {
                entries.pop_front();
            }
        }

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            if let Ok(line) = serde_json::to_string(&entry) {
                let _ = writeln!(file, "{line}");
            }
        }

        for listener in self.listeners.lock().unwrap().iter() {
            listener(&entry);
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
    ) -> ProtocolLogEntry {
        let compact = is_output_poll(payload);
        let payload_max = if compact {
            OUTPUT_POLL_PAYLOAD_HEX_MAX
        } else {
            PAYLOAD_HEX_MAX
        };
        ProtocolLogEntry {
            id: self.next_id.fetch_add(1, Ordering::SeqCst),
            timestamp_ms: now_ms(),
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
        );
        self.push(entry);
    }

    fn on_rx_ok(&self, request_payload: &[u8], frame: &[u8], response: &CrcResponse) {
        let entry = self.make_entry(
            "rx",
            command_char(request_payload),
            describe_response(request_payload, response),
            &response.payload,
            frame,
            Some(response.code),
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
        );
        self.push(entry);
    }

    fn on_info(&self, message: &str) {
        let entry = self.make_entry("info", None, message.into(), &[], &[], None);
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
}
