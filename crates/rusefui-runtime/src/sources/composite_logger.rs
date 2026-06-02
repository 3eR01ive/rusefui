//! High-speed trigger (composite) logger — `l`+`3` read, склейка сессии как у лог. анализатора.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rusefi_protocol::{parse_composite_records, CompositeParseState, CompositeRecord};
use serde::{Deserialize, Serialize};

use super::composite_data_log::CompositeDataLogWriter;
use crate::session::EcuSession;

/// Пока буфер на ECU не готов — короткая пауза и снова read.
const POLL_WAIT_READY: Duration = Duration::from_millis(2);
/// После успешного read — сразу снова (следующий кусок).
const POLL_AFTER_CHUNK: Duration = Duration::from_millis(1);
/// Пауза, если опрос временно запрещён (config load и т.д.).
const POLL_IDLE: Duration = Duration::from_millis(40);
const STATUS_EMIT_INTERVAL: Duration = Duration::from_secs(1);
/// Ожидание serial (knock scope с `with_link_wait` иначе забирает порт).
const SERIAL_MUTEX_WAIT: Duration = Duration::from_millis(200);

/// Вся сессия записи (до «Стоп») — без обрезки по windowMs.
const RING_CAP: usize = 8_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositeEventJson {
    pub t_us: u64,
    pub pri: bool,
    pub sec: bool,
    pub trg: bool,
    pub sync: bool,
    pub coil: bool,
    pub inj: bool,
    /// Номер цикла TDC с начала сессии логгера (фронт `trg`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tdc_cycle: Option<u64>,
}

impl From<CompositeRecord> for CompositeEventJson {
    fn from(r: CompositeRecord) -> Self {
        Self {
            t_us: r.time_us,
            pri: r.pri_level,
            sec: r.sec_level,
            // Канал «TDC» на графике — бит `tdc` из ECU, не «trigger» камеры.
            trg: r.tdc,
            sync: r.sync,
            coil: r.coil,
            inj: r.injector,
            tdc_cycle: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositeSnapshot {
    pub connected: bool,
    pub logging_enabled: bool,
    pub polling: bool,
    pub events: Vec<CompositeEventJson>,
    pub total_events: u64,
    pub last_batch: usize,
    /// Длительность склеенной сессии в ring (мс).
    pub recorded_span_ms: f64,
    /// Разрыв по времени до предыдущего куска (мс), 0 если первый или стык.
    pub last_chunk_gap_ms: f64,
    pub chunks_received: u64,
    /// Всего TDC (фронтов `trg`) с начала сессии.
    pub tdc_cycles_total: u64,
    pub last_error: Option<String>,
    pub rpm: Option<f64>,
}

impl CompositeSnapshot {
    pub fn disconnected() -> Self {
        Self {
            connected: false,
            logging_enabled: false,
            polling: false,
            events: Vec::new(),
            total_events: 0,
            last_batch: 0,
            recorded_span_ms: 0.0,
            last_chunk_gap_ms: 0.0,
            chunks_received: 0,
            tdc_cycles_total: 0,
            last_error: None,
            rpm: None,
        }
    }
}

pub struct CompositeLoggerSource {
    snapshot: Arc<RwLock<CompositeSnapshot>>,
    ring: Arc<Mutex<VecDeque<CompositeEventJson>>>,
    parse_state: Arc<Mutex<CompositeParseState>>,
    running: Arc<AtomicBool>,
    logger_enabled_on_ecu: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn recorded_span_ms(ring: &VecDeque<CompositeEventJson>) -> f64 {
    if ring.len() < 2 {
        return 0.0;
    }
    let a = ring.front().unwrap().t_us;
    let b = ring.back().unwrap().t_us;
    (b.saturating_sub(a)) as f64 / 1000.0
}

fn trim_ring_cap(ring: &mut VecDeque<CompositeEventJson>) {
    while ring.len() > RING_CAP {
        ring.pop_front();
    }
}

fn ring_to_vec(ring: &VecDeque<CompositeEventJson>) -> Vec<CompositeEventJson> {
    ring.iter().cloned().collect()
}

fn build_snapshot(
    ring: &VecDeque<CompositeEventJson>,
    logging_enabled: bool,
    polling: bool,
    connected: bool,
    rpm: Option<f64>,
    last_error: Option<String>,
    last_batch: usize,
    last_chunk_gap_ms: f64,
    chunks_received: u64,
    tdc_cycles_total: u64,
    total_events: u64,
) -> CompositeSnapshot {
    CompositeSnapshot {
        connected,
        logging_enabled,
        polling,
        events: ring_to_vec(ring),
        total_events,
        last_batch,
        recorded_span_ms: recorded_span_ms(ring),
        last_chunk_gap_ms,
        chunks_received,
        tdc_cycles_total,
        last_error,
        rpm,
    }
}

fn append_chunk(
    ring: &mut VecDeque<CompositeEventJson>,
    batch: &[CompositeEventJson],
    next_tdc_cycle: &AtomicU64,
) -> f64 {
    if batch.is_empty() {
        return 0.0;
    }
    let gap_ms = ring.back().map(|last| {
        let dt = batch[0].t_us.saturating_sub(last.t_us);
        if dt == 0 { 0.0 } else { dt as f64 / 1000.0 }
    }).unwrap_or(0.0);

    let mut last_t = ring.back().map(|e| e.t_us);
    let mut prev_trg = ring.back().map(|e| e.trg);
    for rec in batch {
        let mut ev = rec.clone();
        if last_t.is_some_and(|t| ev.t_us < t) {
            ev.t_us = last_t.unwrap() + 1;
        } else if last_t.is_some_and(|t| ev.t_us == t) {
            continue;
        }

        let trg_rise = ev.trg && !prev_trg.unwrap_or(false);
        if trg_rise {
            let n = next_tdc_cycle.fetch_add(1, Ordering::Relaxed) + 1;
            ev.tdc_cycle = Some(n);
        }

        ring.push_back(ev);
        last_t = Some(ring.back().unwrap().t_us);
        prev_trg = Some(ring.back().unwrap().trg);
    }
    gap_ms
}

impl CompositeLoggerSource {
    pub fn new() -> Self {
        Self {
            snapshot: Arc::new(RwLock::new(CompositeSnapshot::disconnected())),
            ring: Arc::new(Mutex::new(VecDeque::new())),
            parse_state: Arc::new(Mutex::new(CompositeParseState::default())),
            running: Arc::new(AtomicBool::new(false)),
            logger_enabled_on_ecu: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
        }
    }

    /// Оставлено для совместимости; на ring не влияет (окно только во Vue).
    pub fn set_max_window_ms(&self, _max_window_ms: f64) {}

    pub fn snapshot(&self) -> CompositeSnapshot {
        self.snapshot.read().unwrap().clone()
    }

    pub fn is_polling(&self) -> bool {
        self.running.load(Ordering::SeqCst) && self.thread.lock().unwrap().is_some()
    }

    fn clear_session(&self) {
        self.ring.lock().unwrap().clear();
        *self.parse_state.lock().unwrap() = CompositeParseState::default();
    }

    /// Остановить опрос; склеенная сессия остаётся в snapshot.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        let _ = self.thread.lock().unwrap().take();

        let snap = {
            let ring = self.ring.lock().unwrap();
            let prev = self.snapshot.read().unwrap();
            build_snapshot(
                &ring,
                false,
                false,
                prev.connected,
                prev.rpm,
                prev.last_error.clone(),
                prev.last_batch,
                prev.last_chunk_gap_ms,
                prev.chunks_received,
                prev.tdc_cycles_total,
                prev.total_events,
            )
        };
        *self.snapshot.write().unwrap() = snap;
        self.logger_enabled_on_ecu.store(false, Ordering::SeqCst);
    }

    pub fn disable_on_ecu(&self, session: &EcuSession) {
        if !self.logger_enabled_on_ecu.load(Ordering::SeqCst) {
            return;
        }
        let _ = session.try_with_link(|link| link.set_composite_logger_enabled(false));
        self.logger_enabled_on_ecu.store(false, Ordering::SeqCst);
    }

    pub fn start<F>(
        &self,
        session: Arc<EcuSession>,
        log_writer: Option<Arc<Mutex<CompositeDataLogWriter>>>,
        on_tick: F,
    ) -> Result<(), String>
    where
        F: Fn(CompositeSnapshot) + Send + Sync + 'static,
    {
        if self.is_polling() {
            return Ok(());
        }

        session.knock_scope().disable_on_ecu(&session);
        session.knock_scope().stop();

        self.running.store(false, Ordering::SeqCst);
        let _ = self.thread.lock().unwrap().take();
        self.clear_session();

        session.with_link(|link| link.set_composite_logger_enabled(true))?;
        self.logger_enabled_on_ecu.store(true, Ordering::SeqCst);
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.logging_enabled = true;
            snap.connected = session.is_connected();
            snap.polling = true;
            snap.events.clear();
            snap.total_events = 0;
            snap.last_batch = 0;
            snap.recorded_span_ms = 0.0;
            snap.last_chunk_gap_ms = 0.0;
            snap.chunks_received = 0;
            snap.tdc_cycles_total = 0;
            snap.last_error = None;
        }
        self.running.store(true, Ordering::SeqCst);

        let running = Arc::clone(&self.running);
        let snapshot = Arc::clone(&self.snapshot);
        let ring = Arc::clone(&self.ring);
        let parse_state = Arc::clone(&self.parse_state);
        let next_tdc_cycle = Arc::new(AtomicU64::new(0));
        let on_tick = Arc::new(on_tick);

        let handle = thread::Builder::new()
            .name("rusefui-composite-poll".into())
            .spawn(move || {
                poll_loop(
                    session,
                    running,
                    snapshot,
                    ring,
                    parse_state,
                    log_writer,
                    next_tdc_cycle,
                    on_tick,
                );
            })
            .expect("spawn composite poll thread");

        *self.thread.lock().unwrap() = Some(handle);
        Ok(())
    }
}

fn poll_loop(
    session: Arc<EcuSession>,
    running: Arc<AtomicBool>,
    snapshot: Arc<RwLock<CompositeSnapshot>>,
    ring: Arc<Mutex<VecDeque<CompositeEventJson>>>,
    parse_state: Arc<Mutex<CompositeParseState>>,
    log_writer: Option<Arc<Mutex<CompositeDataLogWriter>>>,
    next_tdc_cycle: Arc<AtomicU64>,
    on_tick: Arc<dyn Fn(CompositeSnapshot) + Send + Sync>,
) {
    let mut last_rpm_check = std::time::Instant::now();
    let mut last_status_emit = std::time::Instant::now();
    let chunks_received = Arc::new(AtomicU64::new(0));
    const RPM_CHECK_INTERVAL: Duration = Duration::from_millis(250);

    let emit_to_ui = |connected: bool,
                      allow_poll: bool,
                      rpm: Option<f64>,
                      last_error: &Option<String>,
                      last_batch: usize| {
        let snap = {
            let ring_guard = ring.lock().unwrap();
            let snap_ro = snapshot.read().unwrap();
            build_snapshot(
                &ring_guard,
                true,
                allow_poll,
                connected,
                rpm,
                last_error.clone(),
                last_batch,
                snap_ro.last_chunk_gap_ms,
                chunks_received.load(Ordering::Relaxed),
                next_tdc_cycle.load(Ordering::Relaxed),
                snap_ro.total_events,
            )
        };
        *snapshot.write().unwrap() = snap.clone();
        on_tick(snap);
    };

    while running.load(Ordering::SeqCst) {
        let connected = session.is_connected();
        let mut last_error: Option<String> = None;
        let mut last_batch = 0usize;

        let allow_poll = connected && !session.config().snapshot().loading;

        let rpm = if last_rpm_check.elapsed() >= RPM_CHECK_INTERVAL {
            last_rpm_check = std::time::Instant::now();
            session
                .output()
                .snapshot()
                .values
                .get("RPMValue")
                .copied()
        } else {
            snapshot.read().unwrap().rpm
        };

        if allow_poll {
            match session.with_link_wait(SERIAL_MUTEX_WAIT, |link| link.read_composite_buffer())
            {
                Ok(payload) if !payload.is_empty() => {
                    let parsed = {
                        let mut st = parse_state.lock().unwrap();
                        parse_composite_records(&payload, &mut st)
                    };
                    if !parsed.is_empty() {
                        let batch: Vec<CompositeEventJson> =
                            parsed.into_iter().map(Into::into).collect();
                        last_batch = batch.len();
                        let mut r = ring.lock().unwrap();
                        let gap_ms = append_chunk(&mut r, &batch, &next_tdc_cycle);
                        trim_ring_cap(&mut r);
                        drop(r);

                        if let Some(log) = &log_writer {
                            if let Ok(mut w) = log.lock() {
                                w.write_events(&batch);
                            }
                        }

                        chunks_received.fetch_add(1, Ordering::Relaxed);
                        {
                            let mut snap = snapshot.write().unwrap();
                            snap.total_events =
                                snap.total_events.saturating_add(last_batch as u64);
                            snap.last_chunk_gap_ms = gap_ms;
                        }
                        emit_to_ui(connected, allow_poll, rpm, &last_error, last_batch);
                        thread::sleep(POLL_AFTER_CHUNK);
                        continue;
                    }
                }
                Ok(_) => {}
                Err(e) if e.contains("0x84") => {
                    thread::sleep(POLL_WAIT_READY);
                    continue;
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }

        if last_error.is_some() || last_status_emit.elapsed() >= STATUS_EMIT_INTERVAL {
            last_status_emit = std::time::Instant::now();
            emit_to_ui(connected, allow_poll, rpm, &last_error, last_batch);
        }

        thread::sleep(if allow_poll {
            POLL_WAIT_READY
        } else {
            POLL_IDLE
        });
    }
}
