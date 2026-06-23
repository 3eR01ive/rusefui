//! Engine sniffer (логический анализатор) — разбор `wave_chart` из `G`-потока.
//!
//! В отличие от composite logger:
//! - данные приходят в общем `G`-буфере (не выделенным `l`+`3` чтением);
//! - sniffer включается прошивкой сам при `rpm < engineSnifferRpmThreshold`,
//!   команды enable на ECU нет — «enabled» здесь чисто клиентский (poll on/off);
//! - кадр самодостаточен (время с нуля) → snapshot заменяется целиком, не кольцо.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rusefi_protocol::{parse_wave_chart, SnifferEvent, WaveChartParseState};
use serde::Serialize;

use crate::session::EcuSession;

/// Ожидание serial mutex (knock/composite могут держать порт).
const SERIAL_MUTEX_WAIT: Duration = Duration::from_millis(200);
/// После полученного кадра — почти сразу следующий read.
const POLL_AFTER_FRAME: Duration = Duration::from_millis(5);
/// Между read без нового кадра.
const POLL_WAIT: Duration = Duration::from_millis(20);
/// Пауза, когда опрос временно запрещён (нет связи / config load).
const POLL_IDLE: Duration = Duration::from_millis(60);
/// Минимальный интервал статус-эмита без кадров.
const STATUS_EMIT_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnifferEventJson {
    pub t_us: u64,
    pub name: String,
    pub up: bool,
    pub tdc: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpm: Option<u32>,
}

impl From<&SnifferEvent> for SnifferEventJson {
    fn from(e: &SnifferEvent) -> Self {
        Self {
            t_us: e.time_us,
            name: e.name.clone(),
            up: e.up,
            tdc: e.tdc,
            rpm: e.rpm,
        }
    }
}

/// Группа канала для визуальной группировки (`trigger`/`ignition`/`injector`/`other`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnifferChannel {
    pub name: String,
    pub group: String,
}

/// Классифицировать канал по короткому имени из engine sniffer протокола rusEFI.
/// Возвращает (группа, ранг сортировки). Имена: `t1`/`t2`, `VVT*`, `input*` —
/// триггеры; `c*`/`r*` — катушки; `i*`/`j*` — форсунки.
fn classify_channel(name: &str) -> (&'static str, u8) {
    let lower = name.to_ascii_lowercase();
    let b = name.as_bytes();
    let first = b.first().copied().unwrap_or(0);
    let has_suffix = b.len() >= 2;

    if name == "t1"
        || name == "t2"
        || lower.starts_with("vvt")
        || lower.starts_with("input")
        || lower.starts_with("cam")
        || lower.starts_with("trg")
    {
        ("trigger", 0)
    } else if has_suffix && (first == b'c' || first == b'r') {
        ("ignition", 1)
    } else if has_suffix && (first == b'i' || first == b'j') {
        ("injector", 2)
    } else {
        ("other", 3)
    }
}

/// Ключ сортировки внутри группы: (префикс, числовой суффикс). Суффикс из цифр
/// (`i1`..`i12`) — десятичный; буквенный (trailing `rA`/`rB`/`rD`) — после `r9`.
fn channel_sort_key(name: &str) -> (u8, i64) {
    let prefix = name.as_bytes().first().copied().unwrap_or(0);
    let suffix = name.get(1..).unwrap_or("");
    let num = if let Ok(n) = suffix.parse::<i64>() {
        n
    } else if let Some(c) = suffix.chars().next() {
        if c.is_ascii_alphabetic() {
            10 + (c.to_ascii_uppercase() as i64 - 'A' as i64)
        } else {
            i64::MAX
        }
    } else {
        i64::MAX
    };
    (prefix, num)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSnifferSnapshot {
    pub connected: bool,
    pub polling: bool,
    /// Каналы, сгруппированные (триггеры → зажигание → форсунки → прочее);
    /// внутри группы — порядок первого появления.
    pub channels: Vec<SnifferChannel>,
    /// События последнего кадра.
    pub events: Vec<SnifferEventJson>,
    /// Длительность кадра (µs) — макс. время события.
    pub frame_span_us: u64,
    pub frames_received: u64,
    pub rpm: Option<f64>,
    pub last_error: Option<String>,
}

impl EngineSnifferSnapshot {
    pub fn disconnected() -> Self {
        Self {
            connected: false,
            polling: false,
            channels: Vec::new(),
            events: Vec::new(),
            frame_span_us: 0,
            frames_received: 0,
            rpm: None,
            last_error: None,
        }
    }
}

/// Собрать snapshot из событий одного кадра.
///
/// `order` — накопительный порядок каналов за сессию: новые имена дописываются в
/// конец и сохраняют свой индекс, чтобы lane'ы не переставлялись между кадрами.
fn build_frame_snapshot(
    events: &[SnifferEvent],
    connected: bool,
    frames_received: u64,
    fallback_rpm: Option<f64>,
    order: &mut Vec<String>,
) -> EngineSnifferSnapshot {
    let mut span = 0u64;
    let mut tdc_rpm: Option<u32> = None;

    for e in events {
        if e.time_us > span {
            span = e.time_us;
        }
        if e.tdc {
            if let Some(r) = e.rpm {
                tdc_rpm = Some(r);
            }
        } else if !order.iter().any(|c| c == &e.name) {
            order.push(e.name.clone());
        }
    }

    // Группировка по рангу; внутри зажигания/форсунок — по возрастанию номера
    // (натурально: i1<i2<…<i10, c1<…<c12, r1<…<r9<rA<rB<rD), у триггеров/прочих
    // сохраняется порядок первого появления (sort_by стабилен).
    let mut channels: Vec<SnifferChannel> = order
        .iter()
        .map(|name| {
            let (group, _) = classify_channel(name);
            SnifferChannel { name: name.clone(), group: group.to_string() }
        })
        .collect();
    channels.sort_by(|a, b| {
        let (ga, ra) = classify_channel(&a.name);
        let rb = classify_channel(&b.name).1;
        if ra != rb {
            return ra.cmp(&rb);
        }
        if ga == "ignition" || ga == "injector" {
            channel_sort_key(&a.name).cmp(&channel_sort_key(&b.name))
        } else {
            std::cmp::Ordering::Equal
        }
    });

    EngineSnifferSnapshot {
        connected,
        polling: true,
        channels,
        events: events.iter().map(SnifferEventJson::from).collect(),
        frame_span_us: span,
        frames_received,
        rpm: tdc_rpm.map(f64::from).or(fallback_rpm),
        last_error: None,
    }
}

type TickHook = Arc<dyn Fn(EngineSnifferSnapshot) + Send + Sync>;

pub struct EngineSnifferSource {
    snapshot: Arc<RwLock<EngineSnifferSnapshot>>,
    running: Arc<AtomicBool>,
    frames_received: Arc<AtomicU64>,
    thread: Mutex<Option<JoinHandle<()>>>,
    /// Хук эмита снапшота — чтобы `stop()` (в т.ч. вызванный другим логгером)
    /// обновил UI панели.
    tick_hook: Mutex<Option<TickHook>>,
}

impl Default for EngineSnifferSource {
    fn default() -> Self {
        Self::new()
    }
}

impl EngineSnifferSource {
    pub fn new() -> Self {
        Self {
            snapshot: Arc::new(RwLock::new(EngineSnifferSnapshot::disconnected())),
            running: Arc::new(AtomicBool::new(false)),
            frames_received: Arc::new(AtomicU64::new(0)),
            thread: Mutex::new(None),
            tick_hook: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> EngineSnifferSnapshot {
        self.snapshot.read().unwrap().clone()
    }

    pub fn is_polling(&self) -> bool {
        self.running.load(Ordering::SeqCst) && self.thread.lock().unwrap().is_some()
    }

    /// Остановить опрос; последний кадр остаётся в snapshot. Эмитит финальный
    /// снапшот (polling=false), чтобы UI обновился даже при остановке извне.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        let _ = self.thread.lock().unwrap().take();
        let snap = {
            let mut snap = self.snapshot.write().unwrap();
            snap.polling = false;
            snap.clone()
        };
        if let Some(hook) = self.tick_hook.lock().unwrap().clone() {
            hook(snap);
        }
    }

    pub fn start<F>(&self, session: Arc<EcuSession>, on_tick: F) -> Result<(), String>
    where
        F: Fn(EngineSnifferSnapshot) + Send + Sync + 'static,
    {
        if self.is_polling() {
            return Ok(());
        }

        // Composite/knock — высоковолюмные читатели порта; одновременный опрос
        // с ними ломает их (конкуренция за serial). Делаем взаимоисключающими.
        session.composite().disable_on_ecu(&session);
        session.composite().stop();
        session.knock_scope().disable_on_ecu(&session);
        session.knock_scope().stop();

        self.running.store(false, Ordering::SeqCst);
        let _ = self.thread.lock().unwrap().take();
        self.frames_received.store(0, Ordering::SeqCst);

        {
            let mut snap = self.snapshot.write().unwrap();
            *snap = EngineSnifferSnapshot::disconnected();
            snap.connected = session.is_connected();
            snap.polling = true;
        }
        self.running.store(true, Ordering::SeqCst);

        let running = Arc::clone(&self.running);
        let snapshot = Arc::clone(&self.snapshot);
        let frames_received = Arc::clone(&self.frames_received);
        let on_tick: TickHook = Arc::new(on_tick);
        *self.tick_hook.lock().unwrap() = Some(Arc::clone(&on_tick));

        let handle = thread::Builder::new()
            .name("rusefui-engine-sniffer-poll".into())
            .spawn(move || {
                poll_loop(session, running, snapshot, frames_received, on_tick);
            })
            .expect("spawn engine sniffer poll thread");

        *self.thread.lock().unwrap() = Some(handle);
        Ok(())
    }
}

fn poll_loop(
    session: Arc<EcuSession>,
    running: Arc<AtomicBool>,
    snapshot: Arc<RwLock<EngineSnifferSnapshot>>,
    frames_received: Arc<AtomicU64>,
    on_tick: Arc<dyn Fn(EngineSnifferSnapshot) + Send + Sync>,
) {
    let mut parse_state = WaveChartParseState::default();
    let mut last_status_emit = Instant::now();
    // Накопительный порядок каналов: имя получает индекс при первом появлении и
    // больше не меняется — иначе lane'ы прыгают между кадрами.
    let mut channel_order: Vec<String> = Vec::new();

    while running.load(Ordering::SeqCst) {
        let connected = session.is_connected();
        let allow_poll = connected && !session.config().snapshot().loading;
        let mut last_error: Option<String> = None;

        if allow_poll {
            match session.with_link_wait(SERIAL_MUTEX_WAIT, |link| link.get_console_raw()) {
                Ok(raw) => {
                    let events = parse_wave_chart(&raw, &mut parse_state);
                    if !events.is_empty() {
                        let fallback_rpm = session
                            .output()
                            .snapshot()
                            .values
                            .get("RPMValue")
                            .copied();
                        let n = frames_received.fetch_add(1, Ordering::Relaxed) + 1;
                        let snap = build_frame_snapshot(
                            &events,
                            connected,
                            n,
                            fallback_rpm,
                            &mut channel_order,
                        );
                        *snapshot.write().unwrap() = snap.clone();
                        on_tick(snap);
                        last_status_emit = Instant::now();
                        thread::sleep(POLL_AFTER_FRAME);
                        continue;
                    }
                }
                Err(e) => last_error = Some(e),
            }
        }

        if last_error.is_some() || last_status_emit.elapsed() >= STATUS_EMIT_INTERVAL {
            last_status_emit = Instant::now();
            let snap = {
                let mut s = snapshot.write().unwrap();
                s.connected = connected;
                s.polling = true;
                s.last_error = last_error.clone();
                s.clone()
            };
            on_tick(snap);
        }

        thread::sleep(if allow_poll { POLL_WAIT } else { POLL_IDLE });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(name: &str) -> SnifferEvent {
        SnifferEvent { name: name.into(), time_us: 0, up: true, tdc: false, rpm: None }
    }

    #[test]
    fn groups_and_sorts_channels() {
        // Намеренно вперемешку: i10 раньше i2, c3 раньше c1, форсунки раньше зажигания.
        let events = vec![
            ev("i10"), ev("c3"), ev("t2"), ev("i2"), ev("c1"),
            ev("t1"), ev("rA"), ev("r1"), ev("j1"), ev("i1"),
        ];
        let mut order = Vec::new();
        let snap = build_frame_snapshot(&events, true, 1, None, &mut order);
        let got: Vec<(&str, &str)> = snap
            .channels
            .iter()
            .map(|c| (c.name.as_str(), c.group.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                // триггеры — порядок первого появления
                ("t2", "trigger"),
                ("t1", "trigger"),
                // зажигание — c по возрастанию, затем trailing r
                ("c1", "ignition"),
                ("c3", "ignition"),
                ("r1", "ignition"),
                ("rA", "ignition"),
                // форсунки — i по возрастанию, затем j
                ("i1", "injector"),
                ("i2", "injector"),
                ("i10", "injector"),
                ("j1", "injector"),
            ]
        );
    }
}
