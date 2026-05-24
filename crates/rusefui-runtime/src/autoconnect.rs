use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rusefi_protocol::{
    is_port_busy, list_serial_ports, port_exists, rusefi_port_candidates,
    rusefi_usb_fingerprints, SerialPortEntry,
};
use serde::Serialize;

use crate::session::EcuSession;

/// Результат тика: не путать «порт занят» с подключением ECU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoConnectTick {
    Idle,
    /// Только scanning / busy_ports — без `ecu-connection`.
    ScanUi,
    /// Подключение или отключение ECU.
    Ecu { sync_ecu: bool },
}

const POLL_INTERVAL: Duration = Duration::from_millis(2000);
const DEFAULT_BAUD_RATE: u32 = 115_200;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoConnectSnapshot {
    pub offline_mode: bool,
    pub scanning: bool,
    pub auto_connect_enabled: bool,
    pub last_error: Option<String>,
    pub candidate_ports: Vec<String>,
    /// Порты, которые видны, но заняты другим процессом (например TunerStudio).
    pub busy_ports: Vec<String>,
}

pub struct AutoConnectManager {
    session: Arc<EcuSession>,
    offline_mode: AtomicBool,
    scanning: AtomicBool,
    last_error: Mutex<Option<String>>,
    suppress_port: Mutex<Option<String>>,
    last_usb_fingerprints: Mutex<Vec<String>>,
    last_scan_log: Mutex<Option<String>>,
    busy_ports: Mutex<Vec<String>>,
    logged_busy_ports: Mutex<HashSet<String>>,
    cached_candidate_ports: Mutex<Vec<String>>,
    running: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl AutoConnectManager {
    pub fn new(session: Arc<EcuSession>) -> Arc<Self> {
        Arc::new(Self {
            session,
            offline_mode: AtomicBool::new(false),
            scanning: AtomicBool::new(false),
            last_error: Mutex::new(None),
            suppress_port: Mutex::new(None),
            last_usb_fingerprints: Mutex::new(Vec::new()),
            last_scan_log: Mutex::new(None),
            busy_ports: Mutex::new(Vec::new()),
            logged_busy_ports: Mutex::new(HashSet::new()),
            cached_candidate_ports: Mutex::new(Vec::new()),
            running: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
        })
    }

    pub fn snapshot(&self) -> AutoConnectSnapshot {
        AutoConnectSnapshot {
            offline_mode: self.offline_mode.load(Ordering::Relaxed),
            scanning: self.scanning.load(Ordering::Relaxed),
            auto_connect_enabled: !self.offline_mode.load(Ordering::Relaxed),
            last_error: self.last_error.lock().unwrap().clone(),
            candidate_ports: self.cached_candidate_ports.lock().unwrap().clone(),
            busy_ports: self.busy_ports.lock().unwrap().clone(),
        }
    }

    fn refresh_cached_candidates(&self, entries: &[SerialPortEntry]) {
        *self.cached_candidate_ports.lock().unwrap() = rusefi_port_candidates(entries);
    }

    fn log_port_busy_once(&self, port: &str, detail: &str) {
        let mut logged = self.logged_busy_ports.lock().unwrap();
        if logged.insert(port.to_string()) {
            self.session.protocol_log().log_port_busy(port, detail);
        }
    }

    fn clear_port_busy_log(&self, port: &str) {
        self.logged_busy_ports.lock().unwrap().remove(port);
    }

    pub fn set_offline_mode(&self, offline: bool) {
        let was_offline = self.offline_mode.swap(offline, Ordering::Relaxed);
        let log = self.session.protocol_log();
        if offline && !was_offline {
            log.log_link("Offline mode: автоподключение отключено");
            if self.session.is_connected() {
                self.session
                    .disconnect_reason("offline mode включён", false);
            }
        } else if !offline && was_offline {
            log.log_link("Offline mode: автопоиск ECU включён");
            *self.last_usb_fingerprints.lock().unwrap() = Vec::new();
            *self.last_scan_log.lock().unwrap() = None;
        }
        if offline {
            *self.last_error.lock().unwrap() = None;
        }
    }

    pub fn is_offline_mode(&self) -> bool {
        self.offline_mode.load(Ordering::Relaxed)
    }

    pub fn note_manual_disconnect(&self, port_name: Option<&str>) {
        if let Some(port) = port_name {
            self.session.protocol_log().log_link(format!(
                "Ручное отключение: {port} (автопереподключение после физического отключения USB)"
            ));
        }
        *self.suppress_port.lock().unwrap() = port_name.map(str::to_string);
    }

    pub fn clear_manual_disconnect(&self) {
        *self.suppress_port.lock().unwrap() = None;
    }

    pub fn start(
        self: &Arc<Self>,
        on_ecu_change: impl Fn(bool) + Send + Sync + 'static,
        on_scan_ui: impl Fn() + Send + Sync + 'static,
    ) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }

        self.session.protocol_log().log_link(format!(
            "Автопоиск ECU запущен (интервал {} мс, baud {})",
            POLL_INTERVAL.as_millis(),
            DEFAULT_BAUD_RATE
        ));

        let manager = Arc::clone(self);
        let on_ecu_change = Arc::new(on_ecu_change);
        let on_scan_ui = Arc::new(on_scan_ui);
        let handle = thread::Builder::new()
            .name("rusefui-autoconnect".into())
            .spawn(move || poll_loop(manager, on_ecu_change, on_scan_ui))
            .expect("spawn autoconnect thread");

        *self.thread.lock().unwrap() = Some(handle);
    }

    fn log_usb_changes(&self, entries: &[SerialPortEntry]) {
        let fingerprints = rusefi_usb_fingerprints(entries);
        let mut last = self.last_usb_fingerprints.lock().unwrap();
        if fingerprints == *last {
            return;
        }

        let log = self.session.protocol_log();
        if fingerprints.is_empty() {
            if !last.is_empty() {
                log.log_link("USB rusEFI: устройства не обнаружены");
            }
        } else {
            for entry in entries.iter().filter(|e| e.is_rusefi_candidate) {
                log.log_usb_detected(entry);
            }
        }
        *last = fingerprints;
    }

    fn log_scan_once(&self, message: String) {
        let mut last = self.last_scan_log.lock().unwrap();
        if last.as_deref() == Some(message.as_str()) {
            return;
        }
        self.session.protocol_log().log_link(&message);
        *last = Some(message);
    }

    fn tick(&self) -> AutoConnectTick {
        if self.offline_mode.load(Ordering::Relaxed) {
            self.scanning.store(false, Ordering::Relaxed);
            return AutoConnectTick::Idle;
        }

        if self.session.is_ecu_busy() {
            self.scanning.store(false, Ordering::Relaxed);
            return AutoConnectTick::Idle;
        }

        if self.session.is_connected() {
            self.scanning.store(false, Ordering::Relaxed);
            *self.last_scan_log.lock().unwrap() = None;
            if !self.busy_ports.lock().unwrap().is_empty() {
                *self.busy_ports.lock().unwrap() = Vec::new();
                self.logged_busy_ports.lock().unwrap().clear();
            }
            if let Some(info) = self.session.connection_info_if_available() {
                if !port_exists(&info.port_name) {
                    self.session.disconnect_reason("USB порт пропал", true);
                    *self.suppress_port.lock().unwrap() = None;
                    return AutoConnectTick::Ecu { sync_ecu: true };
                }
            }
            return AutoConnectTick::Idle;
        }

        self.scanning.store(true, Ordering::Relaxed);

        let entries = match list_serial_ports() {
            Ok(entries) => entries,
            Err(e) => {
                let msg = e.to_string();
                *self.last_error.lock().unwrap() = Some(msg.clone());
                self.session
                    .protocol_log()
                    .log_link(format!("Ошибка перечисления портов: {msg}"));
                self.scanning.store(false, Ordering::Relaxed);
                return AutoConnectTick::Idle;
            }
        };

        self.log_usb_changes(&entries);
        self.refresh_cached_candidates(&entries);

        let suppress = self.suppress_port.lock().unwrap().clone();
        if let Some(ref suppressed) = suppress {
            if !port_exists(suppressed) {
                self.session.protocol_log().log_link(format!(
                    "USB {suppressed} отключён — снят запрет автопереподключения"
                ));
                *self.suppress_port.lock().unwrap() = None;
            }
        }

        let usb_only: Vec<SerialPortEntry> = entries
            .iter()
            .filter(|e| e.is_rusefi_candidate)
            .cloned()
            .collect();
        let candidates: Vec<String> = rusefi_port_candidates(&entries)
            .into_iter()
            .filter(|port| suppress.as_deref() != Some(port.as_str()))
            .collect();

        if candidates.is_empty() {
            self.log_scan_once("Сканирование: последовательных портов нет".into());
            self.scanning.store(false, Ordering::Relaxed);
            return AutoConnectTick::Idle;
        }

        if usb_only.is_empty() {
            self.log_scan_once(format!(
                "Сканирование: rusEFI USB не найден по VID/PID/имени, проверяем {} порт(ов): {}",
                candidates.len(),
                candidates.join(", ")
            ));
        } else {
            self.log_scan_once(format!(
                "Сканирование: {} rusEFI USB — {}",
                usb_only.len(),
                candidates.join(", ")
            ));
        }

        let log = self.session.protocol_log();
        let prev_busy = self.busy_ports.lock().unwrap().clone();
        let mut busy_now: Vec<String> = Vec::new();
        let mut tried = 0u32;

        for port in candidates {
            if self.session.is_io_locked() {
                break;
            }
            if is_port_busy(&port, DEFAULT_BAUD_RATE) {
                busy_now.push(port.clone());
                self.log_port_busy_once(&port, "не удалось открыть — порт занят");
                *self.last_error.lock().unwrap() = Some(format!(
                    "Порт {port} занят (возможно TunerStudio)"
                ));
                continue;
            }

            tried += 1;
            match self.session.connect_automatic(&port, DEFAULT_BAUD_RATE) {
                Ok(_info) => {
                    self.clear_port_busy_log(&port);
                    *self.busy_ports.lock().unwrap() = Vec::new();
                    *self.last_error.lock().unwrap() = None;
                    self.scanning.store(false, Ordering::Relaxed);
                    *self.last_scan_log.lock().unwrap() = None;
                    return AutoConnectTick::Ecu { sync_ecu: true };
                }
                Err(e) => {
                    let msg = e.clone();
                    *self.last_error.lock().unwrap() = Some(msg.clone());
                    if msg.contains("занят") {
                        busy_now.push(port.clone());
                        self.log_port_busy_once(&port, &msg);
                    } else {
                        log.log_link(format!("Подключение к {port} не удалось: {msg}"));
                    }
                }
            }
        }

        *self.busy_ports.lock().unwrap() = busy_now.clone();
        let busy_changed = prev_busy != busy_now;

        if tried > 0 {
            self.log_scan_once(format!(
                "Сканирование: ECU не подключена (попытка на {tried} свободном порту)"
            ));
        } else if !self.busy_ports.lock().unwrap().is_empty() {
            self.log_scan_once(
                "Сканирование: rusEFI USB найден, но порт занят другим приложением".into(),
            );
        }

        self.scanning.store(false, Ordering::Relaxed);
        if busy_changed {
            AutoConnectTick::ScanUi
        } else {
            AutoConnectTick::Idle
        }
    }
}

fn poll_loop(
    manager: Arc<AutoConnectManager>,
    on_ecu_change: Arc<dyn Fn(bool) + Send + Sync>,
    on_scan_ui: Arc<dyn Fn() + Send + Sync>,
) {
    while manager.running.load(Ordering::Relaxed) {
        match manager.tick() {
            AutoConnectTick::Idle => {}
            AutoConnectTick::ScanUi => on_scan_ui(),
            AutoConnectTick::Ecu { sync_ecu } => on_ecu_change(sync_ecu),
        }
        thread::sleep(POLL_INTERVAL);
    }
}

impl Drop for AutoConnectManager {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}
