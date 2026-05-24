mod runtime_cmds;

use runtime_cmds::{register_protocol_log_emitter, RuntimeState};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(RuntimeState::default())
        .setup(|app| {
            register_protocol_log_emitter(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            runtime_cmds::component_list_logic_types,
            runtime_cmds::component_mount,
            runtime_cmds::component_get_state,
            runtime_cmds::component_dispatch,
            runtime_cmds::component_unmount,
            runtime_cmds::output_get_snapshot,
            runtime_cmds::output_list_fields,
            runtime_cmds::output_start_listener,
            runtime_cmds::config_get_snapshot,
            runtime_cmds::config_list_fields,
            runtime_cmds::config_start_listener,
            runtime_cmds::config_set_scalar,
            runtime_cmds::ini_get_info,
            runtime_cmds::protocol_log_get,
            runtime_cmds::protocol_log_set_filters,
            runtime_cmds::protocol_log_clear,
        ])
        .run(tauri::generate_context!())
        .expect("error while running rusefui");
}
