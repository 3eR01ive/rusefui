//! Логика компонентов и подготовка данных на стороне Rust.
//!
//! Vue отвечает только за отрисовку `state` и отправку `dispatch(action)`.

mod autoconnect;
mod component;
pub mod config_table_grid;
mod config_checklist;
mod config_diff;
mod dyno;
mod ini;
mod project;
mod recent_projects;
mod ui_persist;
mod protocol_log;
mod runtime;
mod workspace;
pub mod components;
pub mod session;
pub mod sources;

pub use autoconnect::{AutoConnectManager, AutoConnectSnapshot, AutoConnectTick};
pub use component::{
    ComponentAction, ComponentLogic, ComponentMeta, EcuSyncOnMount, LogicComponentType,
};
pub use config_table_grid::{
    build_grid_view, format_cell_value, interpolate_rect, nudge_rect_values, CellPos,
    GridRect, NavDir, TableGridState, TableGridView,
};
pub use config_checklist::{
    evaluate_checklist, ChecklistEditor, ChecklistIssue, ChecklistItem, ChecklistLevelStatus,
    ChecklistRules, ChecklistSnapshot, FieldMapping, GroupDefinition, LevelDefinition,
};
pub use config_diff::{
    compute_config_diff, ConfigDiffEntry, ConfigDiffSnapshot, ConfigDiffStore, DiffSide,
};
pub use runtime::ComponentRuntime;
pub use session::{EcuSession, PendingIniResolution};
pub use ini::{
    download_ini_for_signature, enumerate_local_candidates, explicit_ini_path,
    find_any_local_ini, ini_cache_dir, ini_download_target, load_ini_path, parse_rusefi_signature,
    resolve_ini_for_signature, search_directories, signatures_match, IniCandidate,
    IniCandidateSource, IniResolveError, OnlineDownloadStatus, ResolvedIni, RusEfiSignature,
};
pub use project::{
    ProjectEcuConfig, ProjectInfo, ProjectLogRef, ProjectStore, RusefuiProject, FORMAT_VERSION,
};
pub use recent_projects::{RecentProjectEntry, RecentProjectsStore};
pub use ui_persist::{
    CompositeChartUiSettings, DynoUiSettings, LogGraphGroupJson, LogRangeInputJson, LogUiSettings,
    ProjectUi, PERSIST_KEY_COMPOSITE_CHART, PERSIST_KEY_DYNO, PERSIST_KEY_OUTPUT_CHART,
};
pub use ui_persist::{ComponentUiPersist, persist_keys as ui_persist_keys};
pub use protocol_log::{
    default_log_path, LogLevel, ProtocolLogEntry, ProtocolLogFilterSettings, ProtocolLogStore,
};
pub use rusefi_protocol::ProtocolLogSource;
pub use sources::composite_data_log::composite_logs_dir;
pub use sources::composite_logger::{
    CompositeEventJson, CompositeLoggerSource, CompositeSnapshot,
};
pub use sources::knock_scope::{KnockScopeSnapshot, KnockScopeSource, KNOCK_ADC_HZ};
pub use sources::knock_spectrogram::KnockSpectrogramView;
pub use sources::composite_timeline::{
    CompositeTimeline, CompositeTimelineStatus, CompositeTimelineView,
    CompositeTimelineViewQuery,
};
pub use sources::composite_trigger_wheels::{
    compute_trigger_wheels, ComputeTriggerWheelsParams, TriggerWheelsView, WheelEdgeMode,
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
pub use workspace::{
    ConfigSource, WorkspaceCapabilities, WorkspaceFsm, WorkspaceInputs, WorkspacePhase,
    WorkspaceSnapshot, WorkspaceSyncPlan, derive_workspace,
};
