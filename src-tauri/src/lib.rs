pub mod cli;
mod runtime_cmds;

use runtime_cmds::{
    register_config_burn_notify, register_knock_scope_emitter, register_panels_emitter,
    register_protocol_log_emitter, start_autoconnect, RuntimeState,
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
            register_panels_emitter(app.handle());
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
            runtime_cmds::composite_analyze_trigger,
            runtime_cmds::composite_timeline_session_events,
            runtime_cmds::engine_sniffer_get_snapshot,
            runtime_cmds::engine_sniffer_set_enabled,
            runtime_cmds::knock_scope_get_snapshot,
            runtime_cmds::knock_scope_gpu_buffer,
            runtime_cmds::knock_scope_set_enabled,
            runtime_cmds::knock_scope_pan_spectrogram,
            runtime_cmds::knock_scope_set_spectrogram_follow_live,
            runtime_cmds::knock_scope_set_viewport_columns,
            runtime_cmds::knock_scope_save_recording,
            runtime_cmds::knock_scope_load_recording,
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
            runtime_cmds::ini_list_tables,
            runtime_cmds::ini_list_curves,
            runtime_cmds::ini_apply_path,
            runtime_cmds::ini_retry_online_download,
            runtime_cmds::ini_pick_file,
            runtime_cmds::panels_get_manifest,
            runtime_cmds::panels_read_yaml,
            runtime_cmds::read_ui_config,
            runtime_cmds::project_read_ui_config,
            runtime_cmds::project_reset_tab_config,
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
            runtime_cmds::project_capture_ecu_config,
            runtime_cmds::project_add_log,
            runtime_cmds::project_remove_log,
            runtime_cmds::project_list_logs,
            runtime_cmds::project_timeline_list,
            runtime_cmds::project_clear_timeline,
            runtime_cmds::project_copy_without_timeline,
            runtime_cmds::project_list,
            runtime_cmds::pick_project_dir,
            runtime_cmds::project_history_list,
            runtime_cmds::project_diff,
            runtime_cmds::project_checkout,
            runtime_cmds::project_script_list,
            runtime_cmds::project_script_create,
            runtime_cmds::project_script_delete,
            runtime_cmds::project_script_get_content,
            runtime_cmds::project_script_set_content,
            runtime_cmds::project_script_ecu_read,
            runtime_cmds::project_script_ecu_write,
            runtime_cmds::project_script_ecu_burn,
            runtime_cmds::ecu_console_poll,
            runtime_cmds::pick_script_file,
            runtime_cmds::project_script_import,
            runtime_cmds::project_script_history,
            runtime_cmds::project_script_diff,
            runtime_cmds::project_script_checkout_version,
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
