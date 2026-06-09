// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod linux_appimage;

fn main() {
    #[cfg(target_os = "linux")]
    linux_appimage::prepare_webview_env();

    if let Some(cmd) = rusefui_lib::cli::take_ecu_command_from_args() {
        std::process::exit(rusefui_lib::cli::run_ecu_command(&cmd));
    }

    rusefui_lib::run()
}
