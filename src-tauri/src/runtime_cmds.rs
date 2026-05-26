use rusefui_runtime::{
    compute_config_diff, default_log_path, AutoConnectManager, AutoConnectSnapshot,
    ComponentRuntime, CompositeSnapshot, ConfigDiffSnapshot, ConfigDiffStore, ConfigFieldInfo,
    ConfigSnapshot, DiffSide, EcuSession, EcuSyncOnMount, OutputFieldInfo, OutputSnapshot,
    OutputTimelineStatus, OutputTimelineView, OutputTimelineViewControl, ProjectInfo,
    ProjectLogRef, ProjectStore, ProtocolLogEntry, ProtocolLogFilterSettings, ProtocolLogStore,
    RusefuiProject,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// Последнее отправленное в UI `ecu-connection` (без дублей на каждый dispatch).
    last_ecu_connection_emit: Mutex<Option<EcuConnectionEvent>>,
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
            last_ecu_connection_emit: Mutex::new(None),
        }
    }
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
    let snap = state.session.config().snapshot();
    if !snap.connected || !snap.loaded || snap.loading {
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

/// Сброс config/timeline в UI после смены проекта.
fn emit_workspace_reset(app: &AppHandle, state: &RuntimeState) {
    emit_config(app, &state.session.config().snapshot());
    emit_output(app, &state.session.output().snapshot());
    emit_composite(app, &state.session.composite().snapshot());
    let timeline = state.session.output_timeline_status();
    let _ = app.emit("output-timeline-status", timeline);
    let _ = app.emit("workspace-reset", ());
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

fn emit_composite(app: &AppHandle, snapshot: &CompositeSnapshot) {
    let app = app.clone();
    let snapshot = snapshot.clone();
    tauri::async_runtime::spawn(async move {
        let _ = app.emit("composite-logger", snapshot);
    });
}

fn emit_config(app: &AppHandle, snapshot: &ConfigSnapshot) {
    let app = app.clone();
    let snapshot = snapshot.clone();
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

/// Подключение ECU изменилось — `ecu-connection` только при смене + опционально sync.
pub fn schedule_ecu_notify(app: &AppHandle, sync_ecu: bool) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<RuntimeState>();
        emit_ecu_connection_if_changed(&app, &state);
        if sync_ecu && state.session.is_connected() && !state.session.is_ecu_busy() {
            sync_ecu_data(&state, &app);
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
    schedule_ecu_notify(&app, false);
    schedule_autoconnect_ui(&app);
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
        emit_config(app, snap);
    }
}

fn sync_output_poll_session(session: &Arc<EcuSession>, app: &AppHandle) {
    if session.should_poll_output_channels() {
        if session.output().is_polling() {
            return;
        }
        let app = app.clone();
        let poll_session = Arc::clone(session);
        session.output().start(poll_session, move |snap| {
            emit_output(&app, &snap);
        });
    } else {
        session.output().stop();
        emit_output(app, &session.output().snapshot());
    }
}

fn sync_config_load(state: &RuntimeState, app: &AppHandle) {
    if !state.session.is_connected() {
        state.session.config().stop();
        emit_config_update(app, &state.session.config().snapshot());
        return;
    }

    let snap = state.session.config().snapshot();
    if snap.loaded || snap.loading {
        return;
    }

    // Конфиг читается эксклюзивно — output/composite poll мешают (см. Java `readFullImageFromController`).
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
            }
            sync_output_poll_session(&session, &app);
        }
    });
}

fn sync_ecu_data(state: &RuntimeState, app: &AppHandle) {
    if !state.session.is_connected() {
        state.session.config().stop();
        state.session.output().stop();
        state.session.composite().disable_on_ecu(&state.session);
        state.session.composite().stop();
        clear_config_diff(state, app);
        emit_config_update(app, &state.session.config().snapshot());
        emit_output(app, &state.session.output().snapshot());
        emit_composite(app, &state.session.composite().snapshot());
        return;
    }

    let config_snap = state.session.config().snapshot();
    if config_snap.loaded {
        sync_output_poll_session(&state.session, app);
    } else if !config_snap.loading {
        sync_config_load(state, app);
    }
}

fn protocol_log_emit_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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
    let snapshot = rt.mount(&params.instance_id, &params.component_type)?;
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

    let snapshot = {
        let mut rt = state.runtime.lock().map_err(|e| e.to_string())?;
        let snapshot = rt.dispatch(&params.instance_id, action, params.payload)?;

        if action == "connect" {
            state.autoconnect.clear_manual_disconnect();
        } else if action == "disconnect" {
            if let Some(port) = manual_disconnect_port {
                state.autoconnect.note_manual_disconnect(Some(&port));
            }
        }

        snapshot
    };

    emit_state(&app, &params.instance_id, &snapshot);

    if matches!(action, "connect" | "disconnect") {
        schedule_ecu_notify(&app, action == "connect");
    }

    Ok(snapshot)
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
    state.session.output().snapshot()
}

#[tauri::command]
pub fn output_list_fields(state: State<RuntimeState>) -> Vec<OutputFieldInfo> {
    state.session.bootstrap_offline_ini_if_needed();
    state.session.ini_context().list_output_fields()
}

#[tauri::command]
pub fn output_start_listener(state: State<RuntimeState>, app: AppHandle) {
    state.session.bootstrap_offline_ini_if_needed();
    emit_output(&app, &state.session.output().snapshot());
    sync_ecu_data(&state, &app);
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
        let app_emit = app.clone();
        let session = Arc::clone(&state.session);
        state.session.composite().start(session, move |snap| {
            emit_composite(&app_emit, &snap);
        })?;
    } else {
        state.session.composite().disable_on_ecu(&state.session);
        state.session.composite().stop();
        emit_composite(&app, &state.session.composite().snapshot());
    }
    Ok(state.session.composite().snapshot())
}

#[tauri::command]
pub fn composite_set_max_window_ms(max_window_ms: f64, state: State<RuntimeState>) {
    state.session.composite().set_max_window_ms(max_window_ms);
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
) -> OutputTimelineStatus {
    state.session.output_timeline_control(params.ctrl)
}

#[tauri::command]
pub fn output_timeline_load_file(
    state: State<RuntimeState>,
    path: String,
) -> Result<OutputTimelineStatus, String> {
    Ok(state
        .session
        .output_timeline_load_file(std::path::PathBuf::from(path)))
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
pub fn config_get_snapshot(state: State<RuntimeState>) -> ConfigSnapshot {
    state.session.config().snapshot()
}

#[tauri::command]
pub fn config_list_fields(state: State<RuntimeState>) -> Vec<ConfigFieldInfo> {
    state.session.config().list_fields()
}

#[tauri::command]
pub fn config_start_listener(state: State<RuntimeState>, app: AppHandle) {
    emit_config(&app, &state.session.config().snapshot());
    sync_ecu_data(&state, &app);
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
    for (field, side, value) in plan {
        match side {
            DiffSide::Project => {
                session.config().write_scalar(&session, &field, value)?;
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

fn write_config_scalar(
    state: &RuntimeState,
    field: &str,
    value: f64,
) -> Result<ConfigSnapshot, String> {
    let snap = state.session.config().snapshot();
    if state.session.is_connected() && snap.loaded && !snap.read_only {
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
    } else {
        return Err(
            "Нет config для редактирования — откройте проект с ecuConfig или подключите ECU"
                .into(),
        );
    }
    Ok(state.session.config().snapshot())
}

#[tauri::command]
pub fn config_set_scalar(
    params: ConfigSetScalarParams,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<ConfigSnapshot, String> {
    let snap = write_config_scalar(&state, &params.field, params.value)?;
    emit_config_update(&app, &snap);
    if state.project.lock().unwrap().info().dirty {
        emit_project(&app, &state);
    }
    if state.session.should_poll_output_channels() {
        sync_output_poll_session(&state.session, &app);
    }
    Ok(snap)
}

#[tauri::command]
pub fn config_burn(state: State<RuntimeState>, app: AppHandle) -> Result<(), String> {
    if !state.session.is_connected() {
        return Err("ECU не подключена".into());
    }

    state.session.output().stop();
    state.session.composite().disable_on_ecu(&state.session);
    state.session.composite().stop();

    let session = Arc::clone(&state.session);
    session.config().burn_to_flash(&session)?;

    let snap = session.config().snapshot();
    emit_config_update(&app, &snap);

    if session.should_poll_output_channels() {
        sync_output_poll_session(&session, &app);
    } else {
        emit_output(&app, &session.output().snapshot());
    }

    Ok(())
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
    if state.session.is_connected() && snap.loaded && !snap.read_only {
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
    Ok(snap)
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

#[tauri::command]
pub fn autoconnect_set_offline_mode(
    offline: bool,
    state: State<RuntimeState>,
    app: AppHandle,
) {
    state.autoconnect.set_offline_mode(offline);
    if offline {
        state.session.bootstrap_offline_ini_if_needed();
        emit_output(&app, &state.session.output().snapshot());
    }
    schedule_ecu_notify(&app, false);
    schedule_autoconnect_ui(&app);
}

#[tauri::command]
pub fn protocol_log_clear(state: State<RuntimeState>) {
    state.protocol_log.clear();
}

// --- Проект (JSON) ---

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

#[tauri::command]
pub fn project_create_new(
    path: String,
    name: Option<String>,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    let label = name.unwrap_or_else(|| "Новый проект".into());
    let store = state.project.lock().unwrap();
    store.new_document(label);
    store.save_to_path(std::path::Path::new(&path))?;
    drop(store);
    state.session.reset_workspace_for_new_project();
    clear_config_diff(&state, &app);
    emit_project(&app, &state);
    emit_workspace_reset(&app, &state);
    Ok(())
}

#[tauri::command]
pub fn project_load(path: String, state: State<RuntimeState>, app: AppHandle) -> Result<(), String> {
    let store = state.project.lock().unwrap();
    store.load_from_path(std::path::Path::new(&path))?;
    state.session.reset_workspace_for_new_project();
    store.apply_to_session(&state.session)?;
    drop(store);
    clear_config_diff(&state, &app);
    emit_project(&app, &state);
    emit_workspace_reset(&app, &state);
    if state.session.is_connected() && state.session.config().snapshot().loaded {
        try_start_config_diff(&state, &app);
    }
    Ok(())
}

#[tauri::command]
pub fn project_save(state: State<RuntimeState>, app: AppHandle) -> Result<String, String> {
    let store = state.project.lock().unwrap();
    let path = store
        .saved_path()
        .ok_or_else(|| "Укажите файл: «Сохранить как…»".to_string())?;
    store.save_to_path(&path)?;
    drop(store);
    emit_project(&app, &state);
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn project_save_path(
    path: String,
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    state
        .project
        .lock()
        .unwrap()
        .save_to_path(std::path::Path::new(&path))?;
    emit_project(&app, &state);
    Ok(())
}

#[tauri::command]
pub fn project_capture_ecu_config(
    state: State<RuntimeState>,
    app: AppHandle,
) -> Result<(), String> {
    state
        .project
        .lock()
        .unwrap()
        .capture_ecu_config(&state.session)?;
    emit_project(&app, &state);
    Ok(())
}

#[tauri::command]
pub fn project_add_log(
    path: String,
    label: Option<String>,
    state: State<RuntimeState>,
    app: AppHandle,
) {
    state
        .project
        .lock()
        .unwrap()
        .add_log(std::path::Path::new(&path), label);
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
pub async fn pick_project_open_path() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("Открыть проект rusefui")
        .add_filter("rusefui project", &["json"])
        .pick_file()
        .await
        .map(|h| h.path().display().to_string())
}

#[tauri::command]
pub async fn pick_project_save_path(default_name: Option<String>) -> Option<String> {
    let mut dlg = rfd::AsyncFileDialog::new()
        .set_title("Сохранить проект rusefui")
        .add_filter("rusefui project", &["json"]);
    if let Some(name) = default_name.filter(|s| !s.is_empty()) {
        let file_name = if name.ends_with(".json") {
            name
        } else {
            format!("{name}.json")
        };
        dlg = dlg.set_file_name(&file_name);
    }
    dlg.save_file()
        .await
        .map(|h| h.path().display().to_string())
}
