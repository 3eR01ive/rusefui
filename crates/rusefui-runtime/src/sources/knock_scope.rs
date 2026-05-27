//! Knock scope — сырой KNOCK_ADC (`l`+8/9/10), отдельный poll-поток как composite logger.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;

use crate::session::EcuSession;

const POLL_WAIT_READY: Duration = Duration::from_millis(2);
const POLL_AFTER_CHUNK: Duration = Duration::from_millis(1);
const POLL_IDLE: Duration = Duration::from_millis(40);
const STATUS_EMIT_INTERVAL: Duration = Duration::from_secs(1);

/// Частота KNOCK_ADC на Proteus F4/F7.
pub const KNOCK_ADC_HZ: f64 = 218_750.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnockScopeSnapshot {
    pub connected: bool,
    pub scope_enabled: bool,
    pub polling: bool,
    pub capture_count: u64,
    pub sample_count: usize,
    pub samples: Vec<f32>,
    pub sample_min: f32,
    pub sample_max: f32,
    pub last_byte_len: usize,
    pub sample_rate_hz: f64,
    pub buffer_duration_ms: f64,
    pub last_error: Option<String>,
}

impl KnockScopeSnapshot {
    pub fn disconnected() -> Self {
        Self {
            connected: false,
            scope_enabled: false,
            polling: false,
            capture_count: 0,
            sample_count: 0,
            samples: Vec::new(),
            sample_min: 0.0,
            sample_max: 0.0,
            last_byte_len: 0,
            sample_rate_hz: KNOCK_ADC_HZ,
            buffer_duration_ms: 0.0,
            last_error: None,
        }
    }
}

pub struct KnockScopeSource {
    snapshot: Arc<RwLock<KnockScopeSnapshot>>,
    running: Arc<AtomicBool>,
    scope_enabled_on_ecu: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn parse_samples(bytes: &[u8]) -> (Vec<f32>, f32, f32) {
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut min_v = f32::MAX;
    let mut max_v = f32::MIN;
    for chunk in bytes.chunks_exact(2) {
        let raw = u16::from_le_bytes([chunk[0], chunk[1]]);
        let v = (raw & 0x0FFF) as f32;
        out.push(v);
        min_v = min_v.min(v);
        max_v = max_v.max(v);
    }
    if out.is_empty() {
        min_v = 0.0;
        max_v = 0.0;
    }
    (out, min_v, max_v)
}

fn buffer_duration_ms(sample_count: usize) -> f64 {
    if sample_count == 0 {
        0.0
    } else {
        sample_count as f64 / KNOCK_ADC_HZ * 1000.0
    }
}

fn is_buffer_not_ready(err: &str) -> bool {
    err.contains("0x84")
}

impl KnockScopeSource {
    pub fn new() -> Self {
        Self {
            snapshot: Arc::new(RwLock::new(KnockScopeSnapshot::disconnected())),
            running: Arc::new(AtomicBool::new(false)),
            scope_enabled_on_ecu: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> KnockScopeSnapshot {
        self.snapshot.read().unwrap().clone()
    }

    pub fn is_polling(&self) -> bool {
        self.running.load(Ordering::SeqCst) && self.thread.lock().unwrap().is_some()
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        let mut snap = self.snapshot.write().unwrap();
        snap.scope_enabled = false;
        snap.polling = false;
        self.scope_enabled_on_ecu.store(false, Ordering::SeqCst);
    }

    pub fn disable_on_ecu(&self, session: &EcuSession) {
        if !self.scope_enabled_on_ecu.load(Ordering::SeqCst) {
            return;
        }
        let _ = session.try_with_link(|link| link.set_knock_scope_enabled(false));
        self.scope_enabled_on_ecu.store(false, Ordering::SeqCst);
    }

    pub fn start<F>(&self, session: Arc<EcuSession>, on_tick: F) -> Result<(), String>
    where
        F: Fn(KnockScopeSnapshot) + Send + Sync + 'static,
    {
        if self.is_polling() {
            return Ok(());
        }

        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }

        session.with_link(|link| link.set_knock_scope_enabled(true))?;
        self.scope_enabled_on_ecu.store(true, Ordering::SeqCst);

        {
            let mut snap = self.snapshot.write().unwrap();
            snap.scope_enabled = true;
            snap.connected = session.is_connected();
            snap.polling = true;
            snap.capture_count = 0;
            snap.samples.clear();
            snap.sample_count = 0;
            snap.sample_min = 0.0;
            snap.sample_max = 0.0;
            snap.last_byte_len = 0;
            snap.buffer_duration_ms = 0.0;
            snap.last_error = None;
        }

        self.running.store(true, Ordering::SeqCst);

        let running = Arc::clone(&self.running);
        let snapshot = Arc::clone(&self.snapshot);
        let on_tick = Arc::new(on_tick);

        let handle = thread::Builder::new()
            .name("rusefui-knock-scope-poll".into())
            .spawn(move || poll_loop(session, running, snapshot, on_tick))
            .expect("spawn knock scope poll thread");

        *self.thread.lock().unwrap() = Some(handle);
        Ok(())
    }
}

fn poll_loop(
    session: Arc<EcuSession>,
    running: Arc<AtomicBool>,
    snapshot: Arc<RwLock<KnockScopeSnapshot>>,
    on_tick: Arc<dyn Fn(KnockScopeSnapshot) + Send + Sync>,
) {
    let mut last_status_emit = std::time::Instant::now();

    while running.load(Ordering::SeqCst) {
        let connected = session.is_connected();
        let allow_poll = connected && !session.config().snapshot().loading;
        let mut last_error: Option<String> = None;

        if allow_poll {
            if let Some(result) = session.try_with_link(|link| link.read_knock_scope_buffer()) {
                match result {
                    Ok(bytes) if !bytes.is_empty() => {
                        let (samples, min_v, max_v) = parse_samples(&bytes);
                        let mut snap = snapshot.write().unwrap();
                        snap.connected = connected;
                        snap.scope_enabled = true;
                        snap.polling = true;
                        snap.last_byte_len = bytes.len();
                        snap.samples = samples;
                        snap.sample_count = snap.samples.len();
                        snap.sample_min = min_v;
                        snap.sample_max = max_v;
                        snap.buffer_duration_ms = buffer_duration_ms(snap.sample_count);
                        snap.capture_count = snap.capture_count.saturating_add(1);
                        snap.last_error = None;
                        let out = snap.clone();
                        drop(snap);
                        on_tick(out);
                        thread::sleep(POLL_AFTER_CHUNK);
                        continue;
                    }
                    Ok(_) => {}
                    Err(e) if is_buffer_not_ready(&e) => {
                        thread::sleep(POLL_WAIT_READY);
                        continue;
                    }
                    Err(e) => last_error = Some(e),
                }
            }
        }

        if last_error.is_some() || last_status_emit.elapsed() >= STATUS_EMIT_INTERVAL {
            last_status_emit = std::time::Instant::now();
            let mut snap = snapshot.write().unwrap();
            snap.connected = connected;
            snap.polling = running.load(Ordering::SeqCst);
            snap.last_error = last_error;
            let out = snap.clone();
            drop(snap);
            on_tick(out);
        }

        thread::sleep(if allow_poll {
            POLL_WAIT_READY
        } else {
            POLL_IDLE
        });
    }
}
