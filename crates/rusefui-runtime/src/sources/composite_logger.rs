//! High-speed trigger (composite) logger — `TS_GET_COMPOSITE_BUFFER` (`8`) / `l`+`3`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rusefi_protocol::{parse_composite_records, CompositeParseState, CompositeRecord};
use serde::Serialize;

use crate::session::EcuSession;

const COMPOSITE_POLL_HZ: f64 = 25.0;
const POLL_INTERVAL: Duration =
    Duration::from_nanos((1_000_000_000.0 / COMPOSITE_POLL_HZ) as u64);
const EMIT_INTERVAL: Duration = Duration::from_millis(33);
/// Как Java `BinaryProtocolLogger.COMPOSITE_OFF_RPM`.
const COMPOSITE_OFF_RPM: f64 = 700.0;
const HIGH_RPM_HOLD_SEC: f64 = 10.0;
const RING_CAP: usize = 4_000;
/// Сколько последних событий отдаём в UI за один emit (IPC + canvas).
const EMIT_UI_EVENT_CAP: usize = 400;
/// Не шире типичного окна графика (300 ms) + запас под зум/хвост.
const EMIT_UI_TIME_WINDOW_US: u64 = 450_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositeEventJson {
    pub t_us: u64,
    pub pri: bool,
    pub sec: bool,
    pub trg: bool,
    pub sync: bool,
    pub coil: bool,
    pub inj: bool,
}

impl From<CompositeRecord> for CompositeEventJson {
    fn from(r: CompositeRecord) -> Self {
        Self {
            t_us: r.time_us,
            pri: r.pri_level,
            sec: r.sec_level,
            trg: r.trigger,
            sync: r.sync,
            coil: r.coil,
            inj: r.injector,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositeSnapshot {
    pub connected: bool,
    /// Пользователь включил опрос (кнопка или autostart).
    pub logging_enabled: bool,
    pub polling: bool,
    pub events: Vec<CompositeEventJson>,
    pub total_events: u64,
    pub last_batch: usize,
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

    pub fn snapshot(&self) -> CompositeSnapshot {
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
        self.ring.lock().unwrap().clear();
        *self.parse_state.lock().unwrap() = CompositeParseState::default();
        let mut snap = CompositeSnapshot::disconnected();
        snap.logging_enabled = false;
        *self.snapshot.write().unwrap() = snap;
        self.logger_enabled_on_ecu.store(false, Ordering::SeqCst);
    }

    /// Выключить логгер на ECU (best-effort).
    pub fn disable_on_ecu(&self, session: &EcuSession) {
        if !self.logger_enabled_on_ecu.load(Ordering::SeqCst) {
            return;
        }
        let _ = session.try_with_link(|link| link.set_composite_logger_enabled(false));
        self.logger_enabled_on_ecu.store(false, Ordering::SeqCst);
    }

    pub fn start<F>(&self, session: Arc<EcuSession>, on_tick: F) -> Result<(), String>
    where
        F: Fn(CompositeSnapshot) + Send + Sync + 'static,
    {
        if self.is_polling() {
            return Ok(());
        }
        self.stop();
        session.with_link(|link| link.set_composite_logger_enabled(true))?;
        self.logger_enabled_on_ecu.store(true, Ordering::SeqCst);
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.logging_enabled = true;
            snap.connected = session.is_connected();
        }
        self.running.store(true, Ordering::SeqCst);

        let running = Arc::clone(&self.running);
        let snapshot = Arc::clone(&self.snapshot);
        let ring = Arc::clone(&self.ring);
        let parse_state = Arc::clone(&self.parse_state);
        let logger_enabled = Arc::clone(&self.logger_enabled_on_ecu);
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
                    logger_enabled,
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
    logger_enabled_on_ecu: Arc<AtomicBool>,
    on_tick: Arc<dyn Fn(CompositeSnapshot) + Send + Sync>,
) {
    let last_emit_ms = AtomicU64::new(0);
    let mut high_rpm_since: Option<std::time::Instant> = None;
    let mut events_dirty = false;
    let mut last_rpm_check = std::time::Instant::now();
    const RPM_CHECK_INTERVAL: Duration = Duration::from_millis(250);

    while running.load(Ordering::SeqCst) {
        let connected = session.is_connected();
        let mut last_error: Option<String> = None;

        let mut allow_poll = connected && !session.config().snapshot().loading;

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

        if let Some(r) = rpm {
            if r <= COMPOSITE_OFF_RPM {
                high_rpm_since = None;
            } else if high_rpm_since.is_none() {
                high_rpm_since = Some(std::time::Instant::now());
            }
            if let Some(since) = high_rpm_since {
                if since.elapsed().as_secs_f64() >= HIGH_RPM_HOLD_SEC {
                    allow_poll = false;
                    if logger_enabled_on_ecu.load(Ordering::SeqCst) {
                        let _ = session.try_with_link(|link| {
                            link.set_composite_logger_enabled(false)
                        });
                        logger_enabled_on_ecu.store(false, Ordering::SeqCst);
                    }
                }
            }
        }

        if allow_poll {
            if let Some(result) = session.try_with_link(|link| link.read_composite_buffer()) {
                match result {
                    Ok(payload) if !payload.is_empty() => {
                        let parsed = {
                            let mut st = parse_state.lock().unwrap();
                            parse_composite_records(&payload, &mut st)
                        };
                        if !parsed.is_empty() {
                            let batch = parsed.len();
                            let mut r = ring.lock().unwrap();
                            for rec in parsed {
                                if r.len() >= RING_CAP {
                                    r.pop_front();
                                }
                                r.push_back(rec.into());
                            }
                            drop(r);
                            let mut snap = snapshot.write().unwrap();
                            snap.last_batch = batch;
                            snap.total_events =
                                snap.total_events.saturating_add(batch as u64);
                            events_dirty = true;
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        if !e.contains("0x84") {
                            last_error = Some(e);
                            events_dirty = true;
                        }
                    }
                }
            }
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let prev = last_emit_ms.load(Ordering::Relaxed);
        if now.saturating_sub(prev) >= EMIT_INTERVAL.as_millis() as u64 {
            last_emit_ms.store(now, Ordering::Relaxed);

            let status_changed = last_error.is_some()
                || connected != snapshot.read().unwrap().connected;

            if events_dirty || status_changed {
                let events: Vec<CompositeEventJson> = {
                let ring_guard = ring.lock().unwrap();
                if ring_guard.is_empty() {
                    Vec::new()
                } else {
                    let t_end = ring_guard.back().unwrap().t_us;
                    let t_min = t_end.saturating_sub(EMIT_UI_TIME_WINDOW_US);
                    let mut out: Vec<CompositeEventJson> = ring_guard
                        .iter()
                        .filter(|e| e.t_us >= t_min)
                        .cloned()
                        .collect();
                    if out.len() > EMIT_UI_EVENT_CAP {
                        let drop = out.len() - EMIT_UI_EVENT_CAP;
                        out.drain(0..drop);
                    }
                    out
                }
                };

                let snap = {
                    let mut snap = snapshot.write().unwrap();
                    snap.connected = connected;
                    snap.logging_enabled = true;
                    snap.polling = allow_poll;
                    snap.events = events;
                    snap.rpm = rpm;
                    match &last_error {
                        Some(err) => snap.last_error = Some(err.clone()),
                        None if snap.last_error.is_some() => snap.last_error = None,
                        None => {}
                    }
                    snap.clone()
                };
                events_dirty = false;
                on_tick(snap);
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
}
