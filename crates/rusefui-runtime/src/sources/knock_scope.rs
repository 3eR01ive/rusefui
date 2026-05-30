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

use super::knock_spectrogram::{
    encode_knock_spectrogram_gpu_patch_b64, KnockSpectrogramEngine, KnockSpectrogramPatch,
    KnockSpectrogramView, NUM_BINS,
};
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

/// Лёгкий tick для Tauri → Vue (~KB вместо сотен KB JSON на каждый захват).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnockScopeUiTick {
    pub connected: bool,
    pub scope_enabled: bool,
    pub polling: bool,
    pub knock_scope_ready: bool,
    pub enable_knock_scope_in_config: Option<bool>,
    pub capture_count: u64,
    pub sample_count: usize,
    pub sample_min: f32,
    pub sample_max: f32,
    pub last_byte_len: usize,
    pub sample_rate_hz: f64,
    pub buffer_duration_ms: f64,
    pub status_message: Option<String>,
    pub last_error: Option<String>,
    /// Base64: `encode_knock_spectrogram_gpu` — сырой ArrayBuffer для WebGL.
    pub spectrogram_gpu_b64: Option<String>,
    pub spectrogram_width: usize,
    pub spectrogram_height: usize,
    pub spectrogram_peak_hz: Option<f32>,
    /// Max u8 в последнем GPU patch (0 = FFT в полосе knock пустой).
    pub spectrogram_patch_pixel_max: u8,
    /// Урезанный чанк волны для склеивания на UI (не весь DMA-буфер).
    pub waveform_chunk: Vec<f32>,
}

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
    /// FFT-спектрограмма (расчёт на хосте). `pixels` заполняются только для `get_snapshot` / GPU init.
    pub spectrogram: KnockSpectrogramView,
    /// Пик шума по heatmap (без копирования pixels на UI tick).
    pub spectrogram_peak_hz: Option<f32>,
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
            spectrogram_peak_hz: None,
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
    tick_hook: Arc<Mutex<Option<Arc<dyn Fn(&KnockScopeSnapshot, KnockScopeUiTick) + Send + Sync>>>>,
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

const WAVEFORM_CHUNK_MAX: usize = 128;
const UI_EMIT_MIN_INTERVAL: Duration = Duration::from_millis(33);

fn downsample_waveform(samples: &[f32], max_pts: usize) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    if samples.len() <= max_pts {
        return samples.to_vec();
    }
    let step = samples.len() as f64 / max_pts as f64;
    (0..max_pts)
        .map(|i| samples[(i as f64 * step) as usize])
        .collect()
}

fn build_ui_tick(
    snap: &KnockScopeSnapshot,
    patch: Option<KnockSpectrogramPatch>,
    waveform_chunk: Vec<f32>,
) -> KnockScopeUiTick {
    let spectrogram_patch_pixel_max = patch
        .as_ref()
        .map(|p| p.new_columns.iter().copied().max().unwrap_or(0))
        .unwrap_or(0);
    let spectrogram_gpu_b64 = patch
        .as_ref()
        .filter(|p| p.shift_left > 0 || !p.new_columns.is_empty())
        .map(|p| encode_knock_spectrogram_gpu_patch_b64(p));
    KnockScopeUiTick {
        connected: snap.connected,
        scope_enabled: snap.scope_enabled,
        polling: snap.polling,
        knock_scope_ready: snap.knock_scope_ready,
        enable_knock_scope_in_config: snap.enable_knock_scope_in_config,
        capture_count: snap.capture_count,
        sample_count: snap.sample_count,
        sample_min: snap.sample_min,
        sample_max: snap.sample_max,
        last_byte_len: snap.last_byte_len,
        sample_rate_hz: snap.sample_rate_hz,
        buffer_duration_ms: snap.buffer_duration_ms,
        status_message: snap.status_message.clone(),
        last_error: snap.last_error.clone(),
        spectrogram_gpu_b64,
        spectrogram_width: snap.spectrogram.width,
        spectrogram_height: snap.spectrogram.height,
        spectrogram_peak_hz: snap.spectrogram_peak_hz,
        spectrogram_patch_pixel_max,
        waveform_chunk,
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

    /// Глобальный колбэк (emit во фронт). Вызывается на throttled UI tick.
    pub fn set_tick_hook<F>(&self, f: F)
    where
        F: Fn(&KnockScopeSnapshot, KnockScopeUiTick) + Send + Sync + 'static,
    {
        *self.tick_hook.lock().unwrap() = Some(Arc::new(f));
    }

    pub fn snapshot(&self) -> KnockScopeSnapshot {
        self.snapshot.read().unwrap().clone()
    }

    /// Полный GPU-буфер для init WebGL (один раз при mount / enable).
    pub fn spectrogram_gpu_buffer_b64(&self) -> String {
        use super::knock_spectrogram::encode_knock_spectrogram_gpu_b64;
        let guard = self.spectrogram.lock().unwrap();
        if let Some(eng) = guard.as_ref() {
            encode_knock_spectrogram_gpu_b64(&eng.view())
        } else {
            encode_knock_spectrogram_gpu_b64(&self.snapshot.read().unwrap().spectrogram)
        }
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
        snap.spectrogram_peak_hz = None;
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
            snap.spectrogram_peak_hz = None;
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
    tick_hook: Arc<Mutex<Option<Arc<dyn Fn(&KnockScopeSnapshot, KnockScopeUiTick) + Send + Sync>>>>,
    scope_started_at: Arc<Mutex<Option<Instant>>>,
    _on_tick: Arc<dyn Fn(KnockScopeSnapshot) + Send + Sync>,
) {
    let emit = |snap: &KnockScopeSnapshot, ui: KnockScopeUiTick| {
        if let Some(hook) = tick_hook.lock().unwrap().as_ref() {
            hook(snap, ui);
        }
    };
    let mut last_status_emit = Instant::now();
    let mut last_ui_emit = Instant::now();
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
                    let (spec_w, spec_h, spec_f0, spec_fs, peak_hz) = {
                        let mut guard = spectrogram.lock().unwrap();
                        if let Some(eng) = guard.as_mut() {
                            eng.push_samples(&samples);
                            let (w, h, f0, fs) = eng.spectrogram_meta();
                            (w, h, f0, fs, eng.peak_frequency_hz())
                        } else {
                            (0, NUM_BINS, 0.0, 0.0, None)
                        }
                    };
                    let waveform = downsample_waveform(&samples, WAVEFORM_CHUNK_MAX);
                    let capture_count;
                    {
                        let mut snap = snapshot.write().unwrap();
                        snap.connected = connected;
                        snap.scope_enabled = true;
                        snap.polling = true;
                        snap.knock_scope_ready = output_knock_scope_ready(&session);
                        snap.enable_knock_scope_in_config = config_enable;
                        snap.last_byte_len = bytes.len();
                        snap.samples.clear();
                        snap.spectrogram.width = spec_w;
                        snap.spectrogram.height = spec_h;
                        snap.spectrogram.freq_start_hz = spec_f0;
                        snap.spectrogram.freq_step_hz = spec_fs;
                        snap.spectrogram.pixels.clear();
                        snap.spectrogram_peak_hz = peak_hz;
                        snap.sample_count = samples.len();
                        snap.sample_min = min_v;
                        snap.sample_max = max_v;
                        snap.buffer_duration_ms = buffer_duration_ms(snap.sample_count);
                        snap.capture_count = snap.capture_count.saturating_add(1);
                        capture_count = snap.capture_count;
                        snap.last_error = None;
                        snap.status_message =
                            status_hint(capture_count, true, config_enable, waiting_for);
                    }
                    if last_ui_emit.elapsed() >= UI_EMIT_MIN_INTERVAL {
                        let patch = {
                            let mut guard = spectrogram.lock().unwrap();
                            guard.as_mut().map(|eng| eng.take_ui_patch())
                        };
                        let snap = snapshot.read().unwrap();
                        emit(&snap, build_ui_tick(&snap, patch, waveform));
                        last_ui_emit = Instant::now();
                    }
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
            {
                let mut snap = snapshot.write().unwrap();
                snap.connected = connected;
                snap.scope_enabled = true;
                snap.polling = true;
                snap.knock_scope_ready = knock_ready;
                snap.enable_knock_scope_in_config = config_enable;
                snap.status_message =
                    status_hint(0, knock_ready, config_enable, waiting_for);
            }
            let snap = snapshot.read().unwrap();
            emit(&snap, build_ui_tick(&snap, None, Vec::new()));
            last_status_emit = Instant::now();
            did_work = true;
        }

        if last_error.is_some() && last_status_emit.elapsed() >= STATUS_EMIT_INTERVAL {
            {
                let mut snap = snapshot.write().unwrap();
                snap.connected = connected;
                snap.polling = running.load(Ordering::SeqCst);
                snap.knock_scope_ready = knock_ready;
                snap.enable_knock_scope_in_config = config_enable;
                snap.last_error = last_error.clone();
                snap.status_message =
                    status_hint(snap.capture_count, knock_ready, config_enable, waiting_for);
            }
            let snap = snapshot.read().unwrap();
            emit(&snap, build_ui_tick(&snap, None, Vec::new()));
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
