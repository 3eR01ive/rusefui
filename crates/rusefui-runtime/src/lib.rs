//! Логика компонентов и подготовка данных на стороне Rust.
//!
//! Vue отвечает только за отрисовку `state` и отправку `dispatch(action)`.

mod autoconnect;
mod component;
mod config_diff;
mod ini;
mod project;
mod ui_persist;
mod protocol_log;
mod runtime;
pub mod components;
pub mod session;
pub mod sources;

pub use autoconnect::{AutoConnectManager, AutoConnectSnapshot, AutoConnectTick};
pub use component::{
    ComponentAction, ComponentLogic, ComponentMeta, EcuSyncOnMount, LogicComponentType,
};
pub use config_diff::{
    compute_config_diff, ConfigDiffEntry, ConfigDiffSnapshot, ConfigDiffStore, DiffSide,
};
pub use runtime::ComponentRuntime;
pub use session::EcuSession;
pub use ini::{
    explicit_ini_path, find_any_local_ini, ini_cache_dir, load_ini_path,
    resolve_ini_for_signature, search_directories, signatures_match, IniResolveError, ResolvedIni,
};
pub use project::{
    ProjectEcuConfig, ProjectInfo, ProjectLogRef, ProjectStore, RusefuiProject, FORMAT_VERSION,
};
pub use ui_persist::{
    CompositeChartUiSettings, LogGraphGroupJson, LogRangeInputJson, LogUiSettings, ProjectUi,
    PERSIST_KEY_COMPOSITE_CHART, PERSIST_KEY_OUTPUT_CHART,
};
pub use ui_persist::{ComponentUiPersist, persist_keys as ui_persist_keys};
pub use protocol_log::{
    default_log_path, LogLevel, ProtocolLogEntry, ProtocolLogFilterSettings, ProtocolLogStore,
};
pub use sources::composite_logger::{
    CompositeEventJson, CompositeLoggerSource, CompositeSnapshot,
};
pub use sources::config::{ConfigFieldInfo, ConfigSnapshot};
pub use sources::output_channels::{
    IniContext, OutputFieldInfo, OutputSnapshot, DEFAULT_OUTPUT_BLOCK_SIZE,
};
pub use sources::output_data_log::output_logs_dir;
pub use sources::output_timeline::{
    OutputTimeline, OutputTimelineStatus, OutputTimelineView, OutputTimelineViewControl,
    OutputTimelineViewQuery, TimelineFieldView, TimelineMode, TimelinePoint,
};
