//! Логика компонентов и подготовка данных на стороне Rust.
//!
//! Vue отвечает только за отрисовку `state` и отправку `dispatch(action)`.

mod autoconnect;
mod component;
pub mod config_table_grid;
mod config_checklist;
mod config_conflicts;
mod config_ignition_plausibility;
mod config_vars;
mod config_diff;
mod dyno;
mod ignition_map;
mod knock;
mod layout;
mod ini;
mod project;
mod project_timeline;
mod recent_projects;
mod ui_persist;
mod protocol_log;
mod runtime;
mod stimulator_ramp;
mod workspace;
pub mod components;
pub mod session;
pub mod sources;

pub use autoconnect::{
    connect_ecu_blocking, try_connect_ecu_once, AutoConnectManager, AutoConnectSnapshot,
    AutoConnectTick,
};
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
pub use config_vars::{
    logic as config_logic_var, ConflictConstants, ConfigVarResolver, VarBinding,
};
pub use config_diff::{
    compute_config_diff, ConfigDiffEntry, ConfigDiffSnapshot, ConfigDiffStore, DiffSide,
};
pub use runtime::ComponentRuntime;
pub use session::{EcuSession, PendingIniResolution};
pub use stimulator_ramp::{
    StimulatorRampParams, StimulatorRampPhase, StimulatorRampResult, StimulatorRampRunner,
    StimulatorRampStep, DEFAULT_RAMP_STEP_MS,
};
pub use ini::{
    cache_dir_for_project_ini, download_ini_for_signature, enumerate_local_candidates,
    ensure_panels_for_ini, explicit_ini_path, find_any_local_ini, ini_cache_dir, ini_download_target,
    install_ini_to_cache, load_ini_path, parse_rusefi_signature, project_cache_key,
    read_manifest_from_dir, read_panel_yaml, resolve_ini_for_signature, search_directories,
    signatures_match, IniCandidate, IniCandidateSource, IniResolveError, OnlineDownloadStatus,
    PanelCacheStatus, ResolvedIni, RusEfiSignature,
};
pub use rusefi_ini::PanelManifest;
pub use project::{
    is_project_file_path, with_project_extension, ProjectEcuConfig, ProjectInfo, ProjectLogRef,
    ProjectStore, RusefuiProject, FORMAT_VERSION, LEGACY_PROJECT_FILE_EXTENSION,
    PROJECT_FILE_EXTENSION,
};
pub use project_timeline::{
    channel as project_timeline_channel, ProjectTimeline, ProjectTimelineClip,
    ProjectTimelineRecordRef,
};
pub use recent_projects::{RecentProjectEntry, RecentProjectsStore};
pub use ui_persist::{
    CompositeChartUiSettings, DynoUiSettings, KnockUiSettings, LogGraphGroupJson, LogRangeInputJson, LogUiSettings,
    ProjectUi, PERSIST_KEY_COMPOSITE_CHART, PERSIST_KEY_DYNO, PERSIST_KEY_KNOCK, PERSIST_KEY_OUTPUT_CHART,
    RampCurveKind,
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
pub use sources::knock_scope::{KnockScopeSnapshot, KnockScopeSource, KnockScopeUiTick, KNOCK_ADC_HZ};
pub use sources::knock_spectrogram::{
    encode_knock_spectrogram_gpu, encode_knock_spectrogram_gpu_b64,
    encode_knock_spectrogram_gpu_patch_b64, KnockSpectrogramMarker, KnockSpectrogramPatch,
    KnockSpectrogramView,
};
pub use sources::composite_timeline::{
    CompositeTimeline, CompositeTimelineStatus, CompositeTimelineView,
    CompositeTimelineViewQuery,
};
pub use sources::composite_trigger_wheels::{
    compute_trigger_wheels, ComputeTriggerWheelsParams, TriggerWheelsView, WheelEdgeMode,
};
pub use sources::config::{ConfigFieldInfo, ConfigSnapshot};
pub use sources::output_channels::{
    IniContext, OutputFieldInfo, OutputSnapshot, OutputValuesSource, DEFAULT_OUTPUT_BLOCK_SIZE,
};
pub use sources::output_data_log::output_logs_dir;
pub use sources::output_timeline::{
    OutputTimeline, OutputTimelineSeriesChunk, OutputTimelineSeriesSnapshot, OutputTimelineStatus,
    OutputTimelineView, OutputTimelineViewControl, OutputTimelineViewQuery,
    OutputTimelineSeriesQuery, OutputTimelineChunkQuery, TimelineFieldView, TimelineMode,
    TimelinePoint, SERIES_CHUNK_MAX_POINTS, SERIES_SNAPSHOT_MAX_POINTS, FILE_CHUNK_ROWS_DEFAULT,
};
pub use layout::{
    build_nav_paths, is_container, is_filter_nav_path, nav_region, resolve_nav_activatable,
    resolve_nav_selectable, ComponentInstance, NavMode, NavPathEntry, NavRegion, NavSnapshot,
    WorkspaceNav,
};
pub use workspace::{
    ConfigSource, WorkspaceCapabilities, WorkspaceFsm, WorkspaceInputs, WorkspacePhase,
    WorkspaceSnapshot, WorkspaceSyncPlan, derive_workspace,
};
