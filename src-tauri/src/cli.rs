//! Headless-режим: `--command` — подключиться к ECU и выполнить консольную команду без UI.

use std::time::Duration;

use rusefui_runtime::{connect_ecu_blocking, default_log_path, EcuSession, ProtocolLogStore};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Извлекает текст команды из аргументов процесса (`--command TEXT` или `--command=TEXT`).
pub fn take_ecu_command_from_args() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--command" {
            return args.next().filter(|s| !s.is_empty());
        }
        if let Some(rest) = arg.strip_prefix("--command=") {
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }
    None
}

/// Подключается к ECU, выполняет консольную команду (`E` + ответ `G`), печатает ответ в stdout.
/// Возвращает код выхода процесса (0 — успех).
pub fn run_ecu_command(text: &str) -> i32 {
    match run_ecu_command_inner(text) {
        Ok(response) => {
            let trimmed = response.trim();
            if !trimmed.is_empty() {
                println!("{trimmed}");
            }
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn run_ecu_command_inner(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Пустая команда".into());
    }

    let protocol_log = ProtocolLogStore::new(default_log_path());
    let session = EcuSession::new_arc(protocol_log);

    eprintln!("Поиск ECU…");
    connect_ecu_blocking(&session, CONNECT_TIMEOUT)?;

    let info = session
        .connection_info()
        .map_err(|e| format!("Подключено, но нет сведений о соединении: {e}"))?;
    eprintln!("Подключено: {} ({})", info.port_name, info.signature);

    session.run_console_command(text)
}
