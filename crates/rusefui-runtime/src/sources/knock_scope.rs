//! Knock scope — transport for software_knock per-cylinder windows (`l`+8/9/10).
//!
//! Читать буфер циклически (`l`+10), как composite `l`+3: готовность по ответу
//! (данные или 0x84 «ещё не готов»). Захват на ECU идёт через тот же пайплайн,
//! что и software knock (окна по углу коленвала, цилиндр, канал).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::knock_spectrogram::{
    encode_knock_spectrogram_gpu_patch_b64, spectrogram_height_bins, KnockSpectrogramEngine,
    KnockSpectrogramMarker, KnockSpectrogramPatch, KnockSpectrogramView,
};
use crate::session::EcuSession;

/// Буфер ECU ещё не готов (0x84 / пустой ответ) — короткая пауза и снова read.
const POLL_NOT_READY: Duration = Duration::from_micros(500);
/// Порт занят (output poll и т.д.) — yield и быстрый retry.
const POLL_SERIAL_BUSY: Duration = Duration::from_micros(200);
const POLL_IDLE: Duration = Duration::from_millis(40);
const STATUS_EMIT_INTERVAL: Duration = Duration::from_millis(400);
const STALL_HINT_AFTER: Duration = Duration::from_secs(4);
/// Включение/выключение scope на ECU — редко, можно подождать порт.
const SERIAL_MUTEX_WAIT: Duration = Duration::from_millis(200);

const READY_FIELD: &str = "knockScopeReady";
const CONFIG_ENABLE_FIELD: &str = "enableKnockScope";

/// Частота KNOCK_ADC на Proteus F4/F7.
pub const KNOCK_ADC_HZ: f64 = 218_750.0;

const KNOCK_SCOPE_FRAME_VERSION: u8 = 1;
const KNOCK_SCOPE_FRAME_HEADER_SIZE: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KnockScopeFrameHeader {
    version: u8,
    cylinder_number: u8,
    channel_number: u8,
    sample_count: u16,
}

#[derive(Debug, Clone, PartialEq)]
struct KnockScopeFrame {
    header: KnockScopeFrameHeader,
    samples: Vec<f32>,
}

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
    pub last_cylinder: Option<u8>,
    pub last_channel: Option<u8>,
    pub spectrogram_total_columns: usize,
    pub spectrogram_view_start: usize,
    pub spectrogram_view_captures: usize,
    pub spectrogram_follow_live: bool,
    /// Wall-clock от `start()` записи (мс).
    pub recording_elapsed_ms: u64,
    /// Захватов/с по wall-clock (`capture_count / elapsed`).
    pub capture_rate_hz: f64,
    /// RPM из output poll на старте записи (для оценки теор. частоты).
    pub recording_ref_rpm: Option<f64>,
    /// Теор. захватов/с: `rpm × N_цил / 120` (4-такт), N = max(виденный цилиндр)+1.
    pub expected_capture_rate_hz: Option<f64>,
    /// Base64: `encode_knock_spectrogram_gpu` — сырой ArrayBuffer для WebGL.
    pub spectrogram_gpu_b64: Option<String>,
    pub spectrogram_width: usize,
    pub spectrogram_height: usize,
    pub spectrogram_peak_hz: Option<f32>,
    /// Max u8 в последнем GPU patch (0 = FFT в полосе knock пустой).
    pub spectrogram_patch_pixel_max: u8,
    /// Вертикальные метки смены цилиндра в текущем окне heatmap.
    pub spectrogram_markers: Vec<KnockSpectrogramMarker>,
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
    pub last_cylinder: Option<u8>,
    pub last_channel: Option<u8>,
    pub spectrogram_total_columns: usize,
    pub spectrogram_view_start: usize,
    pub spectrogram_view_captures: usize,
    pub spectrogram_follow_live: bool,
    pub recording_elapsed_ms: u64,
    pub capture_rate_hz: f64,
    pub recording_ref_rpm: Option<f64>,
    pub expected_capture_rate_hz: Option<f64>,
    /// FFT-спектрограмма (расчёт на хосте). `pixels` заполняются только для `get_snapshot` / GPU init.
    pub spectrogram: KnockSpectrogramView,
    /// Пик шума по heatmap (без копирования pixels на UI tick).
    pub spectrogram_peak_hz: Option<f32>,
    pub spectrogram_markers: Vec<KnockSpectrogramMarker>,
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
            last_cylinder: None,
            last_channel: None,
            spectrogram_total_columns: 0,
            spectrogram_view_start: 0,
            spectrogram_view_captures: 0,
            spectrogram_follow_live: true,
            recording_elapsed_ms: 0,
            capture_rate_hz: 0.0,
            recording_ref_rpm: None,
            expected_capture_rate_hz: None,
            spectrogram: KnockSpectrogramView::default(),
            spectrogram_peak_hz: None,
            spectrogram_markers: Vec::new(),
        }
    }
}

pub struct KnockScopeSource {
    snapshot: Arc<RwLock<KnockScopeSnapshot>>,
    spectrogram: Arc<Mutex<Option<KnockSpectrogramEngine>>>,
    running: Arc<AtomicBool>,
    scope_enabled_on_ecu: Arc<AtomicBool>,
    scope_started_at: Mutex<Option<Instant>>,
    recording_ref_rpm: Mutex<Option<f64>>,
    max_cylinder_seen: Mutex<Option<u8>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    tick_hook: Arc<Mutex<Option<Arc<dyn Fn(&KnockScopeSnapshot, KnockScopeUiTick) + Send + Sync>>>>,
    stop_hook: Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>,
}

fn adc_samples_to_volts(raw: u16) -> f32 {
    (raw & 0x0FFF) as f32
}

fn parse_adc_payload(bytes: &[u8]) -> (Vec<f32>, f32, f32) {
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut min_v = f32::MAX;
    let mut max_v = f32::MIN;
    for chunk in bytes.chunks_exact(2) {
        let raw = u16::from_le_bytes([chunk[0], chunk[1]]);
        let v = adc_samples_to_volts(raw);
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

fn parse_knock_scope_frame(bytes: &[u8]) -> Option<KnockScopeFrame> {
    if bytes.len() < KNOCK_SCOPE_FRAME_HEADER_SIZE {
        return None;
    }
    let version = bytes[0];
    if version != KNOCK_SCOPE_FRAME_VERSION {
        return None;
    }
    let cylinder_number = bytes[1];
    let channel_number = bytes[2];
    let sample_count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    let payload = &bytes[KNOCK_SCOPE_FRAME_HEADER_SIZE..];
    let payload_samples = payload.len() / 2;
    if sample_count == 0 || sample_count > payload_samples {
        return None;
    }
    let (samples, _, _) = parse_adc_payload(&payload[..sample_count * 2]);
    Some(KnockScopeFrame {
        header: KnockScopeFrameHeader {
            version,
            cylinder_number,
            channel_number,
            sample_count: sample_count as u16,
        },
        samples,
    })
}

/// Разбор ответа ECU: v1 header + payload или legacy raw ADC (старые прошивки).
fn decode_knock_scope_buffer(bytes: &[u8]) -> KnockScopeFrame {
    if let Some(frame) = parse_knock_scope_frame(bytes) {
        return frame;
    }
    let (samples, _, _) = parse_adc_payload(bytes);
    KnockScopeFrame {
        header: KnockScopeFrameHeader {
            version: 0,
            cylinder_number: 0,
            channel_number: 0,
            sample_count: samples.len() as u16,
        },
        samples,
    }
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
        last_cylinder: snap.last_cylinder,
        last_channel: snap.last_channel,
        spectrogram_total_columns: snap.spectrogram_total_columns,
        spectrogram_view_start: snap.spectrogram_view_start,
        spectrogram_view_captures: snap.spectrogram_view_captures,
        spectrogram_follow_live: snap.spectrogram_follow_live,
        recording_elapsed_ms: snap.recording_elapsed_ms,
        capture_rate_hz: snap.capture_rate_hz,
        recording_ref_rpm: snap.recording_ref_rpm,
        expected_capture_rate_hz: snap.expected_capture_rate_hz,
        spectrogram_gpu_b64,
        spectrogram_width: snap.spectrogram.width,
        spectrogram_height: snap.spectrogram.height,
        spectrogram_peak_hz: snap.spectrogram_peak_hz,
        spectrogram_patch_pixel_max,
        spectrogram_markers: snap.spectrogram_markers.clone(),
        waveform_chunk,
    }
}

fn expected_capture_rate_hz(rpm: f64, max_cylinder_index: u8) -> f64 {
    let n = f64::from(max_cylinder_index.saturating_add(1)).max(1.0);
    rpm * n / 120.0
}

fn recording_timing(
    started: Option<Instant>,
    capture_count: u64,
    ref_rpm: Option<f64>,
    max_cylinder: Option<u8>,
) -> (u64, f64, Option<f64>) {
    let Some(t0) = started else {
        return (0, 0.0, None);
    };
    let elapsed_ms = t0.elapsed().as_millis() as u64;
    let rate = if elapsed_ms > 0 {
        capture_count as f64 / (elapsed_ms as f64 / 1000.0)
    } else {
        0.0
    };
    let expected = ref_rpm.and_then(|rpm| {
        max_cylinder.map(|c| expected_capture_rate_hz(rpm, c))
    });
    (elapsed_ms, rate, expected)
}

fn apply_recording_timing(snap: &mut KnockScopeSnapshot, started: Option<Instant>, ref_rpm: Option<f64>, max_cylinder: Option<u8>) {
    let (elapsed_ms, rate, expected) =
        recording_timing(started, snap.capture_count, ref_rpm, max_cylinder);
    snap.recording_elapsed_ms = elapsed_ms;
    snap.capture_rate_hz = rate;
    snap.recording_ref_rpm = ref_rpm;
    snap.expected_capture_rate_hz = expected;
}

fn apply_engine_viewport(snap: &mut KnockScopeSnapshot, eng: &KnockSpectrogramEngine) {
    let (total, view_start, view_width, follow_live) = eng.viewport_stats();
    let (_, height) = eng.spectrogram_meta();
    snap.spectrogram.width = view_width;
    snap.spectrogram.height = height;
    snap.spectrogram_total_columns = total;
    snap.spectrogram_view_start = view_start;
    snap.spectrogram_view_captures = view_width;
    snap.spectrogram_follow_live = follow_live;
    snap.spectrogram_peak_hz = eng.peak_frequency_hz();
    snap.spectrogram_markers = eng.visible_markers();
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
            "На ECU enableKnockScope = no — прошивка не публикует окна. \
             Включите yes в tune и Burn."
                .into(),
        ),
        None => Some(
            "В INI нет enableKnockScope / knockScopeReady — примените свежий INI \
             (knock_scope_host) и переподключитесь."
                .into(),
        ),
        Some(true) if !knock_ready && waiting_for >= STALL_HINT_AFTER => Some(
            "Нет knockScopeReady: нужен software knock и работающий двигатель \
             (окна по углу после искры). Проверьте enableSoftwareKnock."
                .into(),
        ),
        Some(true) if knock_ready => Some("knockScopeReady — чтение окна…".into()),
        Some(true) => Some("Ждём knockScopeReady (окно software knock после искры)…".into()),
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
            recording_ref_rpm: Mutex::new(None),
            max_cylinder_seen: Mutex::new(None),
            thread: Mutex::new(None),
            tick_hook: Arc::new(Mutex::new(None)),
            stop_hook: Arc::new(Mutex::new(None)),
        }
    }

    /// После `stop` / `stop_recording` (перезапуск output poll и т.д.).
    pub fn set_stop_hook<F>(&self, f: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        *self.stop_hook.lock().unwrap() = Some(Arc::new(f));
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

    /// Идёт запись спектрограммы (poll-поток активен). Без `start()` — всегда false.
    pub fn is_polling(&self) -> bool {
        self.running.load(Ordering::SeqCst) && self.thread.lock().unwrap().is_some()
    }

    /// Остановить poll-поток (без serial). Вызывается только из `stop` / `stop_recording`.
    fn stop_host_poll(&self) {
        self.running.store(false, Ordering::SeqCst);
        let _ = self.thread.lock().unwrap().take();
        *self.scope_started_at.lock().unwrap() = None;
        *self.recording_ref_rpm.lock().unwrap() = None;
        *self.max_cylinder_seen.lock().unwrap() = None;
    }

    /// Выключить knock scope на ECU, не блокируя serial (для смены проекта / gate).
    pub fn try_disable_on_ecu(&self, session: &EcuSession) {
        if !self.scope_enabled_on_ecu.load(Ordering::SeqCst) {
            return;
        }
        match session.try_with_link(|link| link.set_knock_scope_enabled(false)) {
            Some(Ok(())) => {
                self.scope_enabled_on_ecu.store(false, Ordering::SeqCst);
            }
            Some(Err(_)) | None => {
                // Порт занят — host-запись уже остановлена; ECU погасим при следующем успешном serial.
                self.scope_enabled_on_ecu.store(false, Ordering::SeqCst);
            }
        }
    }

    /// Остановить запись: poll + заморозить heatmap; ECU scope — отдельно (`try_disable_on_ecu` / `disable_on_ecu`).
    pub fn stop(&self) {
        self.stop_host_poll();

        let frozen = self.spectrogram.lock().unwrap().as_ref().map(|eng| {
            (
                eng.view(),
                eng.visible_markers(),
                eng.peak_frequency_hz(),
                eng.viewport_stats(),
            )
        });

        let mut snap = self.snapshot.write().unwrap();
        snap.scope_enabled = false;
        snap.polling = false;
        if let Some((view, markers, peak, (total, vs, vw, follow))) = frozen {
            snap.spectrogram = view;
            snap.spectrogram_markers = markers;
            snap.spectrogram_peak_hz = peak;
            snap.spectrogram_total_columns = total;
            snap.spectrogram_view_start = vs;
            snap.spectrogram_view_captures = vw;
            snap.spectrogram_follow_live = follow;
            snap.status_message = Some("Запись остановлена — спектрограмма на экране.".into());
        } else {
            snap.status_message = Some("Запись остановлена.".into());
        }
    }

    /// Стоп записи + попытка выключить scope на ECU (не блокирует надолго).
    pub fn stop_recording(&self, session: &EcuSession) {
        self.stop();
        self.try_disable_on_ecu(session);
        session.request_output_poll_resync();
        if let Some(hook) = self.stop_hook.lock().unwrap().as_ref() {
            hook();
        }
    }

    /// Выключить scope на ECU (кнопка «Стоп»); при занятом порте — короткое ожидание.
    pub fn disable_on_ecu(&self, session: &EcuSession) {
        if !self.scope_enabled_on_ecu.load(Ordering::SeqCst) {
            return;
        }
        if let Some(Ok(())) = session.try_with_link(|link| link.set_knock_scope_enabled(false)) {
            self.scope_enabled_on_ecu.store(false, Ordering::SeqCst);
            return;
        }
        let _ = session.with_link_wait(SERIAL_MUTEX_WAIT, |link| {
            link.set_knock_scope_enabled(false)
        });
        self.scope_enabled_on_ecu.store(false, Ordering::SeqCst);
    }

    /// Сдвинуть viewport по записи (столбцы FFT). `delta > 0` — к более новым захватам.
    pub fn pan_spectrogram_view(&self, delta_columns: i32) -> KnockScopeSnapshot {
        if let Some(eng) = self.spectrogram.lock().unwrap().as_mut() {
            eng.pan_view(delta_columns);
            let mut snap = self.snapshot.write().unwrap();
            apply_engine_viewport(&mut snap, eng);
        }
        self.snapshot.read().unwrap().clone()
    }

    /// Прижать viewport к хвосту записи (live) или отключить follow.
    pub fn set_spectrogram_follow_live(&self, follow: bool) -> KnockScopeSnapshot {
        if let Some(eng) = self.spectrogram.lock().unwrap().as_mut() {
            eng.set_follow_live(follow);
            let mut snap = self.snapshot.write().unwrap();
            apply_engine_viewport(&mut snap, eng);
        }
        self.snapshot.read().unwrap().clone()
    }

    /// Полный GPU-буфер текущего viewport (после pan / follow).
    pub fn spectrogram_viewport_gpu_b64(&self) -> String {
        use super::knock_spectrogram::encode_knock_spectrogram_gpu_b64;
        let guard = self.spectrogram.lock().unwrap();
        if let Some(eng) = guard.as_ref() {
            encode_knock_spectrogram_gpu_b64(&eng.view())
        } else {
            encode_knock_spectrogram_gpu_b64(&self.snapshot.read().unwrap().spectrogram)
        }
    }

    /// UI tick с полным GPU viewport (после pan / follow live).
    pub fn viewport_refresh_ui_tick(&self) -> KnockScopeUiTick {
        let snap = self.snapshot.read().unwrap().clone();
        let mut tick = build_ui_tick(&snap, None, Vec::new());
        let gpu = self.spectrogram_viewport_gpu_b64();
        if !gpu.is_empty() {
            tick.spectrogram_gpu_b64 = Some(gpu);
        }
        tick
    }

    /// Начать запись: поднимает poll-поток. Без вызова `start` knock scope на хосте не опрашивается.
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

        let ref_rpm = session
            .output()
            .snapshot()
            .values
            .get("RPMValue")
            .copied()
            .filter(|rpm| *rpm > 0.0);
        // Serial целиком под knock read — иначе poll `O` (~5 ms) съедает окна @ высоких RPM.
        session.output().stop();
        *self.recording_ref_rpm.lock().unwrap() = ref_rpm;
        *self.max_cylinder_seen.lock().unwrap() = None;

        self.running.store(false, Ordering::SeqCst);
        let _ = self.thread.lock().unwrap().take();

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
            snap.last_cylinder = None;
            snap.last_channel = None;
            snap.status_message = status_hint(0, false, config_enable, Duration::ZERO);
            snap.spectrogram = KnockSpectrogramView::default();
            snap.spectrogram_peak_hz = None;
            snap.spectrogram_markers.clear();
            snap.spectrogram_total_columns = 0;
            snap.spectrogram_view_start = 0;
            snap.spectrogram_view_captures = 0;
            snap.spectrogram_follow_live = true;
            snap.recording_elapsed_ms = 0;
            snap.capture_rate_hz = 0.0;
            snap.recording_ref_rpm = ref_rpm;
            snap.expected_capture_rate_hz = None;
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
        let recording_ref_rpm = Arc::new(Mutex::new(ref_rpm));
        let max_cylinder_seen = Arc::new(Mutex::new(None::<u8>));
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
                    recording_ref_rpm,
                    max_cylinder_seen,
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
    recording_ref_rpm: Arc<Mutex<Option<f64>>>,
    max_cylinder_seen: Arc<Mutex<Option<u8>>>,
    _on_tick: Arc<dyn Fn(KnockScopeSnapshot) + Send + Sync>,
) {
    let emit = |snap: &KnockScopeSnapshot, ui: KnockScopeUiTick| {
        if let Some(hook) = tick_hook.lock().unwrap().as_ref() {
            hook(snap, ui);
        }
    };
    let mut last_status_emit = Instant::now();
    let mut last_ui_emit = Instant::now();

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
            match session.try_with_link(|link| link.read_knock_scope_buffer()) {
                Some(Ok(bytes)) if !bytes.is_empty() => {
                    let frame = decode_knock_scope_buffer(&bytes);
                    let samples = frame.samples;
                    let min_v = samples.iter().copied().fold(f32::INFINITY, f32::min);
                    let max_v = samples.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                    let (min_v, max_v) = if samples.is_empty() {
                        (0.0, 0.0)
                    } else {
                        (min_v, max_v)
                    };
                    let cylinder = frame.header.cylinder_number;
                    let channel = frame.header.channel_number;
                    let (spec_w, spec_h, peak_hz, markers, viewport) = {
                        let mut guard = spectrogram.lock().unwrap();
                        if let Some(eng) = guard.as_mut() {
                            eng.push_samples_with_marker(&samples, cylinder, channel);
                            let (w, h) = eng.spectrogram_meta();
                            (
                                w,
                                h,
                                eng.peak_frequency_hz(),
                                eng.visible_markers(),
                                eng.viewport_stats(),
                            )
                        } else {
                            (
                                0,
                                spectrogram_height_bins(KNOCK_ADC_HZ as f32),
                                None,
                                Vec::new(),
                                (0, 0, 0, true),
                            )
                        }
                    };
                    let (total_cols, view_start, view_captures, follow_live) = viewport;
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
                        snap.spectrogram.pixels.clear();
                        snap.spectrogram_peak_hz = peak_hz;
                        snap.spectrogram_markers = markers;
                        snap.spectrogram_total_columns = total_cols;
                        snap.spectrogram_view_start = view_start;
                        snap.spectrogram_view_captures = view_captures;
                        snap.spectrogram_follow_live = follow_live;
                        snap.sample_count = samples.len();
                        snap.sample_min = min_v;
                        snap.sample_max = max_v;
                        snap.buffer_duration_ms = buffer_duration_ms(snap.sample_count);
                        snap.last_cylinder = Some(cylinder);
                        snap.last_channel = Some(channel);
                        snap.capture_count = snap.capture_count.saturating_add(1);
                        capture_count = snap.capture_count;
                        snap.last_error = None;
                        {
                            let mut mx = max_cylinder_seen.lock().unwrap();
                            *mx = Some(mx.map(|m| m.max(cylinder)).unwrap_or(cylinder));
                        }
                        let started = *scope_started_at.lock().unwrap();
                        let ref_rpm = *recording_ref_rpm.lock().unwrap();
                        let max_cyl = *max_cylinder_seen.lock().unwrap();
                        apply_recording_timing(&mut snap, started, ref_rpm, max_cyl);
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
                    did_work = true;
                    continue;
                }
                Some(Ok(_)) => {
                    thread::sleep(POLL_NOT_READY);
                    did_work = true;
                    continue;
                }
                Some(Err(e)) if is_buffer_not_ready(&e) => {
                    thread::sleep(POLL_NOT_READY);
                    did_work = true;
                    continue;
                }
                Some(Err(e)) => {
                    last_error = Some(e);
                }
                None => {
                    thread::yield_now();
                    thread::sleep(POLL_SERIAL_BUSY);
                    did_work = true;
                    continue;
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
                POLL_NOT_READY
            } else {
                POLL_IDLE
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_bytes(values: &[u16]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    }

    #[test]
    fn parse_v1_frame_header_and_payload() {
        let mut bytes = vec![1, 3, 1, 0, 2, 0, 0, 0];
        bytes.extend(sample_bytes(&[100, 200]));
        let frame = parse_knock_scope_frame(&bytes).expect("frame");
        assert_eq!(frame.header.cylinder_number, 3);
        assert_eq!(frame.header.channel_number, 1);
        assert_eq!(frame.samples.len(), 2);
        assert_eq!(frame.samples[0], 100.0);
        assert_eq!(frame.samples[1], 200.0);
    }

    #[test]
    fn legacy_buffer_without_header_still_decodes() {
        let bytes = sample_bytes(&[512, 1024]);
        let frame = decode_knock_scope_buffer(&bytes);
        assert_eq!(frame.header.version, 0);
        assert_eq!(frame.samples.len(), 2);
    }
}
