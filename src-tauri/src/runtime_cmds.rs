use rusefui_runtime::{
    default_log_path, ComponentRuntime, EcuSession, OutputFieldInfo, OutputSnapshot,
    ProtocolLogEntry, ProtocolLogStore,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct RuntimeState {
    pub session: Arc<EcuSession>,
    pub runtime: Mutex<ComponentRuntime>,
    pub protocol_log: Arc<ProtocolLogStore>,
}

impl RuntimeState {
    pub fn new(protocol_log: Arc<ProtocolLogStore>) -> Self {
        let session = EcuSession::new_arc(Arc::clone(&protocol_log));
        Self {
            session: Arc::clone(&session),
            runtime: Mutex::new(ComponentRuntime::new(session)),
            protocol_log,
        }
    }
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
    let _ = app.emit("output-channels", snapshot);
}

fn emit_protocol_log(app: &AppHandle, entry: &ProtocolLogEntry) {
    let _ = app.emit("protocol-log", entry);
}

fn sync_output_poll(state: &RuntimeState, app: &AppHandle) {
    if state.session.is_connected() {
        let app = app.clone();
        let session = Arc::clone(&state.session);
        state.session.output().start(session, move |snap| {
            emit_output(&app, &snap);
        });
    } else {
        state.session.output().stop();
        emit_output(app, &state.session.output().snapshot());
    }
}

pub fn register_protocol_log_emitter(app: &AppHandle) {
    let handle = app.clone();
    let state = app.state::<RuntimeState>();
    state.protocol_log.add_listener(Arc::new(move |entry| {
        emit_protocol_log(&handle, entry);
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
    emit_state(&app, &params.instance_id, &snapshot);
    sync_output_poll(&state, &app);
    Ok(snapshot)
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
    let mut rt = state.runtime.lock().map_err(|e| e.to_string())?;
    let snapshot = rt.dispatch(&params.instance_id, &params.action, params.payload)?;
    emit_state(&app, &params.instance_id, &snapshot);
    sync_output_poll(&state, &app);
    Ok(snapshot)
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
    state.session.ini_context().list_output_fields()
}

#[tauri::command]
pub fn output_start_listener(state: State<RuntimeState>, app: AppHandle) {
    emit_output(&app, &state.session.output().snapshot());
    sync_output_poll(&state, &app);
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
}

#[tauri::command]
pub fn protocol_log_get(limit: Option<usize>, state: State<RuntimeState>) -> ProtocolLogInfo {
    let limit = limit.unwrap_or(200).min(500);
    ProtocolLogInfo {
        path: state.protocol_log.path().display().to_string(),
        entries: state.protocol_log.list(limit),
    }
}

#[tauri::command]
pub fn protocol_log_clear(state: State<RuntimeState>) {
    state.protocol_log.clear();
}
