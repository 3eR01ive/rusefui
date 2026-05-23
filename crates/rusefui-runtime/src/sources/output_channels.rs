use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use crate::session::EcuSession;

/// Размер блока из типичного INI (`ochBlockSize`), скелет без парсера INI.
pub const DEFAULT_OUTPUT_BLOCK_SIZE: u16 = 2044;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Поля из `rusefi_f407-discovery.ini` [OutputChannels] — позже заменить парсером INI.
struct FieldDef {
    name: &'static str,
    offset: usize,
    scale: f64,
    signed: bool,
}

const SKELETON_FIELDS: &[FieldDef] = &[
    FieldDef {
        name: "RPMValue",
        offset: 4,
        scale: 1.0,
        signed: false,
    },
    FieldDef {
        name: "coolant",
        offset: 14,
        scale: 0.01,
        signed: true,
    },
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputSnapshot {
    pub connected: bool,
    pub poll_hz: f64,
    pub raw_len: usize,
    pub values: HashMap<String, f64>,
    pub last_error: Option<String>,
}

impl OutputSnapshot {
    pub fn disconnected() -> Self {
        Self {
            connected: false,
            poll_hz: 0.0,
            raw_len: 0,
            values: HashMap::new(),
            last_error: None,
        }
    }
}

pub struct OutputChannelsSource {
    snapshot: Arc<RwLock<OutputSnapshot>>,
    running: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl OutputChannelsSource {
    pub fn new() -> Self {
        Self {
            snapshot: Arc::new(RwLock::new(OutputSnapshot::disconnected())),
            running: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> OutputSnapshot {
        self.snapshot.read().unwrap().clone()
    }

    pub fn snapshot_json(&self) -> Value {
        serde_json::to_value(self.snapshot()).unwrap_or(json!({}))
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        *self.snapshot.write().unwrap() = OutputSnapshot::disconnected();
    }

    /// Фоновый poll команды `O`; `on_tick` вызывается из потока (например emit в Tauri).
    pub fn start<F>(&self, session: Arc<EcuSession>, on_tick: F)
    where
        F: Fn(OutputSnapshot) + Send + Sync + 'static,
    {
        self.stop();
        self.running.store(true, Ordering::SeqCst);

        let running = Arc::clone(&self.running);
        let snapshot = Arc::clone(&self.snapshot);
        let on_tick = Arc::new(on_tick);

        let handle = thread::Builder::new()
            .name("rusefui-output-poll".into())
            .spawn(move || poll_loop(session, running, snapshot, on_tick))
            .expect("spawn output poll thread");

        *self.thread.lock().unwrap() = Some(handle);
    }
}

fn poll_loop(
    session: Arc<EcuSession>,
    running: Arc<AtomicBool>,
    snapshot: Arc<RwLock<OutputSnapshot>>,
    on_tick: Arc<dyn Fn(OutputSnapshot) + Send + Sync>,
) {
    while running.load(Ordering::SeqCst) {
        let mut snap = OutputSnapshot {
            connected: session.is_connected(),
            poll_hz: 1.0 / POLL_INTERVAL.as_secs_f64(),
            raw_len: 0,
            values: HashMap::new(),
            last_error: None,
        };

        if snap.connected {
            match session.with_link(|link| {
                link.read_output_channels(0, DEFAULT_OUTPUT_BLOCK_SIZE)
            }) {
                Ok(bytes) => {
                    snap.raw_len = bytes.len();
                    snap.values = decode_fields(&bytes);
                }
                Err(e) => {
                    snap.last_error = Some(e);
                }
            }
        }

        *snapshot.write().unwrap() = snap.clone();
        on_tick(snap);

        thread::sleep(POLL_INTERVAL);
    }
}

fn decode_fields(bytes: &[u8]) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for def in SKELETON_FIELDS {
        if let Some(v) = read_field(bytes, def) {
            out.insert(def.name.to_string(), v);
        }
    }
    out
}

fn read_field(bytes: &[u8], def: &FieldDef) -> Option<f64> {
    if def.offset + 2 > bytes.len() {
        return None;
    }
    let raw = if def.signed {
        i16::from_le_bytes([bytes[def.offset], bytes[def.offset + 1]]) as f64
    } else {
        u16::from_le_bytes([bytes[def.offset], bytes[def.offset + 1]]) as f64
    };
    Some(raw * def.scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_rpm_and_clt_from_zeros() {
        let mut bytes = vec![0u8; 32];
        bytes[4] = 0x40;
        bytes[5] = 0x1F; // 8000 LE u16
        let m = decode_fields(&bytes);
        assert_eq!(m.get("RPMValue"), Some(&8000.0));
    }
}
