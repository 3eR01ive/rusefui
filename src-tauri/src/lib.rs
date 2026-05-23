mod runtime_cmds;

use runtime_cmds::RuntimeState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(RuntimeState::default())
        .invoke_handler(tauri::generate_handler![
            runtime_cmds::component_list_logic_types,
            runtime_cmds::component_mount,
            runtime_cmds::component_get_state,
            runtime_cmds::component_dispatch,
            runtime_cmds::component_unmount,
        ])
        .run(tauri::generate_context!())
        .expect("error while running rusefui");
}
