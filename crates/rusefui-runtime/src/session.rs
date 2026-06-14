use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rusefi_ini::{encode_config_value, ConfigFieldKind, IniFile};
use rusefi_protocol::{ConnectionInfo, ProtocolError, SerialLink, DEFAULT_IO_TIMEOUT_MS};
use serde::Serialize;

use crate::ini::{
    download_ini_for_signature, ensure_panels_for_ini, install_ini_to_cache, load_ini_path,
    panels_root_for_project, resolve_ini_for_signature, IniResolveError, OnlineDownloadStatus,
    PanelCacheStatus, ResolvedIni,
};
use crate::protocol_log::ProtocolLogStore;
use crate::stimulator_ramp::StimulatorRampRunner;
use crate::sources::composite_data_log::CompositeDataLogWriter;
use crate::sources::composite_logger::{CompositeEventJson, CompositeLoggerSource};
use crate::sources::knock_scope::KnockScopeSource;
use crate::sources::composite_timeline::{
    CompositeTimeline, CompositeTimelineStatus, CompositeTimelineView,
    CompositeTimelineViewQuery,
};
use crate::sources::config::ConfigSource;
use crate::sources::output_channels::{IniContext, OutputChannelsSource};
use crate::sources::output_data_log::OutputDataLogWriter;
use crate::sources::output_timeline::{
    OutputTimeline, OutputTimelineSeriesQuery, OutputTimelineSeriesSnapshot,
    OutputTimelineChunkQuery, OutputTimelineSeriesChunk,
    OutputTimelineStatus, OutputTimelineView, OutputTimelineViewControl,
    OutputTimelineViewQuery,
};

const STIMULATOR_CMD: &str = "self_stimulation";
const TRIGGER_RPM_FIELD: &str = "triggerSimulatorRpm";

/// Состояние ожидания выбора INI: link к ECU установлен, signature прочитана,
/// но подходящий INI ещё не выбран (mismatch / not found / forced flow).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingIniResolution {
    pub ecu_signature: String,
    pub port_name: String,
    /// Текст ошибки, который привёл к ожиданию.
    pub last_error: String,
    /// Что вернула попытка online-загрузки при connect.
    pub online: OnlineDownloadStatus,
    /// Signature из файла проекта, если отличалась от ECU при connect.
    pub project_signature: Option<String>,
    /// INI уже найден автоматически, но не применён до подтверждения пользователя.
    pub suggested_ini_path: Option<String>,
}

struct EcuSessionInner {
    link: Option<SerialLink>,
    pending: Option<PendingIniResolution>,
}

/// Общая сессия ECU: serial link + фоновый опрос output channels.
pub struct EcuSession {
    inner: Mutex<EcuSessionInner>,
    ini: Mutex<IniContext>,
    loaded_ini_path: Mutex<Option<PathBuf>>,
    output: OutputChannelsSource,
    composite: CompositeLoggerSource,
    knock_scope: KnockScopeSource,
    config: ConfigSource,
    protocol_log: Arc<ProtocolLogStore>,
    /// Пока true — не запускать poll `O` (конфликт с консольными `E` на том же порту).
    stimulation_active: AtomicBool,
    /// После stop knock scope — перезапустить output poll (см. `sync_output_poll_session`).
    output_poll_resync: AtomicBool,
    output_data_log: Mutex<Option<OutputDataLogWriter>>,
    output_timeline: Mutex<OutputTimeline>,
    composite_data_log: Mutex<Option<Arc<Mutex<CompositeDataLogWriter>>>>,
    composite_timeline: Mutex<CompositeTimeline>,
    log_viewport_linked: AtomicBool,
    /// Signature из открытого проекта (`project.ini`) — для сравнения при connect.
    project_ini_signature: Mutex<Option<String>>,
    /// Hash signature активного panel-cache (`4139280449`).
    active_panel_hash: Mutex<Option<String>>,
    /// Корень `ui_panels/` внутри открытого проекта (или scratch-каталог).
    panels_root: Mutex<PathBuf>,
    panels_changed_hook: Mutex<Option<Arc<dyn Fn(PanelCacheStatus) + Send + Sync>>>,
    stimulator_ramp: StimulatorRampRunner,
}

impl EcuSession {
    pub fn new_arc(protocol_log: Arc<ProtocolLogStore>) -> Arc<Self> {
        let ini_ctx = IniContext::disconnected();
        Arc::new(Self {
            inner: Mutex::new(EcuSessionInner {
                link: None,
                pending: None,
            }),
            ini: Mutex::new(ini_ctx.clone()),
            loaded_ini_path: Mutex::new(None),
            output: OutputChannelsSource::new(ini_ctx.clone()),
            composite: CompositeLoggerSource::new(),
            knock_scope: KnockScopeSource::new(),
            config: ConfigSource::new(ini_ctx),
            protocol_log,
            stimulation_active: AtomicBool::new(false),
            output_poll_resync: AtomicBool::new(false),
            output_data_log: Mutex::new(None),
            output_timeline: Mutex::new(OutputTimeline::default()),
            composite_data_log: Mutex::new(None),
            composite_timeline: Mutex::new(CompositeTimeline::default()),
            log_viewport_linked: AtomicBool::new(false),
            project_ini_signature: Mutex::new(None),
            active_panel_hash: Mutex::new(None),
            panels_root: Mutex::new(panels_root_for_project(None)),
            panels_changed_hook: Mutex::new(None),
            stimulator_ramp: StimulatorRampRunner::new(),
        })
    }

    pub fn set_panels_changed_hook(&self, hook: Arc<dyn Fn(PanelCacheStatus) + Send + Sync>) {
        *self.panels_changed_hook.lock().unwrap() = Some(hook);
    }

    pub fn active_panel_hash(&self) -> Option<String> {
        self.active_panel_hash.lock().unwrap().clone()
    }

    pub fn panels_root(&self) -> PathBuf {
        self.panels_root.lock().unwrap().clone()
    }

    pub fn set_project_panels_root(&self, project_dir: Option<&std::path::Path>) {
        *self.panels_root.lock().unwrap() = panels_root_for_project(project_dir);
    }

    pub fn reset_panel_cache_state(&self) {
        *self.active_panel_hash.lock().unwrap() = None;
        self.set_project_panels_root(None);
    }

    /// Panel-cache для текущего INI (cache miss → генерация). Не вызывать из offline bootstrap.
    pub fn ensure_ui_panels(&self) -> Result<Option<PanelCacheStatus>, String> {
        let signature = self
            .ini_context()
            .signature
            .ok_or_else(|| "INI без signature — нельзя построить panel cache".to_string())?;
        let path = self
            .loaded_ini_path
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "INI path не задан — нельзя построить panel cache".to_string())?;

        let prev = self.active_panel_hash.lock().unwrap().clone();
        let panels_root = self.panels_root();
        let status = ensure_panels_for_ini(&path, &signature, &panels_root)?;
        *self.active_panel_hash.lock().unwrap() = Some(status.hash.clone());

        if prev.as_deref() != Some(status.hash.as_str()) || status.generated {
            if let Some(hook) = self.panels_changed_hook.lock().unwrap().as_ref() {
                hook(status.clone());
            }
            return Ok(Some(status));
        }
        Ok(None)
    }

    pub fn log_panel_cache_error(&self, context: &str, err: String) {
        self.protocol_log
            .log_info(&format!("UI panels cache ({context}): {err}"));
    }

    pub fn set_project_ini_signature(&self, signature: Option<String>) {
        *self.project_ini_signature.lock().unwrap() = signature;
    }

    fn project_ini_signature(&self) -> Option<String> {
        self.project_ini_signature.lock().unwrap().clone()
    }

    pub fn log_viewport_linked(&self) -> bool {
        self.log_viewport_linked.load(Ordering::Relaxed)
    }

    pub fn set_log_viewport_linked(&self, linked: bool) {
        self.log_viewport_linked.store(linked, Ordering::Relaxed);
    }

    /// Скопировать окно output → composite (после включения «Связать с Log»).
    pub fn sync_composite_viewport_from_output(&self) {
        if !self.log_viewport_linked.load(Ordering::Relaxed) {
            return;
        }
        let ctrl = self.output_timeline.lock().unwrap().view_control_snapshot();
        let _ = self.composite_timeline.lock().unwrap().apply_view_control(ctrl);
    }

    pub fn composite_timeline_status(&self) -> CompositeTimelineStatus {
        self.composite_timeline.lock().unwrap().status()
    }

    pub fn composite_timeline_query(
        &self,
        query: CompositeTimelineViewQuery,
    ) -> CompositeTimelineView {
        self.composite_timeline.lock().unwrap().query_view(&query)
    }

    pub fn composite_timeline_session_events(&self) -> Vec<CompositeEventJson> {
        self.composite_timeline.lock().unwrap().session_events()
    }

    pub fn composite_timeline_control(
        &self,
        ctrl: OutputTimelineViewControl,
    ) -> CompositeTimelineStatus {
        let mut composite = self.composite_timeline.lock().unwrap();
        let st = composite.apply_view_control(ctrl.clone());
        if self.log_viewport_linked.load(Ordering::Relaxed) {
            drop(composite);
            let _ = self.output_timeline.lock().unwrap().apply_view_control(ctrl);
        }
        st
    }

    pub fn composite_timeline_load_file(
        &self,
        path: PathBuf,
    ) -> Result<CompositeTimelineStatus, String> {
        let mut ct = self.composite_timeline.lock().unwrap();
        ct.load_file(path)?;
        Ok(ct.status())
    }

    pub fn open_composite_log(&self) -> Result<Arc<Mutex<CompositeDataLogWriter>>, String> {
        let info = self.connection_info()?;
        let started_ms = self.output_timeline.lock().unwrap().session_start_ms();
        let writer = CompositeDataLogWriter::open_at(
            &info,
            self.loaded_ini_path().as_deref(),
            started_ms,
        )?;
        let arc = Arc::new(Mutex::new(writer));
        *self.composite_data_log.lock().unwrap() = Some(arc.clone());
        self.composite_timeline.lock().unwrap().clear();
        self.composite_timeline.lock().unwrap().set_live_capture(true);
        if self.log_viewport_linked.load(Ordering::Relaxed) {
            self.sync_composite_viewport_from_output();
        }
        Ok(arc)
    }

    /// После остановки poll-потока: закрыть CSV и загрузить в viewer-proxy.
    pub fn close_composite_log(&self) -> Result<Option<String>, String> {
        let Some(arc) = self.composite_data_log.lock().unwrap().take() else {
            return Ok(None);
        };
        let writer = match Arc::try_unwrap(arc) {
            Ok(m) => m.into_inner().map_err(|_| "composite log lock")?,
            Err(_) => return Err("composite log ещё используется poll-потоком".into()),
        };
        let (path, rows) = writer.close()?;
        self.protocol_log.log_info(&format!(
            "Composite log: {} ({rows} строк)",
            path.display()
        ));
        self.composite_timeline
            .lock()
            .unwrap()
            .load_file(path.clone())?;
        self.composite_timeline
            .lock()
            .unwrap()
            .set_live_capture(false);
        if self.log_viewport_linked.load(Ordering::Relaxed) {
            self.sync_composite_viewport_from_output();
        }
        Ok(Some(path.display().to_string()))
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

    pub fn output_timeline_series_snapshot(
        &self,
        query: OutputTimelineSeriesQuery,
    ) -> OutputTimelineSeriesSnapshot {
        self.output_timeline.lock().unwrap().series_snapshot(&query)
    }

    pub fn output_timeline_pull_series_chunk(
        &self,
        query: OutputTimelineChunkQuery,
    ) -> OutputTimelineSeriesChunk {
        self.output_timeline.lock().unwrap().pull_series_chunk(&query)
    }

    pub fn output_timeline_control(
        &self,
        ctrl: OutputTimelineViewControl,
    ) -> OutputTimelineStatus {
        let st = self
            .output_timeline
            .lock()
            .unwrap()
            .apply_view_control(ctrl.clone());
        if self.log_viewport_linked.load(Ordering::Relaxed) {
            let _ = self
                .composite_timeline
                .lock()
                .unwrap()
                .apply_view_control(ctrl);
        }
        st
    }

    pub fn output_timeline_load_file(&self, path: PathBuf) -> OutputTimelineStatus {
        let mut tl = self.output_timeline.lock().unwrap();
        tl.load_file(path);
        tl.status()
    }

    pub fn set_output_log_cursor_sec(&self, sec: Option<f64>) -> bool {
        self.output_timeline.lock().unwrap().set_log_cursor_sec(sec)
    }

    fn needs_log_cursor_output_sample(&self) -> bool {
        let tl = self.output_timeline.lock().unwrap();
        if tl.has_log_cursor() {
            return true;
        }
        let st = tl.status();
        st.data_max_sec > st.data_min_sec + 1e-12
    }

    /// Глобальные «текущие output»: live с ECU или срез лога в курсоре timeline.
    pub fn current_output_snapshot(&self) -> crate::sources::output_channels::OutputSnapshot {
        use crate::sources::output_channels::OutputValuesSource;
        if self.is_connected() {
            let mut snap = self.output().snapshot();
            snap.values_source = OutputValuesSource::Live;
            if snap.sample_sec.is_none() {
                snap.sample_sec = Some(snap.timeline_live_sec);
            }
            return snap;
        }
        if self.needs_log_cursor_output_sample() {
            return self.output_snapshot_from_log_cursor(0.0, None);
        }
        let mut snap = self.output().snapshot();
        snap.values_source = OutputValuesSource::LogCursor;
        snap.sample_sec = None;
        snap
    }

    pub(crate) fn output_snapshot_from_log_cursor(
        &self,
        poll_hz: f64,
        last_error: Option<String>,
    ) -> crate::sources::output_channels::OutputSnapshot {
        use crate::sources::output_channels::{OutputSnapshot, OutputValuesSource};
        let ini = self.ini_context();
        let tl = self.output_timeline.lock().unwrap();
        let t = tl.effective_cursor_sec();
        let values = tl.sample_all_at(t);
        OutputSnapshot {
            connected: false,
            poll_hz,
            raw_len: 0,
            values,
            last_error,
            ini_signature: ini.signature.clone(),
            ini_field_count: ini.channels.fields.len(),
            session_log_path: tl
                .session_log_path()
                .or_else(|| self.output_session_log_path()),
            timeline_live_sec: tl.live_sec(),
            values_source: OutputValuesSource::LogCursor,
            sample_sec: Some(t),
        }
    }

    /// Пустой рабочий стол: config, timeline, лог сессии (без отключения ECU).
    pub fn reset_workspace_for_new_project(&self) {
        *self.active_panel_hash.lock().unwrap() = None;
        self.config().stop();
        self.output().stop();
        self.composite().stop();
        self.knock_scope().try_disable_on_ecu(self);
        self.knock_scope().reset_idle();
        let _ = self.stop_output_data_log();
        *self.composite_data_log.lock().unwrap() = None;
        self.composite_timeline.lock().unwrap().clear();
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

    /// После stop knock scope — перезапустить poll `O` при следующем `sync_output_poll_session`.
    pub fn request_output_poll_resync(&self) {
        self.output_poll_resync.store(true, Ordering::SeqCst);
    }

    pub fn take_output_poll_resync(&self) -> bool {
        self.output_poll_resync.swap(false, Ordering::SeqCst)
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

    pub fn clear_pending_ini_resolution(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.pending = None;
        }
    }

    /// Проект без `ini` в JSON — ждём явный выбор/загрузку файла (offline или с ECU).
    pub fn set_pending_project_ini_required(
        &self,
        reason: impl Into<String>,
        project_signature: Option<String>,
    ) {
        let (ecu_signature, port_name) = self
            .connection_info_if_available()
            .map(|i| (i.signature, i.port_name))
            .unwrap_or_default();
        let online = if ecu_signature.is_empty() {
            OnlineDownloadStatus::NotApplicable
        } else {
            download_ini_for_signature(&ecu_signature)
        };
        if let Ok(mut guard) = self.inner.lock() {
            guard.pending = Some(PendingIniResolution {
                ecu_signature,
                port_name,
                last_error: reason.into(),
                online,
                project_signature,
                suggested_ini_path: None,
            });
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
        if let Err(e) = self.ensure_ui_panels() {
            self.log_panel_cache_error("load_ini_from_path", e);
        }
        Ok(())
    }

    /// Применить INI к активному ECU (или offline-проекту) с возможностью отключить
    /// проверку signature (`force = true`). Если link жив и был в pending resolution
    /// — завершает подключение: финализирует data-log, очищает pending.
    pub fn apply_ini_with_options(
        &self,
        path: &std::path::Path,
        force: bool,
    ) -> Result<(), String> {
        let file = IniFile::load_file(path).map_err(|e| {
            IniResolveError::LoadFailed {
                path: path.to_path_buf(),
                message: e.to_string(),
            }
            .to_string()
        })?;
        if file.output_channels.fields.is_empty() {
            return Err(format!("В INI нет [OutputChannels]: {}", path.display()));
        }

        let ecu_signature = self
            .connection_info_if_available()
            .map(|i| i.signature)
            .or_else(|| {
                self.inner
                    .lock()
                    .ok()
                    .and_then(|g| g.pending.as_ref().map(|p| p.ecu_signature.clone()))
            })
            .filter(|s| !s.is_empty());

        if let Some(ecu_sig) = ecu_signature.as_deref() {
            match file.signature.as_deref() {
                Some(ini_sig) if ini_sig == ecu_sig => {}
                Some(ini_sig) => {
                    if !force {
                        return Err(format!(
                            "signature не совпадает: ECU={ecu_sig}, INI={ini_sig}"
                        ));
                    }
                    self.protocol_log.log_info(&format!(
                        "INI применён принудительно (force): signature ECU={ecu_sig}, INI={ini_sig}"
                    ));
                }
                None => {
                    if !force {
                        return Err(format!(
                            "в INI нет поля signature: {}",
                            path.display()
                        ));
                    }
                    self.protocol_log.log_info(&format!(
                        "INI применён принудительно (force): в файле нет signature ({})",
                        path.display()
                    ));
                }
            }
        }

        let cache_path = install_ini_to_cache(path, &file).map_err(|e| e.to_string())?;
        if cache_path != path {
            self.protocol_log.log_info(&format!(
                "INI установлен в кэш: {} → {}",
                path.display(),
                cache_path.display()
            ));
        }
        let resolved = ResolvedIni {
            path: cache_path,
            file,
        };
        self.apply_ini(resolved);
        if let Err(e) = self.ensure_ui_panels() {
            self.log_panel_cache_error("apply_ini_with_options", e);
        }

        // Если link был в pending — финализируем подключение.
        let mut finalize = None;
        if let Ok(mut guard) = self.inner.lock() {
            if guard.pending.is_some() {
                if let Some(link) = guard.link.as_ref() {
                    finalize = Some(link.info().clone());
                }
                guard.pending = None;
            }
        }
        if let Some(info) = finalize {
            let ini_ctx = self.ini_context();
            if let Some(ini_path) = self.loaded_ini_path() {
                self.start_output_data_log(&info, &ini_ctx, &ini_path);
            }
        }
        Ok(())
    }

    pub fn pending_ini_resolution(&self) -> Option<PendingIniResolution> {
        self.inner.lock().ok().and_then(|g| g.pending.clone())
    }

    pub fn has_pending_ini_resolution(&self) -> bool {
        self.inner
            .lock()
            .map(|g| g.pending.is_some())
            .unwrap_or(false)
    }

    /// Повторная попытка online-загрузки INI с rusefi.com для signature
    /// текущего pending. Если успешно — сразу применяет.
    pub fn retry_online_ini_resolution(&self) -> Result<String, String> {
        let signature = self
            .pending_ini_resolution()
            .map(|p| p.ecu_signature)
            .ok_or_else(|| "Нет активного ожидания выбора INI".to_string())?;
        let status = download_ini_for_signature(&signature);
        if let Ok(mut guard) = self.inner.lock() {
            if let Some(p) = guard.pending.as_mut() {
                p.online = status.clone();
            }
        }
        match status {
            OnlineDownloadStatus::Succeeded { path, .. } => {
                self.apply_ini_with_options(std::path::Path::new(&path), false)?;
                Ok(path)
            }
            OnlineDownloadStatus::NotApplicable => Err(
                "Signature ECU не парсится в URL — online-загрузка невозможна".into(),
            ),
            OnlineDownloadStatus::NotAttempted { reason } => {
                Err(format!("Загрузка отключена: {reason}"))
            }
            OnlineDownloadStatus::Failed { url, error } => {
                Err(format!("Не удалось скачать {url}: {error}"))
            }
        }
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

    pub fn knock_scope(&self) -> &KnockScopeSource {
        &self.knock_scope
    }

    /// Снимок knock scope; `connected` всегда из живой сессии (как футер / autoconnect).
    pub fn knock_scope_snapshot(&self) -> crate::sources::knock_scope::KnockScopeSnapshot {
        let mut snap = self.knock_scope.snapshot();
        snap.connected = self.is_connected();
        snap
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
        self.knock_scope().disable_on_ecu(self);
        self.knock_scope().stop();
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

        let resolve_result = resolve_ini_for_signature(&info.signature);
        let project_sig = self.project_ini_signature();
        let project_sig_mismatch = project_sig
            .as_deref()
            .filter(|s| !s.is_empty())
            .is_some_and(|ps| !info.signature.is_empty() && ps != info.signature);

        let mut guard = self
            .inner
            .try_lock()
            .map_err(|_| "ECU занята операцией интерфейса — повторите позже".to_string())?;
        if guard.link.is_some() {
            return Err("ECU уже подключена".into());
        }

        let needs_user_ini = resolve_result.is_err() || project_sig_mismatch;

        if needs_user_ini {
            let online = if resolve_result.is_err() {
                download_ini_for_signature(&info.signature)
            } else {
                OnlineDownloadStatus::NotAttempted {
                    reason: "INI найден, но signature проекта не совпадает с ECU — нужен выбор"
                        .into(),
                }
            };
            let last_error = match (&resolve_result, project_sig_mismatch) {
                (Err(e), _) => e.to_string(),
                (Ok(_), true) => format!(
                    "signature проекта не совпадает с ECU: project={}, ecu={}",
                    project_sig.as_deref().unwrap_or("?"),
                    info.signature
                ),
                _ => "Требуется выбор INI".into(),
            };
            let suggested_ini_path = resolve_result
                .ok()
                .map(|r| r.path.display().to_string());
            self.protocol_log.log_info(&format!(
                "ECU подключена, ожидание выбора INI: {last_error}"
            ));
            self.protocol_log.log_ecu_connected(
                automatic,
                &info.port_name,
                info.baud_rate,
                &info.signature,
            );
            guard.link = Some(link);
            guard.pending = Some(PendingIniResolution {
                ecu_signature: info.signature.clone(),
                port_name: info.port_name.clone(),
                last_error,
                online,
                project_signature: project_sig,
                suggested_ini_path,
            });
            return Ok(info);
        }

        let resolved = resolve_result.expect("needs_user_ini false => Ok");
        let ini_path = resolved.path.clone();
        drop(guard);
        self.apply_ini(resolved);
        if let Err(e) = self.ensure_ui_panels() {
            self.log_panel_cache_error("connect", e);
        }
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
        self.protocol_log
            .log_info(&format!("INI загружен: {}", ini_path.display()));
        guard.link = Some(link);
        guard.pending = None;
        drop(guard);
        self.start_output_data_log(&info, &ini_ctx, &ini_path);
        Ok(info)
    }

    pub fn disconnect(&self) {
        self.disconnect_reason("отключение по запросу", false);
    }

    pub fn disconnect_reason(&self, reason: &str, automatic: bool) {
        self.set_stimulation_active(false);
        self.stimulator_ramp.cancel_and_join();
        self.composite().disable_on_ecu(self);
        self.knock_scope().disable_on_ecu(self);
        self.knock_scope().stop();
        self.output().stop();
        self.composite().stop();
        self.config().stop();
        self.stop_output_data_log();
        let Ok(mut guard) = self.inner.try_lock() else {
            return;
        };
        guard.pending = None;
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

    /// Ждёт mutex порта до `timeout`, затем один раз выполняет `f` (для knock scope и др.).
    pub fn with_link_wait<F, R>(&self, timeout: Duration, f: F) -> Result<R, String>
    where
        F: FnOnce(&mut SerialLink) -> Result<R, ProtocolError>,
    {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(mut guard) = self.inner.try_lock() {
                let link = guard
                    .link
                    .as_mut()
                    .ok_or_else(|| "ECU не подключена".to_string())?;
                return f(link).map_err(|e| e.to_string());
            }
            if Instant::now() >= deadline {
                return Err("таймаут ожидания serial (порт занят)".into());
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// Останавливает poll `O`, выполняет `f`, не перезапускает poll (вызовите `output().start` снаружи).
    pub fn run_without_output_poll<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Self) -> Result<R, String>,
    {
        // Только `O`: `l`+composite/knock scope не трогаем — иначе запись триггера обрывается
        // после burn/config/stim и не возобновляется.
        self.output().stop();
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

    /// Смена RPM при уже включённой стимуляции: только `E` + `rpm N` (без disable/enable).
    /// `setTriggerEmulatorRPM` в прошивке; enable снова не нужен.
    pub fn run_stimulator_set_rpm(&self, rpm: u16) -> Result<(), String> {
        if !self.is_connected() {
            return Err("ECU не подключена".into());
        }
        if !self.is_stimulation_active() {
            return Err("Стимуляция не включена".into());
        }

        let result = self.with_link(|link| link.execute_console_command(&format!("rpm {rpm}")));
        if result.is_ok() {
            let _ = self.patch_trigger_rpm_cache(rpm);
        }
        result
    }

    pub fn stimulator_ramp(&self) -> &StimulatorRampRunner {
        &self.stimulator_ramp
    }

    /// Сырой CRC-payload из INI `[ControllerCommands]` (`cmd_etb_auto_calibrate` и т.д.).
    pub fn run_ts_ini_command(&self, command_key: &str) -> Result<(), String> {
        if !self.is_connected() {
            return Err("ECU не подключена".into());
        }
        let key = command_key.trim();
        if key.is_empty() {
            return Err("Не задана команда".into());
        }
        let payload = self
            .ini_context()
            .ts_commands
            .get(key)
            .cloned()
            .ok_or_else(|| format!("команда «{key}» не найдена в INI"))?;
        self.run_without_output_poll(|session| {
            session.with_link(|link| link.send_binary_command(&payload))
        })
    }

    /// `G` — не блокируя output-poll: пропускает цикл если шина занята.
    pub fn poll_console_text(&self) -> String {
        if !self.is_connected() {
            return String::new();
        }
        match self.try_with_link(|link| link.get_console_text()) {
            Some(Ok(text)) => text,
            _ => String::new(),
        }
    }

    /// Консольная `E` + чтение ответа `G` (Java console CommandQueue).
    pub fn run_console_command(self: &Arc<Self>, text: &str) -> Result<String, String> {
        if !self.is_connected() {
            return Err("ECU не подключена".into());
        }
        let text = text.trim();
        if text.is_empty() {
            return Err("Пустая команда".into());
        }

        let delay = Duration::from_millis(u64::from(self.ini_context().inter_write_delay_ms));
        self.run_without_output_poll(|session| {
            session.with_link(|link| {
                link.execute_console_command(text)?;
                thread::sleep(delay);
                let mut response = link.get_console_text()?;
                if response.trim().is_empty() {
                    thread::sleep(Duration::from_millis(100));
                    response = link.get_console_text()?;
                }
                Ok(response)
            })
        })
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
