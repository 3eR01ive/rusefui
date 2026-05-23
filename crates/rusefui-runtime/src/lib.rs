//! Логика компонентов и подготовка данных на стороне Rust.
//!
//! Vue отвечает только за отрисовку `state` и отправку `dispatch(action)`.

mod component;
mod ini;
mod protocol_log;
mod runtime;
pub mod components;
pub mod session;
pub mod sources;

pub use component::{ComponentAction, ComponentLogic, ComponentMeta, LogicComponentType};
pub use runtime::ComponentRuntime;
pub use session::EcuSession;
pub use ini::{load_ini, resolve_ini_path};
pub use protocol_log::{default_log_path, ProtocolLogEntry, ProtocolLogStore};
pub use sources::output_channels::{IniContext, OutputSnapshot, DEFAULT_OUTPUT_BLOCK_SIZE};
