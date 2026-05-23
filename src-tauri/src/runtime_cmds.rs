use rusefui_runtime::{ComponentRuntime, EcuSession, OutputSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

pub struct RuntimeState {
    pub session: Arc<EcuSession>,
    pub runtime: Mutex<ComponentRuntime>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        let session = EcuSession::new_arc();
        Self {
            session: Arc::clone(&session),
            runtime: Mutex::new(ComponentRuntime::new(session)),
        }
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
    IniInfo {
        path: rusefui_runtime::resolve_ini_path().display().to_string(),
        signature: ctx.signature.clone(),
        och_block_size: ctx.block_size,
        field_count: ctx.channels.fields.len(),
    }
}
