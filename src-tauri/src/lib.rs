mod runtime_cmds;

use runtime_cmds::{register_protocol_log_emitter, start_autoconnect, RuntimeState};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(RuntimeState::default())
        .setup(|app| {
            register_protocol_log_emitter(app.handle());
            start_autoconnect(app.handle().clone());
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
            runtime_cmds::output_timeline_status,
            runtime_cmds::output_timeline_query_view,
            runtime_cmds::output_timeline_set_view,
            runtime_cmds::output_timeline_load_file,
            runtime_cmds::pick_output_log_path,
            runtime_cmds::config_get_snapshot,
            runtime_cmds::config_list_fields,
            runtime_cmds::config_start_listener,
            runtime_cmds::config_set_scalar,
            runtime_cmds::config_burn,
            runtime_cmds::config_get_array,
            runtime_cmds::config_set_array_value,
            runtime_cmds::ini_get_info,
            runtime_cmds::autoconnect_get_state,
            runtime_cmds::autoconnect_set_offline_mode,
            runtime_cmds::ecu_resync,
            runtime_cmds::protocol_log_get,
            runtime_cmds::protocol_log_set_filters,
            runtime_cmds::protocol_log_clear,
            runtime_cmds::project_get_info,
            runtime_cmds::project_get_document,
            runtime_cmds::project_ui_get,
            runtime_cmds::project_ui_set,
            runtime_cmds::project_ui_persist_keys,
            runtime_cmds::project_create_new,
            runtime_cmds::project_load,
            runtime_cmds::project_save,
            runtime_cmds::project_save_path,
            runtime_cmds::project_capture_ecu_config,
            runtime_cmds::project_add_log,
            runtime_cmds::project_remove_log,
            runtime_cmds::project_list_logs,
            runtime_cmds::pick_project_open_path,
            runtime_cmds::pick_project_save_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running rusefui");
}
