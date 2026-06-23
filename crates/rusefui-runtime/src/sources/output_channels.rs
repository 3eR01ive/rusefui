use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rusefi_ini::{
    decode_output_channels, CompositeLoggerDef, ConfigFieldKind, FieldKind, IniCurveDef, IniFile,
    IniTableDef, OutputChannels,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::session::EcuSession;

pub const DEFAULT_OUTPUT_BLOCK_SIZE: u16 = 2044;

const OUTPUT_POLL_HZ: f64 = 200.0;
const POLL_INTERVAL: Duration = Duration::from_nanos((1_000_000_000.0 / OUTPUT_POLL_HZ) as u64);
/// Чуть реже во время стимуляции — меньше конкуренции за `inner` с `E`.
const STIM_OUTPUT_POLL_HZ: f64 = 100.0;
const STIM_POLL_INTERVAL: Duration = Duration::from_nanos((1_000_000_000.0 / STIM_OUTPUT_POLL_HZ) as u64);
/// UI-события не реже опроса ECU.
const MIN_EMIT_INTERVAL: Duration = POLL_INTERVAL;

/// Источник поля `values` в [`OutputSnapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum OutputValuesSource {
    /// Последний срез с ECU (опрос output channels).
    #[default]
    Live,
    /// Интерполяция по логу в момент курсора timeline.
    LogCursor,
}

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
    /// Путь CSV-лога output channels для текущей сессии ECU.
    pub session_log_path: Option<String>,
    /// `elapsed_sec` live-головы timeline (ось времени log = CSV).
    pub timeline_live_sec: f64,
    /// Откуда взяты `values` (live ECU или курсор на логе).
    #[serde(default)]
    pub values_source: OutputValuesSource,
    /// `elapsed_sec` на оси лога для `values` (курсор / правый край окна).
    pub sample_sec: Option<f64>,
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
            session_log_path: None,
            timeline_live_sec: 0.0,
            values_source: OutputValuesSource::LogCursor,
            sample_sec: None,
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
    /// Все размеры страниц из INI; индекс 0 = INI page 1.
    pub page_sizes: Vec<u32>,
    pub page_read_has_page_index: bool,
    pub page_chunk_write_has_page_index: bool,
    pub config_fields: HashMap<String, ConfigFieldKind>,
    /// `[ControllerCommands]` — сырые CRC-payload (`cmd_enable_self_stim` и т.д.).
    pub ts_commands: HashMap<String, Vec<u8>>,
    /// 2D-таблицы из `[TableEditor]`.
    pub tables: HashMap<String, IniTableDef>,
    /// Кривые из `[CurveEditor]`.
    pub curves: HashMap<String, IniCurveDef>,
    pub inter_write_delay_ms: u16,
    pub page_activation_delay_ms: u16,
    /// Формат записи composite logger (`[LoggerDefinition]`) — размер зависит от прошивки.
    pub composite_logger: Option<CompositeLoggerDef>,
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
                    FieldKind::Array(_) => (None, "array".to_string()),
                    FieldKind::String(_) => (None, "string".to_string()),
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
            page_sizes: vec![64_000],
            page_read_has_page_index: true,
            page_chunk_write_has_page_index: true,
            config_fields: HashMap::new(),
            ts_commands: HashMap::new(),
            tables: HashMap::new(),
            curves: HashMap::new(),
            inter_write_delay_ms: 10,
            page_activation_delay_ms: 500,
            composite_logger: None,
        }
    }

    pub fn from_ini(ini: &IniFile) -> Self {
        Self {
            signature: ini.signature.clone(),
            channels: Arc::new(ini.output_channels.clone()),
            block_size: ini.output_channels.och_block_size,
            blocking_factor: ini.blocking_factor,
            page_size: ini.page_size,
            page_sizes: ini.page_sizes.clone(),
            page_read_has_page_index: ini.page_read_has_page_index,
            page_chunk_write_has_page_index: ini.page_chunk_write_has_page_index,
            config_fields: ini.config_fields.clone(),
            ts_commands: ini.ts_commands.clone(),
            tables: ini.tables.clone(),
            curves: ini.curves.clone(),
            inter_write_delay_ms: ini.inter_write_delay_ms,
            page_activation_delay_ms: ini.page_activation_delay_ms,
            composite_logger: ini.composite_logger.clone(),
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

    pub fn is_polling(&self) -> bool {
        self.running.load(Ordering::SeqCst) && self.thread.lock().unwrap().is_some()
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        // Не join(): опрос `O` может висеть на serial; join на UI-потоке замораживает WebView.
        let _ = self.thread.lock().unwrap().take();
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
        if self.is_polling() {
            return;
        }
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
            .spawn(move || {
                poll_loop(
                    session,
                    running,
                    snapshot,
                    ini,
                    block_size,
                    chunk_size,
                    on_tick,
                )
            })
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
    let last_emit_ms = AtomicU64::new(0);

    while running.load(Ordering::SeqCst) {
        let stim = session.is_stimulation_active();
        let poll_hz = if stim { STIM_OUTPUT_POLL_HZ } else { OUTPUT_POLL_HZ };

        let mut snap = OutputSnapshot {
            connected: session.is_connected(),
            poll_hz,
            raw_len: 0,
            values: HashMap::new(),
            last_error: None,
            ini_signature: ini.signature.clone(),
            ini_field_count: ini.channels.fields.len(),
            session_log_path: session.output_session_log_path(),
            timeline_live_sec: session.output_timeline_live_sec(),
            values_source: OutputValuesSource::Live,
            sample_sec: None,
        };

        if snap.connected {
            if let Some(result) = session.try_with_link(|link| {
                link.read_output_channels_full(block_size, chunk_size)
            }) {
                match result {
                    Ok(bytes) => {
                        snap.raw_len = bytes.len();
                        snap.values = decode_output_channels(&ini.channels, &bytes);
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        session.record_output_sample(ts, &snap.values);
                        snap.sample_sec = Some(snap.timeline_live_sec);
                    }
                    Err(e) => snap.last_error = Some(e),
                }
            }
        }

        *snapshot.write().unwrap() = snap.clone();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let prev = last_emit_ms.load(Ordering::Relaxed);
        if now.saturating_sub(prev) >= MIN_EMIT_INTERVAL.as_millis() as u64 {
            last_emit_ms.store(now, Ordering::Relaxed);
            on_tick(snap);
        }

        thread::sleep(if stim {
            STIM_POLL_INTERVAL
        } else {
            POLL_INTERVAL
        });
    }
}
