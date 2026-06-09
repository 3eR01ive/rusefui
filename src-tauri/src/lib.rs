pub mod cli;
mod runtime_cmds;

use runtime_cmds::{
    register_config_burn_notify, register_knock_scope_emitter, register_protocol_log_emitter,
    start_autoconnect, RuntimeState,
};
use tauri::Emitter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(RuntimeState::default())
        .setup(|app| {
            register_protocol_log_emitter(app.handle());
            register_config_burn_notify(app.handle());
            register_knock_scope_emitter(app.handle());
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
            runtime_cmds::output_set_log_cursor,
            runtime_cmds::output_list_fields,
            runtime_cmds::output_start_listener,
            runtime_cmds::composite_get_snapshot,
            runtime_cmds::composite_set_enabled,
            runtime_cmds::composite_set_max_window_ms,
            runtime_cmds::composite_compute_trigger_wheels,
            runtime_cmds::composite_timeline_session_events,
            runtime_cmds::knock_scope_get_snapshot,
            runtime_cmds::knock_scope_gpu_buffer,
            runtime_cmds::knock_scope_set_enabled,
            runtime_cmds::knock_scope_pan_spectrogram,
            runtime_cmds::knock_scope_set_spectrogram_follow_live,
            runtime_cmds::composite_timeline_status,
            runtime_cmds::composite_timeline_query_view,
            runtime_cmds::composite_timeline_set_view,
            runtime_cmds::composite_timeline_load_file,
            runtime_cmds::pick_composite_log_path,
            runtime_cmds::log_viewport_set_linked,
            runtime_cmds::log_viewport_get_linked,
            runtime_cmds::output_timeline_status,
            runtime_cmds::output_timeline_series_snapshot,
            runtime_cmds::output_timeline_pull_series_chunk,
            runtime_cmds::output_timeline_query_view,
            runtime_cmds::output_timeline_set_view,
            runtime_cmds::output_timeline_load_file,
            runtime_cmds::pick_output_log_path,
            runtime_cmds::config_get_snapshot,
            runtime_cmds::checklist_load_rules,
            runtime_cmds::config_list_fields,
            runtime_cmds::config_start_listener,
            runtime_cmds::workspace_get_state,
            runtime_cmds::config_diff_get,
            runtime_cmds::config_diff_set_choice,
            runtime_cmds::config_diff_set_all,
            runtime_cmds::config_diff_apply,
            runtime_cmds::config_diff_dismiss,
            runtime_cmds::config_set_scalar,
            runtime_cmds::config_set_string,
            runtime_cmds::config_burn,
            runtime_cmds::config_get_array,
            runtime_cmds::config_set_array_value,
            runtime_cmds::config_set_array_values,
            runtime_cmds::ini_get_info,
            runtime_cmds::ini_get_resolution,
            runtime_cmds::ini_list_candidates,
            runtime_cmds::ini_apply_path,
            runtime_cmds::ini_retry_online_download,
            runtime_cmds::ini_pick_file,
            runtime_cmds::autoconnect_get_state,
            runtime_cmds::autoconnect_get_connection,
            runtime_cmds::autoconnect_set_offline_mode,
            runtime_cmds::stimulator_set_rpm,
            runtime_cmds::stimulator_ramp_start,
            runtime_cmds::stimulator_ramp_cancel,
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
            runtime_cmds::project_close,
            runtime_cmds::project_save,
            runtime_cmds::project_save_path,
            runtime_cmds::project_capture_ecu_config,
            runtime_cmds::project_add_log,
            runtime_cmds::project_remove_log,
            runtime_cmds::project_list_logs,
            runtime_cmds::project_timeline_list,
            runtime_cmds::project_clear_timeline,
            runtime_cmds::project_copy_without_timeline,
            runtime_cmds::pick_project_open_path,
            runtime_cmds::pick_project_save_path,
            runtime_cmds::recent_projects_list,
            runtime_cmds::app_force_quit,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.emit("app-close-requested", ());
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running rusefui");
}
