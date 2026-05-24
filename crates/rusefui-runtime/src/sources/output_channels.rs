use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rusefi_ini::{decode_output_channels, FieldKind, IniFile, OutputChannels, ScalarField};
use serde::Serialize;
use serde_json::{json, Value};

use crate::session::EcuSession;

pub const DEFAULT_OUTPUT_BLOCK_SIZE: u16 = 2044;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputSnapshot {
    pub connected: bool,
    pub poll_hz: f64,
    pub raw_len: usize,
    pub values: HashMap<String, f64>,
    pub last_error: Option<String>,
    pub ini_signature: Option<String>,
    pub ini_field_count: usize,
}

impl OutputSnapshot {
    pub fn disconnected(ini: &IniContext) -> Self {
        Self {
            connected: false,
            poll_hz: 0.0,
            raw_len: 0,
            values: HashMap::new(),
            last_error: None,
            ini_signature: ini.signature.clone(),
            ini_field_count: ini.channels.fields.len(),
        }
    }
}

#[derive(Clone)]
pub struct IniContext {
    pub signature: Option<String>,
    pub channels: Arc<OutputChannels>,
    pub block_size: u16,
    pub blocking_factor: u16,
    pub page_size: u32,
    pub page_read_has_page_index: bool,
    pub config_scalars: HashMap<String, ScalarField>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputFieldInfo {
    pub name: String,
    pub units: Option<String>,
    pub kind: String,
}

impl IniContext {
    pub fn list_output_fields(&self) -> Vec<OutputFieldInfo> {
        self.channels
            .fields
            .iter()
            .map(|f| {
                let (units, kind) = match &f.kind {
                    FieldKind::Scalar(s) => {
                        let u = if s.units.is_empty() {
                            None
                        } else {
                            Some(s.units.clone())
                        };
                        (u, "scalar".to_string())
                    }
                    FieldKind::Bits(_) => (None, "bits".to_string()),
                };
                OutputFieldInfo {
                    name: f.name.clone(),
                    units,
                    kind,
                }
            })
            .collect()
    }

    pub fn disconnected() -> Self {
        Self {
            signature: None,
            channels: Arc::new(OutputChannels {
                och_block_size: DEFAULT_OUTPUT_BLOCK_SIZE,
                fields: Vec::new(),
                by_name: HashMap::new(),
            }),
            block_size: DEFAULT_OUTPUT_BLOCK_SIZE,
            blocking_factor: 1024,
            page_size: 64_000,
            page_read_has_page_index: true,
            config_scalars: HashMap::new(),
        }
    }

    pub fn from_ini(ini: &IniFile) -> Self {
        Self {
            signature: ini.signature.clone(),
            channels: Arc::new(ini.output_channels.clone()),
            block_size: ini.output_channels.och_block_size,
            blocking_factor: ini.blocking_factor,
            page_size: ini.page_size,
            page_read_has_page_index: ini.page_read_has_page_index,
            config_scalars: ini.config_scalars.clone(),
        }
    }
}

pub struct OutputChannelsSource {
    ini: Mutex<IniContext>,
    snapshot: Arc<RwLock<OutputSnapshot>>,
    running: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl OutputChannelsSource {
    pub fn new(ini: IniContext) -> Self {
        Self {
            ini: Mutex::new(ini.clone()),
            snapshot: Arc::new(RwLock::new(OutputSnapshot::disconnected(&ini))),
            running: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
        }
    }

    pub fn ini_context(&self) -> IniContext {
        self.ini.lock().unwrap().clone()
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
        let ini = self.ini.lock().unwrap().clone();
        *self.snapshot.write().unwrap() = OutputSnapshot::disconnected(&ini);
    }

    pub fn replace_ini(&self, ini: IniContext) {
        self.stop();
        *self.ini.lock().unwrap() = ini.clone();
        *self.snapshot.write().unwrap() = OutputSnapshot::disconnected(&ini);
    }

    pub fn start<F>(&self, session: Arc<EcuSession>, on_tick: F)
    where
        F: Fn(OutputSnapshot) + Send + Sync + 'static,
    {
        self.stop();
        self.running.store(true, Ordering::SeqCst);

        let running = Arc::clone(&self.running);
        let snapshot = Arc::clone(&self.snapshot);
        let on_tick = Arc::new(on_tick);
        let ini = self.ini.lock().unwrap().clone();
        let block_size = ini.block_size;
        let chunk_size = ini.blocking_factor;

        let handle = thread::Builder::new()
            .name("rusefui-output-poll".into())
            .spawn(move || poll_loop(session, running, snapshot, ini, block_size, chunk_size, on_tick))
            .expect("spawn output poll thread");

        *self.thread.lock().unwrap() = Some(handle);
    }
}

fn poll_loop(
    session: Arc<EcuSession>,
    running: Arc<AtomicBool>,
    snapshot: Arc<RwLock<OutputSnapshot>>,
    ini: IniContext,
    block_size: u16,
    chunk_size: u16,
    on_tick: Arc<dyn Fn(OutputSnapshot) + Send + Sync>,
) {
    while running.load(Ordering::SeqCst) {
        let mut snap = OutputSnapshot {
            connected: session.is_connected(),
            poll_hz: 1.0 / POLL_INTERVAL.as_secs_f64(),
            raw_len: 0,
            values: HashMap::new(),
            last_error: None,
            ini_signature: ini.signature.clone(),
            ini_field_count: ini.channels.fields.len(),
        };

        if snap.connected {
            match session.with_link(|link| link.read_output_channels_full(block_size, chunk_size)) {
                Ok(bytes) => {
                    snap.raw_len = bytes.len();
                    snap.values = decode_output_channels(&ini.channels, &bytes);
                }
                Err(e) => snap.last_error = Some(e),
            }
        }

        *snapshot.write().unwrap() = snap.clone();
        on_tick(snap);
        thread::sleep(POLL_INTERVAL);
    }
}
