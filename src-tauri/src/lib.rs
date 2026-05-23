mod connection;

use connection::{AppConnection, ConnectParams, ConnectionStatusDto};
use std::sync::Mutex;
use tauri::State;

#[derive(Default)]
pub struct AppState {
    pub connection: Mutex<AppConnection>,
}

#[tauri::command]
fn list_serial_ports() -> Result<Vec<String>, String> {
    rusefi_protocol::SerialLink::list_ports().map_err(|e| e.to_string())
}

#[tauri::command]
fn connect_ecu(
    state: State<'_, AppState>,
    params: ConnectParams,
) -> Result<ConnectionStatusDto, String> {
    state
        .connection
        .lock()
        .map_err(|e| e.to_string())?
        .connect(params)
}

#[tauri::command]
fn disconnect_ecu(state: State<'_, AppState>) -> Result<ConnectionStatusDto, String> {
    state
        .connection
        .lock()
        .map_err(|e| e.to_string())?
        .disconnect()
}

#[tauri::command]
fn connection_status(state: State<'_, AppState>) -> Result<ConnectionStatusDto, String> {
    Ok(state
        .connection
        .lock()
        .map_err(|e| e.to_string())?
        .status_dto())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            list_serial_ports,
            connect_ecu,
            disconnect_ecu,
            connection_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running rusefui");
}
