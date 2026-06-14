use rusefui_runtime::{
    compute_config_diff, default_log_path, enumerate_local_candidates, parse_rusefi_signature,
    evaluate_checklist, AutoConnectManager, AutoConnectSnapshot, ComponentRuntime, CompositeEventJson, CompositeSnapshot,
    ComputeTriggerWheelsParams, KnockScopeSnapshot, KnockScopeUiTick, CompositeTimelineStatus, CompositeTimelineView,
    CompositeTimelineViewQuery, TriggerWheelsView, compute_trigger_wheels,
    ConfigDiffSnapshot, ConfigDiffStore, ConfigFieldInfo, ConfigSnapshot, ConfigSource, ChecklistRules,
    DiffSide,
    EcuSession, EcuSyncOnMount, IniCandidate, OnlineDownloadStatus, OutputFieldInfo,
    OutputSnapshot, OutputTimelineChunkQuery, OutputTimelineSeriesChunk,
    OutputTimelineSeriesQuery, OutputTimelineSeriesSnapshot, OutputTimelineStatus, OutputTimelineView,
    OutputTimelineViewControl,
    read_manifest_from_dir, read_panel_yaml, PanelManifest,
    CommitSummary, PendingIniResolution, ProjectGitRepo, ProjectInfo, ProjectListEntry,
    ProjectLogRef, ProjectScript, ProjectStore, ProjectTimelineClip,
    ProtocolLogEntry,
    ProtocolLogFilterSettings, ProtocolLogStore, RampCurveKind, RecentProjectEntry, RecentProjectsStore,
    RusefuiProject, StimulatorRampParams, StimulatorRampResult, StimulatorRampStep,
    DEFAULT_RAMP_STEP_MS, WorkspaceFsm, WorkspaceInputs, WorkspacePhase, WorkspaceSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

static LAST_PROTOCOL_LOG_UI_EMIT_MS: AtomicU64 = AtomicU64::new(0);
const PROTOCOL_LOG_UI_THROTTLE_MS: u64 = 150;

pub struct RuntimeState {
    pub session: Arc<EcuSession>,
    pub runtime: Mutex<ComponentRuntime>,
    pub protocol_log: Arc<ProtocolLogStore>,
    pub autoconnect: Arc<AutoConnectManager>,
    pub project: Mutex<ProjectStore>,
    pub config_diff: Mutex<ConfigDiffStore>,
    pub workspace_fsm: Mutex<WorkspaceFsm>,
    pub recent_projects: Mutex<RecentProjectsStore>,
    /// Последнее отправленное в UI `ecu-connection` (без дублей на каждый dispatch).
    last_ecu_connection_emit: Mutex<Option<EcuConnectionEvent>>,
    /// RAM ECU изменена, но ещё не записана во flash (B-команда).
    pub ram_dirty: Arc<AtomicBool>,
    /// Правила checklist из checklist.yaml (загружаются с фронта).
    pub checklist_rules: Mutex<Option<ChecklistRules>>,
    /// Сериализация `sync_ecu_data` (autoconnect + invoke не параллелят stop/poll).
    ecu_sync: Mutex<()>,
}

impl RuntimeState {
    pub fn new(protocol_log: Arc<ProtocolLogStore>) -> Self {
        let session = EcuSession::new_arc(Arc::clone(&protocol_log));
        let autoconnect = AutoConnectManager::new(Arc::clone(&session));
        Self {
            session: Arc::clone(&session),
            runtime: Mutex::new(ComponentRuntime::new(session)),
            protocol_log,
            autoconnect,
            project: Mutex::new(ProjectStore::new()),
            config_diff: Mutex::new(ConfigDiffStore::default()),
            workspace_fsm: Mutex::new(WorkspaceFsm::new()),
            recent_projects: Mutex::new(RecentProjectsStore::new()),
            last_ecu_connection_emit: Mutex::new(None),
            ram_dirty: Arc::new(AtomicBool::new(false)),
            checklist_rules: Mutex::new(None),
            ecu_sync: Mutex::new(()),
        }
    }
}

fn workspace_inputs(state: &RuntimeState) -> WorkspaceInputs {
    WorkspaceInputs {
        project: state.project.lock().unwrap().info(),
        autoconnect: state.autoconnect.snapshot(),
        ecu_connected: state.session.is_connected(),
        ini_pending_resolution: state.session.has_pending_ini_resolution(),
        config: state.session.config().snapshot(),
    }
}

fn workspace_snapshot_for_ui(state: &RuntimeState, mut snap: WorkspaceSnapshot) -> WorkspaceSnapshot {
    snap.burn_pending = state.ram_dirty.load(Ordering::Relaxed);
    snap
}

fn emit_workspace_state(app: &AppHandle, state: &RuntimeState, snap: &WorkspaceSnapshot) {
    let snap = workspace_snapshot_for_ui(state, snap.clone());
    let _ = app.emit("workspace-state", snap);
}

fn emit_burn_pending(app: &AppHandle, pending: bool) {
    let _ = app.emit("burn-pending", pending);
}

/// Синхронизирует ram_dirty, событие burn-pending и поле burnPending в workspace-state.
fn set_burn_pending(state: &RuntimeState, app: &AppHandle, pending: bool) {
    let prev = state.ram_dirty.swap(pending, Ordering::Relaxed);
    if prev == pending {
        return;
    }
    emit_burn_pending(app, pending);
    let snap = state
        .workspace_fsm
        .lock()
        .unwrap()
        .snapshot()
        .cloned()
        .unwrap_or_else(|| workspace_inputs(state).derive());
    emit_workspace_state(app, state, &snap);
}

fn reconcile_workspace(state: &RuntimeState, app: &AppHandle) -> WorkspaceSnapshot {
    let mut fsm = state.workspace_fsm.lock().unwrap();
    let prev_phase = fsm.snapshot().map(|s| s.phase);
    let prev_pending = fsm.snapshot().map(|s| s.ini_pending_resolution).unwrap_or(false);
    let inputs = workspace_inputs(state);
    let (snap, _plan, changed) = fsm.reconcile(&inputs);
    drop(fsm);
    if changed {
        // Новый live-baseline с ECU или уход из live-редактирования — flash считаем актуальным.
        if snap.phase == WorkspacePhase::ConfigFromEcu {
            if prev_phase != Some(WorkspacePhase::ConfigFromEcu) {
                set_burn_pending(state, app, false);
            }
        } else {
            set_burn_pending(state, app, false);
        }
        emit_workspace_state(app, state, &snap);
        if prev_pending != snap.ini_pending_resolution {
            emit_ini_resolution(app, state);
        }
    }
    workspace_snapshot_for_ui(state, snap)
}

fn emit_config_diff(app: &AppHandle, snap: &ConfigDiffSnapshot) {
    let app = app.clone();
    let snap = snap.clone();
    tauri::async_runtime::spawn(async move {
        let _ = app.emit("config-diff", snap);
    });
}

fn clear_config_diff(state: &RuntimeState, app: &AppHandle) {
    state.config_diff.lock().unwrap().clear();
    emit_config_diff(app, &state.config_diff.lock().unwrap().snapshot());
}

fn try_start_config_diff(state: &RuntimeState, app: &AppHandle) {
    if state.session.has_pending_ini_resolution() {
        clear_config_diff(state, app);
        return;
    }

    let snap = state.session.config().snapshot();
    if !snap.loaded || snap.loading {
        return;
    }
    // Config diff имеет смысл только после применения INI и загрузки page 0 с ECU.
    if !snap.connected || snap.read_only {
        return;
    }

    let project_values = {
        let store = state.project.lock().unwrap();
        store
            .document()
            .ecu_config
            .as_ref()
            .map(|e| e.values.clone())
    };

    let Some(project_values) = project_values else {
        clear_config_diff(state, app);
        return;
    };

    let ini = state.session.ini_context();
    let entries = compute_config_diff(&project_values, &snap.values, &ini.config_fields);

    let mut diff = state.config_diff.lock().unwrap();
    if entries.is_empty() {
        diff.clear();
    } else {
        diff.start(entries);
    }
    let out = diff.snapshot();
    drop(diff);
    emit_config_diff(app, &out);
}

fn emit_project(app: &AppHandle, state: &RuntimeState) {
    let info = state.project.lock().unwrap().info();
    let _ = app.emit("project-changed", &info);
}

fn record_recent_project(state: &RuntimeState, path: &std::path::Path) {
    let _ = state
        .recent_projects
        .lock()
        .unwrap()
        .record(path);
}

/// Сброс config/timeline в UI после смены проекта.
fn emit_workspace_reset(app: &AppHandle, state: &RuntimeState) {
    println!(
        "[workspace-fsm] emit_workspace_reset project_path={:?}",
        state.project.lock().unwrap().info().path
    );
    state.workspace_fsm.lock().unwrap().reset();
    let _ = reconcile_workspace(state, app);
    sync_ecu_data(state, app);
    let snap = reconcile_workspace(state, app);
    let table_updates = state.runtime.lock().unwrap().reload_config_tables();
    for (instance_id, st) in table_updates {
        emit_state(app, &instance_id, &st);
    }
    let timeline = state.session.output_timeline_status();
    let _ = app.emit("output-timeline-status", timeline);
    let _ = app.emit("workspace-reset", ());
    emit_workspace_state(app, state, &snap);
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self::new(ProtocolLogStore::new(default_log_path()))
    }
}

#[derive(Clone, Serialize)]
struct ComponentStateEvent {
    instance_id: String,
    state: Value,
}

#[derive(Debug, Deserialize)]
pub struct MountParams {
    pub instance_id: String,
    pub component_type: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Deserialize)]
pub struct DispatchParams {
    pub instance_id: String,
    pub action: String,
    #[serde(default)]
    pub payload: Value,
}

fn emit_state(app: &AppHandle, instance_id: &str, state: &Value) {
    let _ = app.emit(
        "component-state",
        ComponentStateEvent {
            instance_id: instance_id.to_string(),
            state: state.clone(),
        },
    );
}

fn emit_output(app: &AppHandle, snapshot: &OutputSnapshot) {
    let app = app.clone();
    let snapshot = snapshot.clone();
    tauri::async_runtime::spawn(async move {
        let _ = app.emit("output-channels", snapshot);
    });
}

fn emit_current_output(app: &AppHandle, state: &RuntimeState) {
    emit_output(app, &state.session.current_output_snapshot());
}

fn emit_composite(app: &AppHandle, snapshot: &CompositeSnapshot) {
    let app = app.clone();
    let snapshot = snapshot.clone();
    tauri::async_runtime::spawn(async move {
        let _ = app.emit("composite-logger", snapshot);
    });
}

fn emit_knock_scope_tick(app: &AppHandle, tick: &KnockScopeUiTick) {
    let app = app.clone();
    let tick = tick.clone();
    tauri::async_runtime::spawn(async move {
        let _ = app.emit("knock-scope", tick);
    });
}

fn emit_knock_scope_reset(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = app.emit("knock-scope-reset", ());
    });
}

fn emit_composite_timeline(app: &AppHandle, status: &CompositeTimelineStatus) {
    let app = app.clone();
    let status = status.clone();
    tauri::async_runtime::spawn(async move {
        let _ = app.emit("composite-timeline-status", status);
    });
}

fn reset_workspace(state: &RuntimeState) {
    state.session.reset_workspace_for_new_project();
    if let Ok(rt) = state.runtime.lock() {
        rt.reset_workspace();
    }
}

fn enrich_config_snapshot(app: &AppHandle, mut snap: ConfigSnapshot) -> ConfigSnapshot {
    if let Some(state) = app.try_state::<RuntimeState>() {
        let rules = state.checklist_rules.lock().unwrap();
        if let Some(rules) = rules.as_ref() {
            let config = state.session.config();
            let ignition_gen = state
                .runtime
                .lock()
                .ok()
                .map(|rt| rt.ignition_gen_params())
                .unwrap_or_default();
            snap.checklist = evaluate_checklist(&snap, rules, &config, &ignition_gen);
        }
    }
    snap
}

fn config_snapshot_for_ui(app: &AppHandle, state: &RuntimeState) -> ConfigSnapshot {
    enrich_config_snapshot(app, state.session.config().snapshot())
}

fn emit_config(app: &AppHandle, snapshot: ConfigSnapshot) {
    let app = app.clone();
    let snapshot = enrich_config_snapshot(&app, snapshot);
    tauri::async_runtime::spawn(async move {
        let _ = app.emit("config-snapshot", snapshot);
    });
}

fn emit_protocol_log(app: &AppHandle, entry: &ProtocolLogEntry) {
    let _ = app.emit("protocol-log", entry);
}

#[derive(Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct EcuConnectionEvent {
    connected: bool,
    offline_mode: bool,
    port_name: Option<String>,
    baud_rate: Option<u32>,
    signature: Option<String>,
    handshake_command: Option<char>,
    last_error: Option<String>,
}

fn build_ecu_connection_event(state: &RuntimeState) -> EcuConnectionEvent {
    let offline_mode = state.autoconnect.is_offline_mode();
    if offline_mode {
        return EcuConnectionEvent {
            connected: false,
            offline_mode: true,
            port_name: None,
            baud_rate: None,
            signature: None,
            handshake_command: None,
            last_error: state.autoconnect.snapshot().last_error,
        };
    }

    if state.session.is_connected() {
        if let Some(info) = state.session.connection_info_if_available() {
            return EcuConnectionEvent {
                connected: true,
                offline_mode: false,
                port_name: Some(info.port_name),
                baud_rate: Some(info.baud_rate),
                signature: Some(info.signature),
                handshake_command: Some(info.handshake_command),
                last_error: None,
            };
        }
        return EcuConnectionEvent {
            connected: true,
            offline_mode: false,
            port_name: None,
            baud_rate: None,
            signature: None,
            handshake_command: None,
            last_error: None,
        };
    }

    EcuConnectionEvent {
        connected: false,
        offline_mode: false,
        port_name: None,
        baud_rate: None,
        signature: None,
        handshake_command: None,
        last_error: state.autoconnect.snapshot().last_error,
    }
}

/// Подключение ECU изменилось — `ecu-connection` только при смене + пересчёт FSM + опциональный sync.
pub fn schedule_ecu_notify(app: &AppHandle, sync_ecu: bool) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<RuntimeState>();
        emit_ecu_connection_if_changed(&app, &state);
        let should_sync = sync_ecu && state.session.is_connected() && !state.session.is_ecu_busy();
        if should_sync {
            sync_ecu_data(&state, &app);
        } else {
            let _ = reconcile_workspace(&state, &app);
        }
    });
}

/// Только scanning / busy_ports — без `ecu-connection` и без invoke в ConnectionPanel.
pub fn schedule_autoconnect_ui(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        emit_autoconnect_state(&app);
    });
}

pub fn emit_ecu_connection_if_changed(app: &AppHandle, state: &RuntimeState) {
    let event = build_ecu_connection_event(state);
    let mut last = state.last_ecu_connection_emit.lock().unwrap();
    if last.as_ref() == Some(&event) {
        return;
    }
    *last = Some(event.clone());
    let _ = app.emit("ecu-connection", event);
}

pub fn emit_autoconnect_state(app: &AppHandle) {
    let state = app.state::<RuntimeState>();
    let _ = app.emit("autoconnect-state", state.autoconnect.snapshot());
}

pub fn start_autoconnect(app: AppHandle) {
    let state = app.state::<RuntimeState>();
    let autoconnect = Arc::clone(&state.autoconnect);
    let app_ecu = app.clone();
    let app_scan = app.clone();
    autoconnect.start(
        move |sync_ecu| {
            schedule_ecu_notify(&app_ecu, sync_ecu);
        },
        move || {
            schedule_autoconnect_ui(&app_scan);
        },
    );
    state
        .session
        .protocol_log()
        .log_link(format!(
            "Лог протокола: {}",
            state.protocol_log.path().display()
        ));
    schedule_ecu_notify(&app, true);
    schedule_autoconnect_ui(&app);
    let _ = reconcile_workspace(&state, &app);
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigProgressEvent {
    loading: bool,
    progress: f64,
    bytes_loaded: u32,
    bytes_total: u32,
}

fn emit_config_update(app: &AppHandle, snap: &ConfigSnapshot) {
    if snap.loading && !snap.loaded {
        let app = app.clone();
        let event = ConfigProgressEvent {
            loading: snap.loading,
            progress: snap.progress,
            bytes_loaded: snap.bytes_loaded,
            bytes_total: snap.bytes_total,
        };
        tauri::async_runtime::spawn(async move {
            let _ = app.emit("config-progress", event);
        });
    } else {
        emit_config(app, snap.clone());
    }
}

fn sync_output_poll_session(session: &Arc<EcuSession>, app: &AppHandle) {
    if session.knock_scope().is_polling() {
        session.output().stop();
        return;
    }
    if session.should_poll_output_channels() {
        if session.output().is_polling() {
            return;
        }
        let app = app.clone();
        let poll_session = Arc::clone(session);
        session.output().start(poll_session, move |snap| {
            let state = app.state::<RuntimeState>();
            if let Ok(mut rt) = state.runtime.try_lock() {
                for (instance_id, st) in rt.feed_output(&snap) {
                    if st
                        .get("configUpdated")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        emit_config_update(&app, &state.session.config().snapshot());
                        persist_config_after_table_edit(&state, &app);
                    }
                    emit_state(&app, &instance_id, &st);
                }
            }
            emit_output(&app, &snap);
        });
    } else {
        session.output().stop();
        let state = app.state::<RuntimeState>();
        emit_current_output(app, &state);
    }
}

fn sync_config_load(state: &RuntimeState, app: &AppHandle) {
    let ws = reconcile_workspace(state, app);
    if ws.phase != WorkspacePhase::EcuConnectedIdle {
        println!(
            "[workspace-fsm] sync_config_load: skip (phase={:?}, expected EcuConnectedIdle)",
            ws.phase
        );
        return;
    }

    let cfg = state.session.config().snapshot();
    if cfg.loading {
        println!("[workspace-fsm] sync_config_load: skip (config already loading)");
        return;
    }
    // Live config с ECU уже в RAM.
    if cfg.loaded && !cfg.read_only {
        println!("[workspace-fsm] sync_config_load: skip (live config already loaded)");
        return;
    }
    println!("[workspace-fsm] sync_config_load: starting ECU page 0 load");
    // Preview из проекта не сбрасываем: start_load подменит RAM только после
    // успешного чтения page 1 с ECU (иначе при 0x84 на доп. страницах UI пустой).

    state.session.output().stop();
    state.session.composite().stop();

    let app = app.clone();
    let session = Arc::clone(&state.session);
    state.session.config().start_load(session.clone(), move |snap| {
        emit_config_update(&app, &snap);
        if !snap.loading {
            if snap.loaded {
                let st = app.state::<RuntimeState>();
                try_start_config_diff(&st, &app);
                // Свежий снимок с ECU — RAM совпадает с тем, что только что прочитали.
                set_burn_pending(&st, &app, false);
                let _ = reconcile_workspace(&st, &app);
            }
            sync_output_poll_session(&session, &app);
        }
    });
}

/// Синхронизация подсистем по фазе workspace (стейт-машина).
fn sync_ecu_data(state: &RuntimeState, app: &AppHandle) {
    let _sync_guard = state
        .ecu_sync
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let ws = reconcile_workspace(state, app);

    println!(
        "[workspace-fsm] sync_ecu_data: phase={:?} config_source={:?} ecu_connected={} config_loaded={} config_loading={}",
        ws.phase,
        ws.config_source,
        ws.ecu_connected,
        ws.config_loaded,
        ws.config_loading,
    );

    match ws.phase {
        WorkspacePhase::Gate => {
            println!("[workspace-fsm] sync_ecu_data action: gate — stop all sources");
            state.session.knock_scope().stop();
            state.session.config().stop();
            state.session.output().stop();
            state.session.composite().disable_on_ecu(&state.session);
            state.session.composite().stop();
            clear_config_diff(state, app);
        }
        WorkspacePhase::ProjectOnly | WorkspacePhase::EcuScanning => {
            println!(
                "[workspace-fsm] sync_ecu_data action: {:?} — stop poll/composite{}",
                ws.phase,
                if ws.config_source == ConfigSource::EcuLive {
                    ", stop live config"
                } else {
                    ""
                }
            );
            state.session.knock_scope().stop();
            if ws.config_source == ConfigSource::EcuLive {
                state.session.config().stop();
                clear_config_diff(state, app);
            }
            state.session.output().stop();
            state.session.composite().disable_on_ecu(&state.session);
            state.session.composite().stop();
        }
        WorkspacePhase::EcuIniMismatch => {
            println!("[workspace-fsm] sync_ecu_data action: ecu_ini_mismatch — stop all sources, keep link");
            state.session.knock_scope().stop();
            // Ждём выбора INI: глушим все источники, но link не разрываем.
            state.session.config().stop();
            state.session.output().stop();
            state.session.composite().disable_on_ecu(&state.session);
            state.session.composite().stop();
            clear_config_diff(state, app);
        }
        WorkspacePhase::EcuConnectedIdle => {
            println!("[workspace-fsm] sync_ecu_data action: ecu_connected_idle — sync_config_load");
            // Не дергать composite.stop() здесь: sync_config_load остановит перед load,
            // а частые sync_ecu_data иначе глушили бы запись триггера без перезапуска.
            sync_config_load(state, app);
        }
        WorkspacePhase::ConfigFromProject => {
            println!(
                "[workspace-fsm] sync_ecu_data action: config_from_project — poll_output={}",
                ws.capabilities.poll_output_channels
            );
            if ws.capabilities.poll_output_channels {
                sync_output_poll_session(&state.session, app);
            } else {
                state.session.output().stop();
                emit_current_output(app, state);
            }
        }
        WorkspacePhase::ConfigLoadingFromEcu => {
            println!("[workspace-fsm] sync_ecu_data action: config_loading_from_ecu — stop poll/composite");
            state.session.knock_scope().stop();
            state.session.knock_scope().disable_on_ecu(&state.session);
            state.session.output().stop();
            state.session.composite().disable_on_ecu(&state.session);
            state.session.composite().stop();
            emit_composite(app, &state.session.composite().snapshot());
            emit_knock_scope_reset(app);
        }
        WorkspacePhase::ConfigFromEcu => {
            println!("[workspace-fsm] sync_ecu_data action: config_from_ecu — start output poll");
            sync_output_poll_session(&state.session, app);
        }
    }

    emit_config_update(app, &state.session.config().snapshot());
    emit_current_output(app, state);
    emit_composite(app, &state.session.composite().snapshot());
    emit_knock_scope_reset(app);

    if state.session.take_output_poll_resync() {
        sync_output_poll_session(&state.session, app);
    }
}

fn protocol_log_emit_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn register_config_burn_notify(app: &AppHandle) {
    let handle = app.clone();
    let state = app.state::<RuntimeState>();
    state.session.config().set_live_ram_dirty_hook(move || {
        if let Some(state) = handle.try_state::<RuntimeState>() {
            set_burn_pending(&state, &handle, true);
        }
    });
}

pub fn register_panels_emitter(app: &AppHandle) {
    let handle = app.clone();
    let state = app.state::<RuntimeState>();
    state.session.set_panels_changed_hook(Arc::new(move |status| {
        let _ = handle.emit("ini-panels-ready", status);
    }));
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelsManifestResponse {
    pub source: String,
    pub hash: Option<String>,
    pub manifest: Option<PanelManifest>,
}

fn active_panel_cache_dir(session: &EcuSession) -> Result<std::path::PathBuf, String> {
    let hash = session
        .active_panel_hash()
        .ok_or_else(|| "INI panel cache не готов — откройте проект и дождитесь загрузки INI".to_string())?;
    Ok(session.panels_root().join(hash))
}

#[tauri::command]
pub fn panels_get_manifest(state: State<RuntimeState>) -> Result<PanelsManifestResponse, String> {
    let session = &state.session;
    if let Err(e) = session.ensure_ui_panels() {
        session.log_panel_cache_error("panels_get_manifest", e);
    }
    let cache_dir = match active_panel_cache_dir(session) {
        Ok(dir) => dir,
        Err(_) => {
            return Ok(PanelsManifestResponse {
                source: "unavailable".into(),
                hash: None,
                manifest: None,
            });
        }
    };
    let manifest = read_manifest_from_dir(&cache_dir)?;
    Ok(PanelsManifestResponse {
        source: "cache".into(),
        hash: session.active_panel_hash(),
        manifest: Some(manifest),
    })
}

#[tauri::command]
pub fn panels_read_yaml(file: String, state: State<RuntimeState>) -> Result<String, String> {
    let session = &state.session;
    if let Err(e) = session.ensure_ui_panels() {
        session.log_panel_cache_error("panels_read_yaml", e);
    }
    let cache_dir = active_panel_cache_dir(session)?;
    read_panel_yaml(&cache_dir, &file)
}

/// Читает статический UI-конфиг из бандла (обход Windows WebView2 SPA-fallback для YAML).
#[tauri::command]
pub fn read_ui_config(app: AppHandle, path: String) -> Result<String, String> {
    let key = format!("/config/{}", path.trim_start_matches('/'));
    match app.asset_resolver().get(key.clone()) {
        Some(asset) => String::from_utf8(asset.bytes).map_err(|e| e.to_string()),
        None => Err(format!("UI config not found in bundle: {key}")),
    }
}

/// Читает UI-конфиг из папки проекта.
#[tauri::command]
pub fn project_read_ui_config(
    path: String,
    state: State<RuntimeState>,
) -> Result<String, String> {
    let dir = state
        .project
        .lock()
        .unwrap()
        .project_dir()
        .ok_or_else(|| "no project open".to_string())?;
    let config_path = dir.join("config").join(path.trim_start_matches('/'));
    std::fs::read_to_string(&config_path).map_err(|e| format!("{path}: {e}"))
}

pub fn register_knock_scope_emitter(app: &AppHandle) {
    let handle = app.clone();
    let state = app.state::<RuntimeState>();
    state.session.knock_scope().set_tick_hook(move |snap, ui| {
        if let Some(state) = handle.try_state::<RuntimeState>() {
            // try_lock: poll-поток не блокируется, если UI держит lock в stop_run (join иначе зависает).
            if let Ok(mut rt) = state.runtime.try_lock() {
                for (instance_id, st) in rt.feed_knock_scope(&snap) {
                    emit_state(&handle, &instance_id, &st);
                }
            }
        }
        emit_knock_scope_tick(&handle, &ui);
    });
    let handle = app.clone();
    let session = Arc::clone(&state.session);
    state.session.knock_scope().set_stop_hook(move || {
        sync_output_poll_session(&session, &handle);
    });
}

pub fn register_protocol_log_emitter(app: &AppHandle) {
    let handle = app.clone();
    let state = app.state::<RuntimeState>();
    state.protocol_log.add_listener(Arc::new(move |entry| {
        let immediate = entry.direction == "link" || entry.direction == "err";
        if !immediate {
            let now = protocol_log_emit_now_ms();
            let prev = LAST_PROTOCOL_LOG_UI_EMIT_MS.load(Ordering::Relaxed);
            if now.saturating_sub(prev) < PROTOCOL_LOG_UI_THROTTLE_MS {
                return;
            }
            LAST_PROTOCOL_LOG_UI_EMIT_MS.store(now, Ordering::Relaxed);
        }
        let app = handle.clone();
        let entry = entry.clone();
        tauri::async_runtime::spawn(async move {
            emit_protocol_log(&app, &entry);
        });
    }));
}

#[tauri::command]
pub fn component_list_logic_types(state: State<RuntimeState>) -> Vec<String> {
    state
        .runtime
        .lock()
        .map(|r| r.list_logic_types().into_iter().map(str::to_string).collect())
        .unwrap_or_default()
}

#[tauri::command]
pub fn component_mount(
    params: MountParams,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<Value, String> {
    let mut rt = state.runtime.lock().map_err(|e| e.to_string())?;
    let snapshot = rt.mount(
        &params.instance_id,
        &params.component_type,
        params.payload,
    )?;
    let sync = rt.ecu_sync_on_mount(&params.instance_id);
    emit_state(&app, &params.instance_id, &snapshot);
    apply_ecu_sync_on_mount(sync, &state, &app);
    Ok(snapshot)
}

fn apply_ecu_sync_on_mount(policy: EcuSyncOnMount, state: &RuntimeState, app: &AppHandle) {
    match policy {
        EcuSyncOnMount::Full => sync_ecu_data(state, app),
        EcuSyncOnMount::OutputPollIfConfigLoaded => {
            if state.session.config().snapshot().loaded {
                sync_output_poll_session(&state.session, app);
            }
        }
        EcuSyncOnMount::None => {}
    }
}

#[tauri::command]
pub fn component_get_state(
    instance_id: String,
    state: State<RuntimeState>,
) -> Result<Value, String> {
    state
        .runtime
        .lock()
        .map_err(|e| e.to_string())?
        .state(&instance_id)
}

#[tauri::command]
pub fn component_dispatch(
    params: DispatchParams,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<Value, String> {
    let action = params.action.as_str();
    if matches!(action, "start" | "stop") {
        return component_dispatch_simulation(params, state, app);
    }
    if matches!(action, "send" | "run_quick") {
        return component_dispatch_command(params, state, app);
    }
    if action == "run" {
        let is_ini_command = {
            let rt = state.runtime.lock().map_err(|e| e.to_string())?;
            rt.instance_component_type(&params.instance_id)
                .map(|t| t == "ini-command-button")
                .unwrap_or(false)
        };
        if is_ini_command {
            return component_dispatch_ini_command(params, state, app);
        }
    }

    component_dispatch_inner(params, state, app)
}

fn component_dispatch_simulation(
    params: DispatchParams,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<Value, String> {
    let action = params.action;
    let instance_id = params.instance_id;

    let rpm = if action == "start" {
        let rt = state.runtime.lock().map_err(|e| e.to_string())?;
        let s = rt.state(&instance_id)?;
        s.get("rpm")
            .and_then(|v| v.as_u64())
            .unwrap_or(1500) as u16
    } else {
        0
    };

    let snapshot = {
        let mut rt = state.runtime.lock().map_err(|e| e.to_string())?;
        rt.dispatch(&instance_id, &format!("begin_{action}"), Value::Null)?
    };
    emit_state(&app, &instance_id, &snapshot);

    let session = Arc::clone(&state.session);
    let app = app.clone();
    let action_owned = action.clone();
    let instance_id_bg = instance_id.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let ecu_result = match action_owned.as_str() {
            "start" => session.run_stimulator_start(rpm),
            "stop" => session.run_stimulator_stop(),
            _ => unreachable!(),
        };

        let ok = ecu_result.is_ok();
        let finish_payload = serde_json::json!({
            "ok": ok,
            "error": ecu_result.err(),
        });

        let state = app.state::<RuntimeState>();
        let finish_action = format!("finish_{action_owned}");
        let dispatch_result = (|| -> Result<(), String> {
            let mut rt = state.runtime.lock().map_err(|e| e.to_string())?;
            let snap = rt.dispatch(&instance_id_bg, &finish_action, finish_payload)?;
            emit_state(&app, &instance_id_bg, &snap);
            if ok {
                sync_output_poll_session(&state.session, &app);
            }
            Ok(())
        })();

        if dispatch_result.is_err() {
            session.set_stimulation_active(false);
        }
    });

    Ok(snapshot)
}

fn component_dispatch_ini_command(
    params: DispatchParams,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<Value, String> {
    let instance_id = params.instance_id;

    let snapshot = {
        let mut rt = state.runtime.lock().map_err(|e| e.to_string())?;
        rt.dispatch(&instance_id, "begin_run", Value::Null)?
    };
    let command_key = snapshot
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if command_key.is_empty() {
        return Err("Не задана INI-команда".into());
    }
    emit_state(&app, &instance_id, &snapshot);

    let session = Arc::clone(&state.session);
    let app = app.clone();
    let instance_id_bg = instance_id.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let ecu_result = session.run_ts_ini_command(&command_key);
        let ok = ecu_result.is_ok();
        let finish_payload = serde_json::json!({
            "ok": ok,
            "error": ecu_result.err(),
        });

        let state = app.state::<RuntimeState>();
        let dispatch_result = (|| -> Result<(), String> {
            let mut rt = state.runtime.lock().map_err(|e| e.to_string())?;
            let snap = rt.dispatch(&instance_id_bg, "finish_run", finish_payload)?;
            emit_state(&app, &instance_id_bg, &snap);
            if ok {
                sync_output_poll_session(&state.session, &app);
            }
            Ok(())
        })();

        if dispatch_result.is_err() {
            eprintln!("ini-command finish_run failed: {:?}", dispatch_result);
        }
    });

    Ok(snapshot)
}

fn component_dispatch_command(
    params: DispatchParams,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<Value, String> {
    let instance_id = params.instance_id;
    let send_payload = params.payload;

    let snapshot = {
        let mut rt = state.runtime.lock().map_err(|e| e.to_string())?;
        rt.dispatch(&instance_id, "begin_send", send_payload)?
    };
    let text = snapshot
        .get("pendingText")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let exchange_id = snapshot
        .get("pendingExchangeId")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if text.is_empty() {
        return Err("Не удалось определить текст команды".into());
    }
    emit_state(&app, &instance_id, &snapshot);

    let session = Arc::clone(&state.session);
    let app = app.clone();
    let instance_id_bg = instance_id.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let ecu_result = session.run_console_command(&text);
        let ok = ecu_result.is_ok();
        let finish_payload = serde_json::json!({
            "ok": ok,
            "error": ecu_result.as_ref().err().map(String::as_str),
            "response": ecu_result.as_ref().ok().map(String::as_str),
            "text": text,
            "exchangeId": exchange_id,
        });

        let state = app.state::<RuntimeState>();
        let dispatch_result = (|| -> Result<(), String> {
            let mut rt = state.runtime.lock().map_err(|e| e.to_string())?;
            let snap = rt.dispatch(&instance_id_bg, "finish_send", finish_payload)?;
            emit_state(&app, &instance_id_bg, &snap);
            sync_output_poll_session(&state.session, &app);
            Ok(())
        })();

        if dispatch_result.is_err() {
            eprintln!("command finish_send failed: {:?}", dispatch_result);
        }
    });

    Ok(snapshot)
}

fn component_dispatch_inner(
    params: DispatchParams,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<Value, String> {
    let action = params.action.as_str();
    let stops_output_poll = matches!(action, "connect" | "disconnect") && state.session.is_connected();

    if stops_output_poll {
        state.session.output().stop();
    }

    let manual_disconnect_port = if action == "disconnect" {
        state
            .session
            .connection_info_if_available()
            .map(|info| info.port_name)
    } else {
        None
    };

    let may_write_config = component_action_may_write_config(action, &params.payload);

    let (snapshot, peer_ignition_states) = {
        let mut rt = state.runtime.lock().map_err(|e| e.to_string())?;
        let snapshot = rt.dispatch(&params.instance_id, action, params.payload)?;

        if action == "connect" {
            state.autoconnect.clear_manual_disconnect();
        } else if action == "disconnect" {
            if let Some(port) = manual_disconnect_port {
                state.autoconnect.note_manual_disconnect(Some(&port));
            }
        }

        let peers = if matches!(action, "set_params" | "replace_params") {
            rt.peer_ignition_table_states(&params.instance_id)
        } else {
            Vec::new()
        };

        (snapshot, peers)
    };

    emit_state(&app, &params.instance_id, &snapshot);
    for (peer_id, peer_state) in peer_ignition_states {
        emit_state(&app, &peer_id, &peer_state);
    }

    if may_write_config {
        emit_config_update(&app, &state.session.config().snapshot());
        persist_config_after_table_edit(&state, &app);
    }

    if matches!(action, "connect" | "disconnect") {
        schedule_ecu_notify(&app, action == "connect");
    }

    Ok(snapshot)
}

fn config_table_action_may_write(action: &str, payload: &Value) -> bool {
    match action {
        "interpolate" | "commit_cell" | "set_selection_value" | "type_key" | "paste" => true,
        "keydown" => {
            let ctrl = payload.get("ctrl").and_then(|v| v.as_bool()).unwrap_or(false);
            let key = payload.get("key").and_then(|v| v.as_str()).unwrap_or("");
            ctrl && matches!(
                key,
                "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight"
            )
        }
        _ => false,
    }
}

fn component_action_may_write_config(action: &str, payload: &Value) -> bool {
    if config_table_action_may_write(action, payload) {
        return true;
    }
    match action {
        "generate_map" => true,
        "stop_run" => payload
            .get("applyThreshold")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        "apply_frequency" => true,
        _ => false,
    }
}

fn persist_config_after_table_edit(state: &RuntimeState, app: &AppHandle) {
    let snap = state.session.config().snapshot();
    let wrote_live = state.session.is_connected() && snap.loaded && !snap.read_only;
    if snap.loaded && snap.read_only {
        if state
            .project
            .lock()
            .unwrap()
            .sync_ecu_config_from_session(&state.session)
            .is_ok()
            && state.project.lock().unwrap().info().dirty
        {
            emit_project(app, state);
        }
    }
    if wrote_live {
        set_burn_pending(state, app, true);
    }
}

#[tauri::command]
pub fn stimulator_set_rpm(rpm: u16, state: State<RuntimeState>) -> Result<(), String> {
    state.session.run_stimulator_set_rpm(rpm)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StimulatorRampStartParams {
    idle_rpm: u16,
    peak_rpm: u16,
    ramp_up_sec: f32,
    ramp_down_sec: f32,
    curve: RampCurveKind,
    step_ms: Option<u64>,
    rpm_min: u16,
    rpm_max: u16,
}

#[tauri::command]
pub fn stimulator_ramp_start(
    params: StimulatorRampStartParams,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    let ramp_params = StimulatorRampParams {
        idle_rpm: params.idle_rpm,
        peak_rpm: params.peak_rpm,
        ramp_up_sec: params.ramp_up_sec,
        ramp_down_sec: params.ramp_down_sec,
        curve: params.curve,
        step_ms: params.step_ms.unwrap_or(DEFAULT_RAMP_STEP_MS),
        rpm_min: params.rpm_min,
        rpm_max: params.rpm_max,
    };

    let session = Arc::clone(&state.session);
    let app_step = app.clone();
    let app_done = app.clone();

    state.session.stimulator_ramp().start(
        session,
        ramp_params,
        move |step: StimulatorRampStep| {
            let _ = app_step.emit("stimulator-ramp-step", step);
        },
        move |result: StimulatorRampResult| {
            let _ = app_done.emit("stimulator-ramp-finished", result);
        },
    )
}

#[tauri::command]
pub fn stimulator_ramp_cancel(state: State<RuntimeState>) {
    state.session.stimulator_ramp().cancel();
}

#[tauri::command]
pub fn ecu_resync(_state: State<RuntimeState>, app: AppHandle) {
    schedule_ecu_notify(&app, true);
}

#[tauri::command]
pub fn component_unmount(instance_id: String, state: State<RuntimeState>) {
    if let Ok(mut rt) = state.runtime.lock() {
        rt.unmount(&instance_id);
    }
}

#[tauri::command]
pub fn output_get_snapshot(state: State<RuntimeState>) -> OutputSnapshot {
    state.session.current_output_snapshot()
}

#[tauri::command]
pub fn output_list_fields(state: State<RuntimeState>) -> Vec<OutputFieldInfo> {
    state.session.ini_context().list_output_fields()
}

#[tauri::command]
pub fn output_start_listener(_state: State<RuntimeState>, app: AppHandle) {
    let app_bg = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app_bg.state::<RuntimeState>();
        emit_current_output(&app_bg, &state);
        sync_ecu_data(&state, &app_bg);
    });
}

#[tauri::command]
pub fn composite_get_snapshot(state: State<RuntimeState>) -> CompositeSnapshot {
    state.session.composite().snapshot()
}

#[tauri::command]
pub fn composite_set_enabled(
    enabled: bool,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<CompositeSnapshot, String> {
    if enabled {
        if !state.session.is_connected() {
            return Err("ECU не подключена".into());
        }
        if state.session.config().snapshot().loading {
            return Err("Дождитесь окончания загрузки config".into());
        }
        let log = state.session.open_composite_log()?;
        let app_emit = app.clone();
        let session = Arc::clone(&state.session);
        state.session.composite().start(session, Some(log), move |snap| {
            emit_composite(&app_emit, &snap);
        })?;
        emit_composite_timeline(&app, &state.session.composite_timeline_status());
        emit_knock_scope_reset(&app);
    } else {
        state.session.composite().disable_on_ecu(&state.session);
        state.session.composite().stop();
        if let Ok(Some(path)) = state.session.close_composite_log() {
            state
                .project
                .lock()
                .unwrap()
                .add_log(
                    std::path::Path::new(&path),
                    None,
                    Some("composite_csv"),
                );
            emit_project(&app, &state);
        }
        emit_composite(&app, &state.session.composite().snapshot());
        if state.session.log_viewport_linked() {
            state.session.sync_composite_viewport_from_output();
        }
        emit_composite_timeline(&app, &state.session.composite_timeline_status());
    }
    Ok(state.session.composite().snapshot())
}

#[tauri::command]
pub fn composite_compute_trigger_wheels(params: ComputeTriggerWheelsParams) -> TriggerWheelsView {
    compute_trigger_wheels(&params)
}

#[tauri::command]
pub fn composite_timeline_session_events(state: State<RuntimeState>) -> Vec<CompositeEventJson> {
    state.session.composite_timeline_session_events()
}

#[tauri::command]
pub fn composite_set_max_window_ms(max_window_ms: f64, state: State<RuntimeState>) {
    state.session.composite().set_max_window_ms(max_window_ms);
}

#[tauri::command]
pub fn knock_scope_get_snapshot(state: State<RuntimeState>) -> KnockScopeSnapshot {
    state.session.knock_scope_snapshot()
}

#[tauri::command]
pub fn knock_scope_gpu_buffer(state: State<RuntimeState>) -> String {
    state.session.knock_scope().spectrogram_gpu_buffer_b64()
}

#[tauri::command]
pub fn knock_scope_set_enabled(
    enabled: bool,
    window_ms: Option<u32>,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<KnockScopeSnapshot, String> {
    if enabled {
        if !state.session.is_connected() {
            return Err("ECU не подключена".into());
        }
        if state.session.config().snapshot().loading {
            return Err("Дождитесь окончания загрузки config".into());
        }
        let session = Arc::clone(&state.session);
        let window_ms = window_ms.unwrap_or(500);
        state.session.knock_scope().start(session, window_ms, |_| {})?;
        emit_composite(&app, &state.session.composite().snapshot());
        emit_composite_timeline(&app, &state.session.composite_timeline_status());
    } else {
        state.session.knock_scope().stop_recording(&state.session);
        sync_output_poll_session(&state.session, &app);
    }
    Ok(state.session.knock_scope_snapshot())
}

#[tauri::command]
pub fn knock_scope_pan_spectrogram(
    delta_columns: i32,
    state: State<RuntimeState>,
    app: AppHandle,
) -> KnockScopeSnapshot {
    state
        .session
        .knock_scope()
        .pan_spectrogram_view(delta_columns);
    emit_knock_scope_tick(
        &app,
        &state.session.knock_scope().viewport_refresh_ui_tick(),
    );
    state.session.knock_scope_snapshot()
}

#[tauri::command]
pub fn knock_scope_set_spectrogram_follow_live(
    follow: bool,
    state: State<RuntimeState>,
    app: AppHandle,
) -> KnockScopeSnapshot {
    state
        .session
        .knock_scope()
        .set_spectrogram_follow_live(follow);
    emit_knock_scope_tick(
        &app,
        &state.session.knock_scope().viewport_refresh_ui_tick(),
    );
    state.session.knock_scope_snapshot()
}

#[tauri::command]
pub fn composite_timeline_status(state: State<RuntimeState>) -> CompositeTimelineStatus {
    state.session.composite_timeline_status()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositeTimelineQueryParams {
    pub pixel_width: u32,
    pub view_end_sec: Option<f64>,
    pub span_sec: Option<f64>,
}

#[tauri::command]
pub fn composite_timeline_query_view(
    state: State<RuntimeState>,
    params: CompositeTimelineQueryParams,
) -> CompositeTimelineView {
    state.session.composite_timeline_query(CompositeTimelineViewQuery {
        pixel_width: params.pixel_width,
        view_end_sec: params.view_end_sec,
        span_sec: params.span_sec,
    })
}

#[tauri::command]
pub fn composite_timeline_set_view(
    state: State<RuntimeState>,
    params: OutputTimelineControlParams,
    app: AppHandle,
) -> CompositeTimelineStatus {
    let st = state.session.composite_timeline_control(params.ctrl);
    emit_composite_timeline(&app, &st);
    if state.session.log_viewport_linked() {
        let _ = app.emit(
            "output-timeline-status",
            state.session.output_timeline_status(),
        );
    }
    st
}

#[tauri::command]
pub fn composite_timeline_load_file(
    state: State<RuntimeState>,
    path: String,
    app: AppHandle,
) -> Result<CompositeTimelineStatus, String> {
    let st = state
        .session
        .composite_timeline_load_file(std::path::PathBuf::from(path))?;
    emit_composite_timeline(&app, &st);
    Ok(st)
}

#[tauri::command]
pub fn log_viewport_set_linked(
    linked: bool,
    state: State<RuntimeState>,
    app: AppHandle,
) -> bool {
    state.session.set_log_viewport_linked(linked);
    if linked {
        state.session.sync_composite_viewport_from_output();
        emit_composite_timeline(&app, &state.session.composite_timeline_status());
        let _ = app.emit(
            "output-timeline-status",
            state.session.output_timeline_status(),
        );
    }
    linked
}

#[tauri::command]
pub fn log_viewport_get_linked(state: State<RuntimeState>) -> bool {
    state.session.log_viewport_linked()
}

/// Нативный диалог: trigger/composite CSV.
#[tauri::command]
pub async fn pick_composite_log_path() -> Option<String> {
    let handle = rfd::AsyncFileDialog::new()
        .set_title("Открыть trigger / composite log")
        .add_filter("CSV log", &["csv"])
        .pick_file()
        .await?;
    Some(handle.path().display().to_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTimelineQueryParams {
    pub fields: Vec<String>,
    pub pixel_width: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTimelineControlParams {
    pub ctrl: OutputTimelineViewControl,
}

#[tauri::command]
pub fn output_timeline_status(state: State<RuntimeState>) -> OutputTimelineStatus {
    state.session.output_timeline_status()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTimelineSeriesParams {
    pub fields: Vec<String>,
    pub max_points_per_field: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTimelineChunkParams {
    pub fields: Vec<String>,
    pub max_rows: Option<usize>,
    pub max_points_per_field: Option<usize>,
    pub reset_stream: Option<bool>,
}

#[tauri::command]
pub fn output_timeline_pull_series_chunk(
    state: State<RuntimeState>,
    params: OutputTimelineChunkParams,
) -> OutputTimelineSeriesChunk {
    state.session.output_timeline_pull_series_chunk(OutputTimelineChunkQuery {
        fields: params.fields,
        max_rows: params
            .max_rows
            .unwrap_or(rusefui_runtime::FILE_CHUNK_ROWS_DEFAULT),
        max_points_per_field: params
            .max_points_per_field
            .unwrap_or(rusefui_runtime::SERIES_CHUNK_MAX_POINTS),
        reset_stream: params.reset_stream.unwrap_or(false),
    })
}

#[tauri::command]
pub fn output_timeline_series_snapshot(
    state: State<RuntimeState>,
    params: OutputTimelineSeriesParams,
) -> OutputTimelineSeriesSnapshot {
    state.session.output_timeline_series_snapshot(OutputTimelineSeriesQuery {
        fields: params.fields,
        max_points_per_field: params
            .max_points_per_field
            .unwrap_or(rusefui_runtime::SERIES_SNAPSHOT_MAX_POINTS),
    })
}

#[tauri::command]
pub fn output_timeline_query_view(
    state: State<RuntimeState>,
    params: OutputTimelineQueryParams,
) -> OutputTimelineView {
    state.session.output_timeline_query(rusefui_runtime::OutputTimelineViewQuery {
        fields: params.fields,
        pixel_width: params.pixel_width,
    })
}

#[tauri::command]
pub fn output_timeline_set_view(
    state: State<RuntimeState>,
    params: OutputTimelineControlParams,
    app: AppHandle,
) -> OutputTimelineStatus {
    let st = state.session.output_timeline_control(params.ctrl);
    let _ = app.emit("output-timeline-status", &st);
    if state.session.log_viewport_linked() {
        emit_composite_timeline(&app, &state.session.composite_timeline_status());
    }
    if !state.session.is_connected() {
        emit_current_output(&app, &state);
    }
    st
}

#[tauri::command]
pub fn output_set_log_cursor(
    sec: Option<f64>,
    state: State<RuntimeState>,
    app: AppHandle,
) {
    if state.session.set_output_log_cursor_sec(sec) && !state.session.is_connected() {
        emit_current_output(&app, &state);
    }
}

#[tauri::command]
pub fn output_timeline_load_file(
    state: State<RuntimeState>,
    path: String,
    app: AppHandle,
) -> Result<OutputTimelineStatus, String> {
    let st = state
        .session
        .output_timeline_load_file(std::path::PathBuf::from(path));
    let _ = app.emit("output-timeline-status", &st);
    emit_current_output(&app, &state);
    Ok(st)
}

/// Нативный диалог выбора CSV-лога output channels.
#[tauri::command]
pub async fn pick_output_log_path() -> Option<String> {
    let handle = rfd::AsyncFileDialog::new()
        .set_title("Открыть log output channels")
        .add_filter("CSV log", &["csv"])
        .pick_file()
        .await?;
    Some(handle.path().display().to_string())
}

#[tauri::command]
pub fn config_get_snapshot(state: State<RuntimeState>, app: AppHandle) -> ConfigSnapshot {
    config_snapshot_for_ui(&app, &state)
}

#[tauri::command]
pub fn checklist_load_rules(
    yaml: String,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    let rules = ChecklistRules::parse_yaml(&yaml)?;
    *state.checklist_rules.lock().unwrap() = Some(rules);
    emit_config_update(&app, &state.session.config().snapshot());
    Ok(())
}

#[tauri::command]
pub fn config_list_fields(state: State<RuntimeState>) -> Vec<ConfigFieldInfo> {
    state.session.config().list_fields()
}

#[tauri::command]
pub fn config_start_listener(state: State<RuntimeState>, app: AppHandle) {
    emit_config(&app, state.session.config().snapshot());
    let app_bg = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app_bg.state::<RuntimeState>();
        sync_ecu_data(&state, &app_bg);
    });
}

#[tauri::command]
pub fn config_diff_get(state: State<RuntimeState>) -> ConfigDiffSnapshot {
    state.config_diff.lock().unwrap().snapshot()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDiffChoiceParams {
    pub field: String,
    pub side: String,
}

#[tauri::command]
pub fn config_diff_set_choice(
    params: ConfigDiffChoiceParams,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<ConfigDiffSnapshot, String> {
    let side = match params.side.as_str() {
        "project" => DiffSide::Project,
        "ecu" => DiffSide::Ecu,
        _ => return Err(format!("сторона должна быть project или ecu, получено {:?}", params.side)),
    };
    let mut diff = state.config_diff.lock().unwrap();
    diff.set_choice(&params.field, side)?;
    let snap = diff.snapshot();
    drop(diff);
    emit_config_diff(&app, &snap);
    Ok(snap)
}

#[tauri::command]
pub fn config_diff_set_all(
    side: String,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<ConfigDiffSnapshot, String> {
    let side = match side.as_str() {
        "project" => DiffSide::Project,
        "ecu" => DiffSide::Ecu,
        _ => return Err("сторона: project | ecu".into()),
    };
    let mut diff = state.config_diff.lock().unwrap();
    diff.set_all_choices(side);
    let snap = diff.snapshot();
    drop(diff);
    emit_config_diff(&app, &snap);
    Ok(snap)
}

#[tauri::command]
pub fn config_diff_apply(state: State<RuntimeState>, app: AppHandle) -> Result<(), String> {
    let plan: Vec<(String, DiffSide, f64)> = {
        let diff = state.config_diff.lock().unwrap();
        if !diff.snapshot().active {
            return Err("Нет активного сравнения config".into());
        }
        diff.snapshot()
            .entries
            .iter()
            .map(|e| {
                let side = diff.choice_for(&e.field).unwrap_or(DiffSide::Ecu);
                let value = match side {
                    DiffSide::Project => e.project,
                    DiffSide::Ecu => e.ecu,
                };
                (e.field.clone(), side, value)
            })
            .collect()
    };

    state.session.output().stop();
    let session = Arc::clone(&state.session);
    let mut wrote_to_ecu_ram = false;
    for (field, side, value) in plan {
        match side {
            DiffSide::Project => {
                session.config().write_scalar(&session, &field, value)?;
                wrote_to_ecu_ram = true;
            }
            DiffSide::Ecu => {
                state
                    .project
                    .lock()
                    .unwrap()
                    .patch_ecu_config_field(&session, &field, value)?;
            }
        }
    }

    if wrote_to_ecu_ram {
        set_burn_pending(&state, &app, true);
    }

    clear_config_diff(&state, &app);
    emit_config_update(&app, &state.session.config().snapshot());
    emit_project(&app, &state);
    if state.session.should_poll_output_channels() {
        sync_output_poll_session(&state.session, &app);
    }
    Ok(())
}

#[tauri::command]
pub fn config_diff_dismiss(state: State<RuntimeState>, app: AppHandle) {
    clear_config_diff(&state, &app);
}

#[derive(Debug, Deserialize)]
pub struct ConfigSetScalarParams {
    pub field: String,
    pub value: f64,
}

#[derive(Debug, Deserialize)]
pub struct ConfigSetStringParams {
    pub field: String,
    pub value: String,
}

/// Сессия потеряла project-preview (например, после гонки с загрузкой ECU) — восстановить из файла.
fn try_apply_project_config_for_edit(state: &RuntimeState) -> Result<bool, String> {
    let ws = workspace_inputs(state).derive();
    if !ws.capabilities.edit_project_config {
        return Ok(false);
    }
    let store = state.project.lock().unwrap();
    if store.info().path.is_none() {
        return Ok(false);
    }
    drop(store);
    state.project.lock().unwrap().apply_to_session(&state.session)?;
    let snap = state.session.config().snapshot();
    Ok(snap.loaded && snap.read_only)
}

fn write_config_scalar(
    state: &RuntimeState,
    field: &str,
    value: f64,
) -> Result<(ConfigSnapshot, bool), String> {
    let snap = state.session.config().snapshot();
    let wrote_live = state.session.is_connected() && snap.loaded && !snap.read_only;
    if wrote_live {
        state.session.output().stop();
        let session = Arc::clone(&state.session);
        session.config().write_scalar(&session, field, value)?;
    } else if snap.loaded && snap.read_only {
        state
            .session
            .config()
            .set_scalar_local(field, value)?;
        state
            .project
            .lock()
            .unwrap()
            .sync_ecu_config_from_session(&state.session)?;
    } else if try_apply_project_config_for_edit(state)? {
        state
            .session
            .config()
            .set_scalar_local(field, value)?;
        state
            .project
            .lock()
            .unwrap()
            .sync_ecu_config_from_session(&state.session)?;
    } else {
        return Err(
            "Нет config для редактирования — откройте проект с ecuConfig или подключите ECU"
                .into(),
        );
    }
    Ok((state.session.config().snapshot(), wrote_live))
}

fn write_config_string(
    state: &RuntimeState,
    field: &str,
    value: &str,
) -> Result<(ConfigSnapshot, bool), String> {
    let snap = state.session.config().snapshot();
    let wrote_live = state.session.is_connected() && snap.loaded && !snap.read_only;
    if wrote_live {
        state.session.output().stop();
        let session = Arc::clone(&state.session);
        session.config().write_string(&session, field, value)?;
    } else if snap.loaded && snap.read_only {
        state
            .session
            .config()
            .set_string_local(field, value)?;
        state
            .project
            .lock()
            .unwrap()
            .sync_ecu_config_from_session(&state.session)?;
    } else if try_apply_project_config_for_edit(state)? {
        state
            .session
            .config()
            .set_string_local(field, value)?;
        state
            .project
            .lock()
            .unwrap()
            .sync_ecu_config_from_session(&state.session)?;
    } else {
        return Err(
            "Нет config для редактирования — откройте проект с ecuConfig или подключите ECU"
                .into(),
        );
    }
    Ok((state.session.config().snapshot(), wrote_live))
}

#[tauri::command]
pub fn config_set_scalar(
    params: ConfigSetScalarParams,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<ConfigSnapshot, String> {
    let (snap, wrote_live) = write_config_scalar(&state, &params.field, params.value)?;
    emit_config_update(&app, &snap);
    if state.project.lock().unwrap().info().dirty {
        emit_project(&app, &state);
    }
    if state.session.should_poll_output_channels() {
        sync_output_poll_session(&state.session, &app);
    }
    if wrote_live {
        set_burn_pending(&state, &app, true);
    }
    Ok(enrich_config_snapshot(&app, snap))
}

#[tauri::command]
pub fn config_set_string(
    params: ConfigSetStringParams,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<ConfigSnapshot, String> {
    let (snap, wrote_live) = write_config_string(&state, &params.field, &params.value)?;
    emit_config_update(&app, &snap);
    if state.project.lock().unwrap().info().dirty {
        emit_project(&app, &state);
    }
    if state.session.should_poll_output_channels() {
        sync_output_poll_session(&state.session, &app);
    }
    if wrote_live {
        set_burn_pending(&state, &app, true);
    }
    Ok(enrich_config_snapshot(&app, snap))
}

#[tauri::command]
pub async fn config_burn(state: State<'_, RuntimeState>, app: AppHandle) -> Result<(), String> {
    if !state.session.is_connected() {
        return Err("ECU не подключена".into());
    }

    state.session.output().stop();
    state.session.composite().disable_on_ecu(&state.session);
    state.session.composite().stop();

    let session = Arc::clone(&state.session);

    // Блокирующий serial I/O переносим в пул потоков — IPC-поток Tauri
    // остаётся свободным, WebView продолжает рендериться.
    tokio::task::spawn_blocking(move || session.config().burn_to_flash(&session))
        .await
        .map_err(|e| e.to_string())??;

    set_burn_pending(&state, &app, false);

    // Обновление снимков config/output — в фоне, не задерживаем ответ фронту.
    let session = Arc::clone(&state.session);
    let app_bg = app.clone();
    tauri::async_runtime::spawn(async move {
        let snap = session.config().snapshot();
        emit_config_update(&app_bg, &snap);
        if session.should_poll_output_channels() {
            sync_output_poll_session(&session, &app_bg);
        } else {
            let state_bg = app_bg.state::<RuntimeState>();
            emit_current_output(&app_bg, &state_bg);
        }
    });

    Ok(())
}

#[tauri::command]
pub fn app_force_quit(app: AppHandle) {
    app.exit(0);
}

#[derive(Debug, Deserialize)]
pub struct ConfigArrayFieldParams {
    pub field: String,
}

#[tauri::command]
pub fn config_get_array(
    params: ConfigArrayFieldParams,
    state: State<RuntimeState>,
) -> Result<Vec<f64>, String> {
    state.session.config().get_array(&params.field)
}

#[derive(Debug, Deserialize)]
pub struct ConfigSetArrayValueParams {
    pub field: String,
    pub index: usize,
    pub value: f64,
}

#[tauri::command]
pub fn config_set_array_value(
    params: ConfigSetArrayValueParams,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<ConfigSnapshot, String> {
    let snap = state.session.config().snapshot();
    let wrote_live = state.session.is_connected() && snap.loaded && !snap.read_only;
    if wrote_live {
        state.session.config().write_array_value(
            &state.session,
            &params.field,
            params.index,
            params.value,
        )?;
    } else if snap.loaded && snap.read_only {
        state.session.config().set_array_value_local(
            &params.field,
            params.index,
            params.value,
        )?;
        state
            .project
            .lock()
            .unwrap()
            .sync_ecu_config_from_session(&state.session)?;
    } else if try_apply_project_config_for_edit(&state)? {
        state.session.config().set_array_value_local(
            &params.field,
            params.index,
            params.value,
        )?;
        state
            .project
            .lock()
            .unwrap()
            .sync_ecu_config_from_session(&state.session)?;
    } else {
        return Err(
            "Нет config для редактирования — откройте проект с ecuConfig или подключите ECU".into(),
        );
    }

    let snap = state.session.config().snapshot();
    emit_config_update(&app, &snap);
    if state.project.lock().unwrap().info().dirty {
        emit_project(&app, &state);
    }
    if wrote_live {
        set_burn_pending(&state, &app, true);
    }
    Ok(enrich_config_snapshot(&app, snap))
}

#[derive(Debug, Deserialize)]
pub struct ConfigArrayValueUpdate {
    pub index: usize,
    pub value: f64,
}

#[derive(Debug, Deserialize)]
pub struct ConfigSetArrayValuesParams {
    pub field: String,
    pub updates: Vec<ConfigArrayValueUpdate>,
}

#[tauri::command]
pub fn config_set_array_values(
    params: ConfigSetArrayValuesParams,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<ConfigSnapshot, String> {
    if params.updates.is_empty() {
        return Ok(config_snapshot_for_ui(&app, &state));
    }

    let pairs: Vec<(usize, f64)> = params
        .updates
        .iter()
        .map(|u| (u.index, u.value))
        .collect();

    let snap = state.session.config().snapshot();
    let wrote_live = state.session.is_connected() && snap.loaded && !snap.read_only;
    if wrote_live {
        state
            .session
            .config()
            .write_array_values(&state.session, &params.field, &pairs)?;
    } else if snap.loaded && snap.read_only {
        state
            .session
            .config()
            .set_array_values_local(&params.field, &pairs)?;
        state
            .project
            .lock()
            .unwrap()
            .sync_ecu_config_from_session(&state.session)?;
    } else if try_apply_project_config_for_edit(&state)? {
        state
            .session
            .config()
            .set_array_values_local(&params.field, &pairs)?;
        state
            .project
            .lock()
            .unwrap()
            .sync_ecu_config_from_session(&state.session)?;
    } else {
        return Err(
            "Нет config для редактирования — откройте проект с ecuConfig или подключите ECU".into(),
        );
    }

    let snap = state.session.config().snapshot();
    emit_config_update(&app, &snap);
    if state.project.lock().unwrap().info().dirty {
        emit_project(&app, &state);
    }
    if wrote_live {
        set_burn_pending(&state, &app, true);
    }
    Ok(enrich_config_snapshot(&app, snap))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IniInfo {
    pub path: String,
    pub signature: Option<String>,
    pub och_block_size: u16,
    pub field_count: usize,
}

#[tauri::command]
pub fn ini_get_info(state: State<RuntimeState>) -> IniInfo {
    let ctx = state.session.ini_context();
    let path = state
        .session
        .loaded_ini_path()
        .map(|p| p.display().to_string())
        .or_else(|| {
            rusefui_runtime::explicit_ini_path()
                .map(|p| p.display().to_string())
        })
        .unwrap_or_else(|| "(INI загружается при подключении по signature ECU)".into());
    IniInfo {
        path,
        signature: ctx.signature.clone(),
        och_block_size: ctx.block_size,
        field_count: ctx.channels.fields.len(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolLogInfo {
    pub path: String,
    pub entries: Vec<ProtocolLogEntry>,
    pub filters: ProtocolLogFilterSettings,
}

#[tauri::command]
pub fn protocol_log_get(limit: Option<usize>, state: State<RuntimeState>) -> ProtocolLogInfo {
    let limit = limit.unwrap_or(200).min(500);
    ProtocolLogInfo {
        path: state.protocol_log.path().display().to_string(),
        entries: state.protocol_log.list(limit),
        filters: state.protocol_log.filters(),
    }
}

#[tauri::command]
pub fn protocol_log_set_filters(
    filters: ProtocolLogFilterSettings,
    state: State<RuntimeState>,
) {
    state.protocol_log.set_filters(filters);
}

#[tauri::command]
pub fn autoconnect_get_state(state: State<RuntimeState>) -> AutoConnectSnapshot {
    state.autoconnect.snapshot()
}

/// Возвращает текущий статус подключения ECU без побочных эффектов.
/// Используется при монтировании frontend для синхронизации начального состояния.
#[tauri::command]
pub fn autoconnect_get_connection(state: State<RuntimeState>) -> impl serde::Serialize {
    build_ecu_connection_event(&state)
}

#[tauri::command]
pub fn autoconnect_set_offline_mode(
    offline: bool,
    state: State<RuntimeState>,
    app: AppHandle,
) {
    state.autoconnect.set_offline_mode(offline);
    if offline {
        emit_current_output(&app, &state);
    }
    schedule_ecu_notify(&app, false);
    schedule_autoconnect_ui(&app);
}

#[tauri::command]
pub fn protocol_log_clear(state: State<RuntimeState>) {
    state.protocol_log.clear();
}

// --- Workspace (стейт-машина) ---

#[tauri::command]
pub fn workspace_get_state(state: State<RuntimeState>) -> WorkspaceSnapshot {
    let snap = state
        .workspace_fsm
        .lock()
        .unwrap()
        .snapshot()
        .cloned()
        .unwrap_or_else(|| workspace_inputs(&state).derive());
    workspace_snapshot_for_ui(&state, snap)
}

// --- Проект (git-backed) ---

#[tauri::command]
pub fn project_get_info(state: State<RuntimeState>) -> ProjectInfo {
    state.project.lock().unwrap().info()
}

#[tauri::command]
pub fn project_get_document(state: State<RuntimeState>) -> RusefuiProject {
    state.project.lock().unwrap().document()
}

#[tauri::command]
pub fn project_ui_get(key: String, state: State<RuntimeState>) -> Result<Value, String> {
    state.project.lock().unwrap().ui_get(&key)
}

#[tauri::command]
pub fn project_ui_set(
    key: String,
    value: Value,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    state.project.lock().unwrap().ui_set(&key, value)?;
    emit_project(&app, &state);
    Ok(())
}

#[tauri::command]
pub fn project_ui_persist_keys(state: State<RuntimeState>) -> Vec<String> {
    state
        .project
        .lock()
        .unwrap()
        .ui_persist_keys()
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// Создать новый проект в `~/.rusefui/projects/{name}/`.
#[tauri::command]
pub fn project_create_new(
    name: String,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    let label = if name.trim().is_empty() { "Новый проект".into() } else { name };
    let dir = state.project.lock().unwrap().create_project(&label)?;
    record_recent_project(&state, &dir);
    reset_workspace(&state);
    state.project.lock().unwrap().apply_to_session(&state.session)?;
    clear_config_diff(&state, &app);
    emit_project(&app, &state);
    emit_ini_resolution(&app, &state);
    emit_workspace_reset(&app, &state);
    emit_knock_scope_reset(&app);
    Ok(())
}

/// Открыть проект: принимает путь к папке (git-проект) или legacy .rusefui файл.
#[tauri::command]
pub fn project_load(path: String, state: State<RuntimeState>, app: AppHandle) -> Result<(), String> {
    println!("[workspace-fsm] project_load: {path}");
    let path_ref = std::path::Path::new(&path);

    let open_result = state.project.lock().unwrap().open_project_path(path_ref);
    if let Err(ref e) = open_result {
        println!("[workspace-fsm] project_load failed: {e}");
        return Err(open_result.unwrap_err());
    }
    let dir = open_result.unwrap();

    reset_workspace(&state);
    let apply_result = state.project.lock().unwrap().apply_to_session(&state.session);
    if let Err(ref e) = apply_result {
        println!("[workspace-fsm] project_load apply_to_session failed: {e}");
        state.project.lock().unwrap().new_document("Новый проект".into());
        return apply_result;
    }

    println!("[workspace-fsm] project_load: ok");
    record_recent_project(&state, &dir);
    clear_config_diff(&state, &app);
    emit_project(&app, &state);
    emit_ini_resolution(&app, &state);
    emit_workspace_reset(&app, &state);
    emit_knock_scope_reset(&app);
    if state.session.is_connected() && state.session.config().snapshot().loaded {
        try_start_config_diff(&state, &app);
    }
    Ok(())
}

/// Сохранить проект (git commit). Возвращает commit id.
#[tauri::command]
pub fn project_save(
    message: Option<String>,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<String, String> {
    let store = state.project.lock().unwrap();
    store.prepare_for_save(&state.session)?;
    drop(store);
    let commit_id = state.project.lock().unwrap().commit(message.as_deref())?;
    let dir = state.project.lock().unwrap().project_dir();
    if let Some(d) = dir {
        record_recent_project(&state, &d);
    }
    emit_project(&app, &state);
    Ok(commit_id)
}

#[tauri::command]
pub fn recent_projects_list(state: State<RuntimeState>) -> Vec<RecentProjectEntry> {
    state.recent_projects.lock().unwrap().list_entries()
}

/// Список всех git-проектов в `~/.rusefui/projects/`.
#[tauri::command]
pub fn project_list() -> Vec<ProjectListEntry> {
    ProjectGitRepo::list_all()
}

/// Выбрать папку через диалог (для проектов за пределами ~/.rusefui/projects/).
#[tauri::command]
pub async fn pick_project_dir() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("Выбрать папку проекта rusefui")
        .pick_folder()
        .await
        .map(|h| h.path().display().to_string())
}

/// Закрыть проект и вернуть UI в фазу Gate.
#[tauri::command]
pub fn project_close(state: State<RuntimeState>, app: AppHandle) -> Result<(), String> {
    state.project.lock().unwrap().new_document("Новый проект".into());
    set_burn_pending(&state, &app, false);
    reset_workspace(&state);
    state.session.set_project_ini_signature(None);
    state.session.reset_panel_cache_state();
    clear_config_diff(&state, &app);
    emit_project(&app, &state);
    emit_workspace_reset(&app, &state);
    emit_knock_scope_reset(&app);
    Ok(())
}

#[tauri::command]
pub fn project_capture_ecu_config(
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    state.project.lock().unwrap().capture_ecu_config(&state.session)?;
    emit_project(&app, &state);
    Ok(())
}

#[tauri::command]
pub fn project_add_log(
    path: String,
    label: Option<String>,
    kind: Option<String>,
    state: State<RuntimeState>,
    app: AppHandle,
) {
    state.project.lock().unwrap().add_log(
        std::path::Path::new(&path),
        label,
        kind.as_deref(),
    );
    emit_project(&app, &state);
}

#[tauri::command]
pub fn project_remove_log(path: String, state: State<RuntimeState>, app: AppHandle) {
    state.project.lock().unwrap().remove_log(&path);
    emit_project(&app, &state);
}

#[tauri::command]
pub fn project_list_logs(state: State<RuntimeState>) -> Vec<ProjectLogRef> {
    state.project.lock().unwrap().document().logs
}

#[tauri::command]
pub fn project_timeline_list(state: State<RuntimeState>) -> Vec<ProjectTimelineClip> {
    state.project.lock().unwrap().list_timeline_clips()
}

#[tauri::command]
pub fn project_clear_timeline(state: State<RuntimeState>, app: AppHandle) -> Result<bool, String> {
    let changed = state.project.lock().unwrap().clear_timeline();
    if changed {
        emit_project(&app, &state);
    }
    Ok(changed)
}

/// Форк проекта без timeline (новый git-проект). new_name = "" → автоимя.
#[tauri::command]
pub fn project_copy_without_timeline(
    new_name: String,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    let dir = state
        .project
        .lock()
        .unwrap()
        .fork_without_timeline(&new_name, &state.session)?;
    reset_workspace(&state);
    state.project.lock().unwrap().apply_to_session(&state.session)?;
    record_recent_project(&state, &dir);
    clear_config_diff(&state, &app);
    emit_project(&app, &state);
    emit_workspace_reset(&app, &state);
    emit_knock_scope_reset(&app);
    if state.session.is_connected() && state.session.config().snapshot().loaded {
        try_start_config_diff(&state, &app);
    }
    Ok(())
}

// --- История проекта (git) ---

#[tauri::command]
pub fn project_history_list(state: State<RuntimeState>) -> Result<Vec<CommitSummary>, String> {
    state.project.lock().unwrap().history()
}

#[tauri::command]
pub fn project_diff(
    from_id: String,
    to_id: Option<String>,
    state: State<RuntimeState>,
) -> Result<String, String> {
    state.project.lock().unwrap().diff(&from_id, to_id.as_deref())
}

#[tauri::command]
pub fn project_checkout(
    commit_id: String,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    state.project.lock().unwrap().checkout_commit(&commit_id)?;
    emit_project(&app, &state);
    Ok(())
}

// --- Project scripts ---

#[tauri::command]
pub fn project_script_list(state: State<RuntimeState>) -> Vec<rusefui_runtime::ProjectScript> {
    state.project.lock().unwrap().list_scripts()
}

#[tauri::command]
pub fn project_script_create(
    name: String,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<rusefui_runtime::ProjectScript, String> {
    let script = state.project.lock().unwrap().create_script(&name)?;
    emit_project(&app, &state);
    Ok(script)
}

#[tauri::command]
pub fn project_script_delete(
    id: String,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    state.project.lock().unwrap().delete_script(&id)?;
    emit_project(&app, &state);
    Ok(())
}

#[tauri::command]
pub fn project_script_get_content(
    id: String,
    state: State<RuntimeState>,
) -> Result<String, String> {
    state.project.lock().unwrap().get_script_content(&id)
}

#[tauri::command]
pub fn project_script_set_content(
    id: String,
    content: String,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    state.project.lock().unwrap().set_script_content(&id, &content)?;
    emit_project(&app, &state);
    Ok(())
}

#[tauri::command]
pub fn project_script_ecu_read(
    script_field: String,
    state: State<RuntimeState>,
) -> Result<String, String> {
    rusefui_runtime::ecu_script_read(&state.session, &script_field)
}

#[tauri::command]
pub fn project_script_ecu_write(
    script_field: String,
    content: String,
    state: State<RuntimeState>,
) -> Result<(), String> {
    rusefui_runtime::ecu_script_write(&state.session, &script_field, &content)
}

#[tauri::command]
pub fn project_script_ecu_burn(state: State<RuntimeState>) -> Result<(), String> {
    rusefui_runtime::ecu_script_burn(&state.session)
}

#[tauri::command]
pub fn ecu_console_poll(state: State<RuntimeState>) -> String {
    state.session.poll_console_text()
}

#[tauri::command]
pub async fn pick_script_file() -> Option<String> {
    let handle = rfd::AsyncFileDialog::new()
        .add_filter("Lua", &["lua"])
        .set_title("Открыть Lua-скрипт")
        .pick_file()
        .await?;
    Some(handle.path().to_string_lossy().into_owned())
}

#[tauri::command]
pub fn project_script_import(
    path: String,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<ProjectScript, String> {
    let script = state
        .project
        .lock()
        .unwrap()
        .import_script_file(std::path::Path::new(&path))?;
    emit_project(&app, &state);
    Ok(script)
}

#[tauri::command]
pub fn project_script_history(
    id: String,
    state: State<RuntimeState>,
) -> Result<Vec<CommitSummary>, String> {
    state.project.lock().unwrap().script_history(&id)
}

#[tauri::command]
pub fn project_script_diff(
    id: String,
    from_id: String,
    to_id: Option<String>,
    state: State<RuntimeState>,
) -> Result<String, String> {
    state
        .project
        .lock()
        .unwrap()
        .script_diff(&id, &from_id, to_id.as_deref())
}

#[tauri::command]
pub fn project_script_checkout_version(
    id: String,
    commit_id: String,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<String, String> {
    let content = state.project.lock().unwrap().checkout_script(&id, &commit_id)?;
    emit_project(&app, &state);
    Ok(content)
}

// --- INI resolution (несовпадение signature ECU ↔ INI) ---

#[derive(Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IniResolutionInfo {
    pub pending: bool,
    pub ecu_signature: Option<String>,
    pub port_name: Option<String>,
    pub project_signature: Option<String>,
    pub bundle_target: Option<String>,
    pub last_error: Option<String>,
    pub online: Option<OnlineDownloadStatus>,
    /// INI уже найден для ECU, но не применён (ожидание подтверждения).
    pub suggested_ini_path: Option<String>,
}

fn build_ini_resolution_info(state: &RuntimeState) -> IniResolutionInfo {
    let pending: Option<PendingIniResolution> = state.session.pending_ini_resolution();
    let project_signature = state
        .project
        .lock()
        .ok()
        .and_then(|p| p.document().ini.and_then(|i| i.signature));
    match pending {
        Some(p) => {
            let bundle_target = parse_rusefi_signature(&p.ecu_signature).map(|s| s.bundle_target);
            IniResolutionInfo {
                pending: true,
                ecu_signature: Some(p.ecu_signature),
                port_name: Some(p.port_name),
                project_signature: p.project_signature.or(project_signature),
                bundle_target,
                last_error: Some(p.last_error),
                online: Some(p.online),
                suggested_ini_path: p.suggested_ini_path,
            }
        }
        None => IniResolutionInfo {
            pending: false,
            ecu_signature: None,
            port_name: None,
            project_signature,
            bundle_target: None,
            last_error: None,
            online: None,
            suggested_ini_path: None,
        },
    }
}

fn emit_ini_resolution(app: &AppHandle, state: &RuntimeState) {
    let info = build_ini_resolution_info(state);
    let _ = app.emit("ini-resolution", info);
}

#[tauri::command]
pub fn ini_get_resolution(state: State<RuntimeState>) -> IniResolutionInfo {
    build_ini_resolution_info(&state)
}

#[tauri::command]
pub fn ini_list_candidates(state: State<RuntimeState>) -> Vec<IniCandidate> {
    let ecu_sig = state
        .session
        .pending_ini_resolution()
        .map(|p| p.ecu_signature)
        .or_else(|| {
            state
                .session
                .connection_info_if_available()
                .map(|i| i.signature)
        });
    enumerate_local_candidates(ecu_sig.as_deref())
}

#[tauri::command]
pub fn ini_apply_path(
    path: String,
    force: bool,
    update_project_ref: Option<bool>,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    let path_ref = std::path::Path::new(&path);
    state.session.apply_ini_with_options(path_ref, force)?;
    // По умолчанию синхронизируем `project.ini` с применённым файлом — иначе
    // при следующем `project_load` снова получим mismatch.
    if update_project_ref.unwrap_or(true) {
        let ini = state.session.ini_context();
        state.project.lock().unwrap().set_ini_ref(
            state
                .session
                .loaded_ini_path()
                .map(|p| p.display().to_string()),
            ini.signature.clone(),
        );
        emit_project(&app, &state);
    }
    state
        .project
        .lock()
        .unwrap()
        .apply_ecu_config_if_present(&state.session)?;
    // INI применён — фаза должна обновиться (Mismatch → EcuConnectedIdle/ConfigLoad).
    emit_ini_resolution(&app, &state);
    schedule_ecu_notify(&app, true);
    Ok(())
}

#[tauri::command]
pub fn ini_retry_online_download(
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<String, String> {
    let result = state.session.retry_online_ini_resolution();
    if result.is_ok() {
        let ini = state.session.ini_context();
        state.project.lock().unwrap().set_ini_ref(
            state
                .session
                .loaded_ini_path()
                .map(|p| p.display().to_string()),
            ini.signature.clone(),
        );
        emit_project(&app, &state);
        schedule_ecu_notify(&app, true);
    }
    emit_ini_resolution(&app, &state);
    result
}

#[tauri::command]
pub async fn ini_pick_file() -> Option<String> {
    let handle = rfd::AsyncFileDialog::new()
        .set_title("Выбрать INI для ECU")
        .add_filter("TunerStudio INI", &["ini"])
        .pick_file()
        .await?;
    Some(handle.path().display().to_string())
}
