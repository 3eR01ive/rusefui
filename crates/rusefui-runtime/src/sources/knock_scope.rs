//! Knock scope — сырой KNOCK_ADC (`l`+8/9/10), отдельный poll-поток как composite logger.
//!
//! Читать буфер циклически (`l`+10), как composite `l`+3: готовность по ответу
//! (данные или 0x84 «ещё не готов»). Опора только на `knockScopeReady` из `O` даёт
//! пропуски (флаг обнуляется сразу после READ, DMA ~18 ms, poll O 5 ms).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::knock_spectrogram::{KnockSpectrogramEngine, KnockSpectrogramView};
use crate::session::EcuSession;

const POLL_WAIT_READY: Duration = Duration::from_millis(10);
/// После READ на ECU DMA ~19 ms; затем снова `l`+8 (иначе scope гаснет → сплошной 0x84).
const REARM_AFTER_CAPTURE: Duration = Duration::from_millis(22);
const POLL_IDLE: Duration = Duration::from_millis(40);
const STATUS_EMIT_INTERVAL: Duration = Duration::from_millis(400);
const STALL_HINT_AFTER: Duration = Duration::from_secs(4);
const SERIAL_MUTEX_WAIT: Duration = Duration::from_millis(800);
/// Повторный `l`+8, если долго только 0x84 без успешного захвата.
const REARM_STALL: Duration = Duration::from_millis(400);

const READY_FIELD: &str = "knockScopeReady";
const CONFIG_ENABLE_FIELD: &str = "enableKnockScope";

/// Частота KNOCK_ADC на Proteus F4/F7.
pub const KNOCK_ADC_HZ: f64 = 218_750.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnockScopeSnapshot {
    pub connected: bool,
    pub scope_enabled: bool,
    pub polling: bool,
    pub knock_scope_ready: bool,
    pub enable_knock_scope_in_config: Option<bool>,
    pub capture_count: u64,
    pub sample_count: usize,
    pub samples: Vec<f32>,
    pub sample_min: f32,
    pub sample_max: f32,
    pub last_byte_len: usize,
    pub sample_rate_hz: f64,
    pub buffer_duration_ms: f64,
    pub status_message: Option<String>,
    pub last_error: Option<String>,
    /// FFT-спектрограмма (расчёт на хосте).
    pub spectrogram: KnockSpectrogramView,
}

impl KnockScopeSnapshot {
    pub fn disconnected() -> Self {
        Self {
            connected: false,
            scope_enabled: false,
            polling: false,
            knock_scope_ready: false,
            enable_knock_scope_in_config: None,
            capture_count: 0,
            sample_count: 0,
            samples: Vec::new(),
            sample_min: 0.0,
            sample_max: 0.0,
            last_byte_len: 0,
            sample_rate_hz: KNOCK_ADC_HZ,
            buffer_duration_ms: 0.0,
            status_message: None,
            last_error: None,
            spectrogram: KnockSpectrogramView::default(),
        }
    }
}

pub struct KnockScopeSource {
    snapshot: Arc<RwLock<KnockScopeSnapshot>>,
    spectrogram: Arc<Mutex<Option<KnockSpectrogramEngine>>>,
    running: Arc<AtomicBool>,
    scope_enabled_on_ecu: Arc<AtomicBool>,
    scope_started_at: Mutex<Option<Instant>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    tick_hook: Arc<Mutex<Option<Arc<dyn Fn(KnockScopeSnapshot) + Send + Sync>>>>,
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

fn config_enable_knock_scope(session: &EcuSession) -> Option<bool> {
    let cfg = session.config().snapshot();
    if !cfg.loaded {
        return None;
    }
    cfg.values
        .get(CONFIG_ENABLE_FIELD)
        .copied()
        .map(|v| v >= 0.5)
}

fn output_knock_scope_ready(session: &EcuSession) -> bool {
    session
        .output()
        .snapshot()
        .values
        .get(READY_FIELD)
        .copied()
        .map(|v| v >= 0.5)
        .unwrap_or(false)
}

fn status_hint(
    capture_count: u64,
    knock_ready: bool,
    config_enable: Option<bool>,
    waiting_for: Duration,
) -> Option<String> {
    if capture_count > 0 {
        return None;
    }
    match config_enable {
        Some(false) => Some(
            "На ECU enableKnockScope = no — прошивка не запускает scope. \
             Включите yes в tune и Burn."
                .into(),
        ),
        None => Some(
            "В INI нет enableKnockScope / knockScopeReady — примените свежий INI \
             (knock_scope_host) и переподключитесь."
                .into(),
        ),
        Some(true) if !knock_ready && waiting_for >= STALL_HINT_AFTER => Some(
            "Нет knockScopeReady: KNOCK_ADC занят software knock или scope сброшен на ECU. \
             Попробуйте отключить knock sensing или перезапустить scope."
                .into(),
        ),
        Some(true) if knock_ready => Some("knockScopeReady — чтение буфера…".into()),
        Some(true) => Some("Ждём knockScopeReady (~20 ms окно DMA после l+8)…".into()),
    }
}

impl KnockScopeSource {
    pub fn new() -> Self {
        Self {
            snapshot: Arc::new(RwLock::new(KnockScopeSnapshot::disconnected())),
            spectrogram: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            scope_enabled_on_ecu: Arc::new(AtomicBool::new(false)),
            scope_started_at: Mutex::new(None),
            thread: Mutex::new(None),
            tick_hook: Arc::new(Mutex::new(None)),
        }
    }

    /// Глобальный колбэк (emit во фронт). Вызывается на каждый захват вместе с `on_tick` из `start`.
    pub fn set_tick_hook<F>(&self, f: F)
    where
        F: Fn(KnockScopeSnapshot) + Send + Sync + 'static,
    {
        *self.tick_hook.lock().unwrap() = Some(Arc::new(f));
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
        *self.scope_started_at.lock().unwrap() = None;
        let mut snap = self.snapshot.write().unwrap();
        snap.scope_enabled = false;
        snap.polling = false;
        snap.knock_scope_ready = false;
        snap.status_message = None;
        snap.spectrogram = KnockSpectrogramView::default();
        *self.spectrogram.lock().unwrap() = None;
        self.scope_enabled_on_ecu.store(false, Ordering::SeqCst);
    }

    pub fn disable_on_ecu(&self, session: &EcuSession) {
        if !self.scope_enabled_on_ecu.load(Ordering::SeqCst) {
            return;
        }
        let _ = session.with_link_wait(SERIAL_MUTEX_WAIT, |link| {
            link.set_knock_scope_enabled(false)
        });
        self.scope_enabled_on_ecu.store(false, Ordering::SeqCst);
    }

    pub fn start<F>(
        &self,
        session: Arc<EcuSession>,
        window_ms: u32,
        on_tick: F,
    ) -> Result<(), String>
    where
        F: Fn(KnockScopeSnapshot) + Send + Sync + 'static,
    {
        if self.is_polling() {
            return Ok(());
        }

        let window_ms = window_ms.clamp(50, 15_000);

        session.composite().disable_on_ecu(&session);
        session.composite().stop();

        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }

        session.with_link(|link| link.set_knock_scope_enabled(true))?;
        self.scope_enabled_on_ecu.store(true, Ordering::SeqCst);
        *self.scope_started_at.lock().unwrap() = Some(Instant::now());

        let config_enable = config_enable_knock_scope(&session);
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
            snap.enable_knock_scope_in_config = config_enable;
            snap.knock_scope_ready = false;
            snap.last_error = None;
            snap.status_message = status_hint(0, false, config_enable, Duration::ZERO);
            snap.spectrogram = KnockSpectrogramView::default();
        }

        *self.spectrogram.lock().unwrap() = Some(KnockSpectrogramEngine::new(
            KNOCK_ADC_HZ as f32,
            window_ms,
        ));

        self.running.store(true, Ordering::SeqCst);

        let running = Arc::clone(&self.running);
        let snapshot = Arc::clone(&self.snapshot);
        let spectrogram = Arc::clone(&self.spectrogram);
        let tick_hook = Arc::clone(&self.tick_hook);
        let scope_started_at = Arc::new(Mutex::new(Some(Instant::now())));
        let on_tick = Arc::new(on_tick);

        let handle = thread::Builder::new()
            .name("rusefui-knock-scope-poll".into())
            .spawn(move || {
                poll_loop(
                    session,
                    running,
                    snapshot,
                    spectrogram,
                    tick_hook,
                    scope_started_at,
                    on_tick,
                )
            })
            .expect("spawn knock scope poll thread");

        *self.thread.lock().unwrap() = Some(handle);
        Ok(())
    }
}

fn poll_loop(
    session: Arc<EcuSession>,
    running: Arc<AtomicBool>,
    snapshot: Arc<RwLock<KnockScopeSnapshot>>,
    spectrogram: Arc<Mutex<Option<KnockSpectrogramEngine>>>,
    tick_hook: Arc<Mutex<Option<Arc<dyn Fn(KnockScopeSnapshot) + Send + Sync>>>>,
    scope_started_at: Arc<Mutex<Option<Instant>>>,
    on_tick: Arc<dyn Fn(KnockScopeSnapshot) + Send + Sync>,
) {
    let emit = |snap: &KnockScopeSnapshot| {
        on_tick(snap.clone());
        if let Some(hook) = tick_hook.lock().unwrap().as_ref() {
            hook(snap.clone());
        }
    };
    let mut last_status_emit = Instant::now();
    let mut not_ready_since: Option<Instant> = None;

    while running.load(Ordering::SeqCst) {
        let connected = session.is_connected();
        let allow_poll = connected && !session.config().snapshot().loading;
        let config_enable = config_enable_knock_scope(&session);
        let knock_ready = output_knock_scope_ready(&session);
        let waiting_for = scope_started_at
            .lock()
            .unwrap()
            .map(|t| t.elapsed())
            .unwrap_or(Duration::ZERO);

        let mut last_error: Option<String> = None;
        let mut did_work = false;

        if allow_poll {
            if let Some(since) = not_ready_since {
                if since.elapsed() >= REARM_STALL
                    && snapshot.read().unwrap().capture_count > 0
                {
                    let _ = session.with_link_wait(SERIAL_MUTEX_WAIT, |link| {
                        link.set_knock_scope_enabled(true)
                    });
                    not_ready_since = None;
                    did_work = true;
                    thread::sleep(REARM_AFTER_CAPTURE);
                }
            }

            match session.with_link_wait(SERIAL_MUTEX_WAIT, |link| link.read_knock_scope_buffer())
            {
                Ok(bytes) if !bytes.is_empty() => {
                    let (samples, min_v, max_v) = parse_samples(&bytes);
                    let spec_view = {
                        let mut guard = spectrogram.lock().unwrap();
                        if let Some(eng) = guard.as_mut() {
                            eng.push_samples(&samples);
                            eng.view()
                        } else {
                            KnockSpectrogramView::default()
                        }
                    };
                    let mut snap = snapshot.write().unwrap();
                    snap.connected = connected;
                    snap.scope_enabled = true;
                    snap.polling = true;
                    snap.knock_scope_ready = output_knock_scope_ready(&session);
                    snap.enable_knock_scope_in_config = config_enable;
                    snap.last_byte_len = bytes.len();
                    snap.samples = samples;
                    snap.spectrogram = spec_view;
                    snap.sample_count = snap.samples.len();
                    snap.sample_min = min_v;
                    snap.sample_max = max_v;
                    snap.buffer_duration_ms = buffer_duration_ms(snap.sample_count);
                    snap.capture_count = snap.capture_count.saturating_add(1);
                    snap.last_error = None;
                    snap.status_message =
                        status_hint(snap.capture_count, true, config_enable, waiting_for);
                    let out = snap.clone();
                    drop(snap);
                    emit(&out);
                    not_ready_since = None;
                    did_work = true;
                    thread::sleep(REARM_AFTER_CAPTURE);
                    let _ = session.with_link_wait(SERIAL_MUTEX_WAIT, |link| {
                        link.set_knock_scope_enabled(true)
                    });
                }
                Ok(_) => {
                    not_ready_since.get_or_insert_with(Instant::now);
                    thread::sleep(POLL_WAIT_READY);
                    did_work = true;
                }
                Err(e) if is_buffer_not_ready(&e) => {
                    not_ready_since.get_or_insert_with(Instant::now);
                    thread::sleep(POLL_WAIT_READY);
                    did_work = true;
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }

        if allow_poll && !did_work && {
            snapshot.read().unwrap().capture_count == 0
        } && last_status_emit.elapsed() >= STATUS_EMIT_INTERVAL
        {
            let mut snap = snapshot.write().unwrap();
            snap.connected = connected;
            snap.scope_enabled = true;
            snap.polling = true;
            snap.knock_scope_ready = knock_ready;
            snap.enable_knock_scope_in_config = config_enable;
            snap.status_message =
                status_hint(0, knock_ready, config_enable, waiting_for);
            let out = snap.clone();
            drop(snap);
            emit(&out);
            last_status_emit = Instant::now();
            did_work = true;
        }

        if last_error.is_some() && last_status_emit.elapsed() >= STATUS_EMIT_INTERVAL {
            let mut snap = snapshot.write().unwrap();
            snap.connected = connected;
            snap.polling = running.load(Ordering::SeqCst);
            snap.knock_scope_ready = knock_ready;
            snap.enable_knock_scope_in_config = config_enable;
            snap.last_error = last_error.clone();
            snap.status_message =
                status_hint(snap.capture_count, knock_ready, config_enable, waiting_for);
            let out = snap.clone();
            drop(snap);
            emit(&out);
            last_status_emit = Instant::now();
            did_work = true;
        }

        if !did_work {
            thread::sleep(if allow_poll {
                POLL_WAIT_READY
            } else {
                POLL_IDLE
            });
        }
    }
}
