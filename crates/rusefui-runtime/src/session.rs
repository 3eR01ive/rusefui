use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rusefi_ini::{encode_config_value, ConfigFieldKind};
use rusefi_protocol::{ConnectionInfo, ProtocolError, SerialLink, DEFAULT_IO_TIMEOUT_MS};

use crate::ini::{find_any_local_ini, load_ini_path, resolve_ini_for_signature, ResolvedIni};
use crate::protocol_log::ProtocolLogStore;
use crate::sources::composite_logger::CompositeLoggerSource;
use crate::sources::config::ConfigSource;
use crate::sources::output_channels::{IniContext, OutputChannelsSource};
use crate::sources::output_data_log::OutputDataLogWriter;
use crate::sources::output_timeline::{
    OutputTimeline, OutputTimelineStatus, OutputTimelineView, OutputTimelineViewControl,
    OutputTimelineViewQuery,
};

const STIMULATOR_CMD: &str = "self_stimulation";
const TRIGGER_RPM_FIELD: &str = "triggerSimulatorRpm";

struct EcuSessionInner {
    link: Option<SerialLink>,
}

/// Общая сессия ECU: serial link + фоновый опрос output channels.
pub struct EcuSession {
    inner: Mutex<EcuSessionInner>,
    ini: Mutex<IniContext>,
    loaded_ini_path: Mutex<Option<PathBuf>>,
    output: OutputChannelsSource,
    composite: CompositeLoggerSource,
    config: ConfigSource,
    protocol_log: Arc<ProtocolLogStore>,
    /// Пока true — не запускать poll `O` (конфликт с консольными `E` на том же порту).
    stimulation_active: AtomicBool,
    output_data_log: Mutex<Option<OutputDataLogWriter>>,
    output_timeline: Mutex<OutputTimeline>,
}

impl EcuSession {
    pub fn new_arc(protocol_log: Arc<ProtocolLogStore>) -> Arc<Self> {
        let ini_ctx = IniContext::disconnected();
        Arc::new(Self {
            inner: Mutex::new(EcuSessionInner { link: None }),
            ini: Mutex::new(ini_ctx.clone()),
            loaded_ini_path: Mutex::new(None),
            output: OutputChannelsSource::new(ini_ctx.clone()),
            composite: CompositeLoggerSource::new(),
            config: ConfigSource::new(ini_ctx),
            protocol_log,
            stimulation_active: AtomicBool::new(false),
            output_data_log: Mutex::new(None),
            output_timeline: Mutex::new(OutputTimeline::default()),
        })
    }

    pub fn output_timeline_live_sec(&self) -> f64 {
        self.output_timeline.lock().unwrap().live_sec()
    }

    pub fn output_timeline_status(&self) -> OutputTimelineStatus {
        self.output_timeline.lock().unwrap().status()
    }

    pub fn output_timeline_query(&self, query: OutputTimelineViewQuery) -> OutputTimelineView {
        self.output_timeline.lock().unwrap().query_view(&query)
    }

    pub fn output_timeline_control(
        &self,
        ctrl: OutputTimelineViewControl,
    ) -> OutputTimelineStatus {
        self.output_timeline.lock().unwrap().apply_view_control(ctrl)
    }

    pub fn output_timeline_load_file(&self, path: PathBuf) -> OutputTimelineStatus {
        let mut tl = self.output_timeline.lock().unwrap();
        tl.load_file(path);
        tl.status()
    }

    /// Пустой рабочий стол: config, timeline, лог сессии (без отключения ECU).
    pub fn reset_workspace_for_new_project(&self) {
        self.config().stop();
        self.composite().stop();
        let _ = self.stop_output_data_log();
        *self.output_timeline.lock().unwrap() = OutputTimeline::default();

        if self.is_connected() {
            let ini = self.ini_context();
            let field_names: Vec<String> = ini
                .channels
                .fields
                .iter()
                .map(|f| f.name.clone())
                .collect();
            if !field_names.is_empty() {
                let started_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let mut tl = self.output_timeline.lock().unwrap();
                tl.reset_session_with_start(&field_names, 30.0, started_ms);
                tl.set_connected(true);
            }
        }

        self.bootstrap_offline_ini_if_needed();
    }

    pub fn record_output_sample(&self, timestamp_ms: u64, values: &HashMap<String, f64>) {
        if let Ok(mut log_guard) = self.output_data_log.try_lock() {
            if let Some(log) = log_guard.as_mut() {
                log.write_sample(timestamp_ms, values);
            }
        }
        // Блокирующий lock: try_lock терял сэмплы, пока UI держит mutex на query_view.
        self.output_timeline
            .lock()
            .unwrap()
            .ingest_from_wall_ms(timestamp_ms, values);
    }

    fn start_output_data_log(
        &self,
        info: &ConnectionInfo,
        ini: &IniContext,
        ini_path: &PathBuf,
    ) {
        let field_names: Vec<String> = ini
            .channels
            .fields
            .iter()
            .map(|f| f.name.clone())
            .collect();
        let started_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        {
            let mut tl = self.output_timeline.lock().unwrap();
            tl.reset_session_with_start(&field_names, 30.0, started_ms);
            tl.set_connected(true);
        }

        match OutputDataLogWriter::open_at(info, ini, Some(ini_path), started_ms) {
            Ok(writer) => {
                let path = writer.path().display().to_string();
                self.protocol_log
                    .log_info(&format!("Output log (сессия): {path}"));
                *self.output_data_log.lock().unwrap() = Some(writer);
            }
            Err(e) => {
                self.protocol_log
                    .log_info(&format!("Output log не создан: {e}"));
            }
        }
    }

    fn stop_output_data_log(&self) -> Option<PathBuf> {
        let mut guard = match self.output_data_log.lock() {
            Ok(g) => g,
            Err(_) => return None,
        };
        let closed = if let Some(writer) = guard.take() {
            match writer.close() {
                Ok((path, rows)) => {
                    self.protocol_log.log_info(&format!(
                        "Output log закрыт: {} ({rows} строк)",
                        path.display()
                    ));
                    Some(path)
                }
                Err(e) => {
                    self.protocol_log
                        .log_info(&format!("Output log: ошибка закрытия: {e}"));
                    None
                }
            }
        } else {
            None
        };
        if let Some(path) = &closed {
            let mut tl = self.output_timeline.lock().unwrap();
            tl.set_session_file(Some(path.clone()));
            tl.set_connected(false);
        } else {
            self.output_timeline.lock().unwrap().set_connected(false);
        }
        closed
    }

    pub fn output_session_log_path(&self) -> Option<String> {
        self.output_timeline.lock().ok()?.session_log_path().or_else(|| {
            self.output_data_log
                .lock()
                .ok()
                .and_then(|g| g.as_ref().map(|w| w.path().display().to_string()))
        })
    }

    pub fn is_stimulation_active(&self) -> bool {
        self.stimulation_active.load(Ordering::Relaxed)
    }

    pub fn set_stimulation_active(&self, active: bool) {
        self.stimulation_active.store(active, Ordering::SeqCst);
    }

    /// Output poll `O` (во время стимуляции `O` и консольные `E` не мешают — см. Java console + TS).
    pub fn should_poll_output_channels(&self) -> bool {
        self.is_connected() && !self.config().snapshot().loading
    }

    pub fn protocol_log(&self) -> Arc<ProtocolLogStore> {
        Arc::clone(&self.protocol_log)
    }

    pub fn ini_context(&self) -> IniContext {
        self.ini.lock().unwrap().clone()
    }

    /// Применить INI без подключения к ECU (offline, настройка графиков).
    pub fn apply_ini(&self, resolved: ResolvedIni) {
        let ini_ctx = IniContext::from_ini(&resolved.file);
        *self.ini.lock().unwrap() = ini_ctx.clone();
        *self.loaded_ini_path.lock().unwrap() = Some(resolved.path.clone());
        self.output.replace_ini(ini_ctx.clone());
        self.config.replace_ini(ini_ctx);
        self.protocol_log.log_info(&format!(
            "INI загружен (offline): {}",
            resolved.path.display()
        ));
    }

    /// Если output channels ещё пусты — взять локальный INI (`RUSEFI_INI_PATH`, test_data, …).
    pub fn bootstrap_offline_ini_if_needed(&self) {
        if !self.ini_context().channels.fields.is_empty() {
            return;
        }
        if let Some(resolved) = find_any_local_ini() {
            self.apply_ini(resolved);
        }
    }

    pub fn load_ini_from_path(&self, path: &std::path::Path) -> Result<(), String> {
        let resolved = load_ini_path(path).map_err(|e| e.to_string())?;
        if resolved.file.output_channels.fields.is_empty() {
            return Err(format!(
                "В INI нет [OutputChannels]: {}",
                path.display()
            ));
        }
        self.apply_ini(resolved);
        Ok(())
    }

    pub fn loaded_ini_path(&self) -> Option<PathBuf> {
        self.loaded_ini_path.lock().unwrap().clone()
    }

    pub fn output(&self) -> &OutputChannelsSource {
        &self.output
    }

    pub fn composite(&self) -> &CompositeLoggerSource {
        &self.composite
    }

    pub fn config(&self) -> &ConfigSource {
        &self.config
    }

    pub fn is_connected(&self) -> bool {
        self.inner.lock().unwrap().link.is_some()
    }

    pub fn is_ecu_busy(&self) -> bool {
        self.is_io_locked() || self.config().snapshot().loading
    }

    pub fn connection_info(&self) -> Result<ConnectionInfo, String> {
        let guard = self.inner.lock().unwrap();
        let link = guard
            .link
            .as_ref()
            .ok_or_else(|| "ECU не подключена".to_string())?;
        Ok(link.info().clone())
    }

    /// Без блокировки, если порт занят операцией UI (стимуляция, config IO).
    pub fn connection_info_if_available(&self) -> Option<ConnectionInfo> {
        let guard = self.inner.try_lock().ok()?;
        guard.link.as_ref().map(|link| link.info().clone())
    }

    pub fn is_io_locked(&self) -> bool {
        self.inner.try_lock().is_err()
    }

    pub fn connect(&self, port: &str, baud_rate: u32) -> Result<ConnectionInfo, String> {
        self.connect_internal(port, baud_rate, false)
    }

    pub fn connect_automatic(&self, port: &str, baud_rate: u32) -> Result<ConnectionInfo, String> {
        self.connect_internal(port, baud_rate, true)
    }

    fn connect_internal(
        &self,
        port: &str,
        baud_rate: u32,
        automatic: bool,
    ) -> Result<ConnectionInfo, String> {
        self.output.stop();
        self.composite().disable_on_ecu(self);
        self.composite().stop();
        self.config.stop();

        {
            let guard = self
                .inner
                .try_lock()
                .map_err(|_| "ECU занята операцией интерфейса — повторите позже".to_string())?;
            if guard.link.is_some() {
                return Err("ECU уже подключена".into());
            }
        }

        let tracer =
            Some(Arc::clone(&self.protocol_log) as Arc<dyn rusefi_protocol::ProtocolTracer>);
        let link = SerialLink::connect(port, baud_rate, DEFAULT_IO_TIMEOUT_MS, tracer)
            .map_err(protocol_error_message)?;
        let info = link.info().clone();

        let resolved = match resolve_ini_for_signature(&info.signature) {
            Ok(resolved) => resolved,
            Err(e) => {
                self.protocol_log.log_info(&format!(
                    "Подключение отклонено: {e} (signature={})",
                    info.signature
                ));
                return Err(e.to_string());
            }
        };

        let ini_path = resolved.path.clone();
        self.apply_ini(resolved);
        let ini_ctx = self.ini_context();

        let mut guard = self
            .inner
            .try_lock()
            .map_err(|_| "ECU занята операцией интерфейса — повторите позже".to_string())?;
        if guard.link.is_some() {
            return Err("ECU уже подключена".into());
        }

        self.protocol_log.log_ecu_connected(
            automatic,
            &info.port_name,
            info.baud_rate,
            &info.signature,
        );
        self.protocol_log.log_info(&format!("INI загружен: {}", ini_path.display()));
        guard.link = Some(link);
        self.start_output_data_log(&info, &ini_ctx, &ini_path);
        Ok(info)
    }

    pub fn disconnect(&self) {
        self.disconnect_reason("отключение по запросу", false);
    }

    pub fn disconnect_reason(&self, reason: &str, automatic: bool) {
        self.set_stimulation_active(false);
        self.composite().disable_on_ecu(self);
        self.output().stop();
        self.composite().stop();
        self.config().stop();
        self.stop_output_data_log();
        let Ok(mut guard) = self.inner.try_lock() else {
            return;
        };
        if let Some(link) = guard.link.take() {
            let port = link.info().port_name.clone();
            self.protocol_log
                .log_ecu_disconnected(automatic, &port, reason);
        }
    }

    pub fn with_link<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&mut SerialLink) -> Result<R, ProtocolError>,
    {
        let mut guard = self.inner.lock().unwrap();
        let link = guard
            .link
            .as_mut()
            .ok_or_else(|| "ECU не подключена".to_string())?;
        f(link).map_err(|e| e.to_string())
    }

    /// Как `with_link`, но не блокирует вызывающий поток (poll / autoconnect).
    pub fn try_with_link<F, R>(&self, f: F) -> Option<Result<R, String>>
    where
        F: FnOnce(&mut SerialLink) -> Result<R, ProtocolError>,
    {
        let mut guard = self.inner.try_lock().ok()?;
        let link = guard.link.as_mut()?;
        Some(f(link).map_err(|e| e.to_string()))
    }

    /// Останавливает poll `O`, выполняет `f`, не перезапускает poll (вызовите `output().start` снаружи).
    pub fn run_without_output_poll<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Self) -> Result<R, String>,
    {
        self.output().stop();
        self.composite().disable_on_ecu(self);
        self.composite().stop();
        f(self)
    }

    /// Консольные `E`: disable → rpm → enable (см. Java `RpmCommand` / `trigger_emulator_algo.cpp`).
    pub fn run_stimulator_start(self: &Arc<Self>, rpm: u16) -> Result<(), String> {
        if !self.is_connected() {
            return Err("ECU не подключена".into());
        }

        self.set_stimulation_active(true);
        let delay = Duration::from_millis(u64::from(self.ini_context().inter_write_delay_ms));

        let result = self.run_without_output_poll(|session| {
            session.with_link(|link| {
                link.execute_console_command(&format!("disable {STIMULATOR_CMD}"))
            })?;
            thread::sleep(delay);
            session.with_link(|link| link.execute_console_command(&format!("rpm {rpm}")))?;
            thread::sleep(delay);
            session.with_link(|link| {
                link.execute_console_command(&format!("enable {STIMULATOR_CMD}"))
            })
        });

        match &result {
            Ok(()) => {
                let _ = self.patch_trigger_rpm_cache(rpm);
            }
            Err(_) => self.set_stimulation_active(false),
        }

        result
    }

    pub fn run_stimulator_stop(self: &Arc<Self>) -> Result<(), String> {
        if !self.is_connected() {
            self.set_stimulation_active(false);
            return Err("ECU не подключена".into());
        }

        let result = self.run_without_output_poll(|session| {
            session.with_link(|link| {
                link.execute_console_command(&format!("disable {STIMULATOR_CMD}"))
            })
        });
        self.set_stimulation_active(false);
        result
    }

    fn patch_trigger_rpm_cache(&self, rpm: u16) -> Result<(), String> {
        let ini = self.ini_context();
        let field = ini
            .config_fields
            .get(TRIGGER_RPM_FIELD)
            .ok_or_else(|| format!("поле {TRIGGER_RPM_FIELD} не найдено в INI"))?;
        let offset = match field {
            ConfigFieldKind::Scalar(s) => s.offset,
            _ => return Err(format!("{TRIGGER_RPM_FIELD}: ожидался scalar")),
        };
        let raw = self.config().page_raw();
        let encoded = encode_config_value(field, f64::from(rpm), &raw)
            .ok_or_else(|| format!("не удалось закодировать {TRIGGER_RPM_FIELD}"))?;
        self.config().patch_page_raw(offset as usize, &encoded);
        Ok(())
    }
}

fn protocol_error_message(e: ProtocolError) -> String {
    match e {
        ProtocolError::PortBusy { port_name, detail } => format!(
            "Порт {port_name} занят другим приложением ({detail}). \
             Закройте TunerStudio или отключите ECU там."
        ),
        other => other.to_string(),
    }
}
