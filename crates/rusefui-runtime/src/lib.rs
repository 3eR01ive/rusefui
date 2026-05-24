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
pub use ini::{
    explicit_ini_path, ini_cache_dir, resolve_ini_for_signature, search_directories,
    signatures_match, IniResolveError, ResolvedIni,
};
pub use protocol_log::{
    default_log_path, LogLevel, ProtocolLogEntry, ProtocolLogFilterSettings, ProtocolLogStore,
};
pub use sources::config::{ConfigFieldInfo, ConfigSnapshot};
pub use sources::output_channels::{
    IniContext, OutputFieldInfo, OutputSnapshot, DEFAULT_OUTPUT_BLOCK_SIZE,
};
