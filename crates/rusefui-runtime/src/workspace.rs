//! Состояние рабочего места: проект × ECU × источник config.
//!
//! Единая модель вместо разрозненных `if connected / read_only / has_path`.

use serde::Serialize;

use crate::autoconnect::AutoConnectSnapshot;
use crate::project::ProjectInfo;
use crate::sources::config::ConfigSnapshot;

/// Высокоуровневая фаза UI (одна «кухня» для гейта и подсказок).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspacePhase {
    /// Нет файла проекта — только экран создания/открытия.
    Gate,
    /// Проект открыт; ECU недоступна (offline или не подключена).
    ProjectOnly,
    /// Проект открыт; autoconnect ищет порт.
    EcuScanning,
    /// ECU подключена, но signature не совпала ни с одним известным INI —
    /// ждём пользовательский выбор/загрузку.
    EcuIniMismatch,
    /// ECU подключена; config ещё не готов.
    EcuConnectedIdle,
    /// Активен снимок `ecuConfig` из файла проекта (offline-редактирование).
    ConfigFromProject,
    /// Идёт чтение page 0 с ECU.
    ConfigLoadingFromEcu,
    /// Активен config с ECU (live RAM).
    ConfigFromEcu,
}

/// Откуда берутся значения полей config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfigSource {
    None,
    ProjectFile,
    EcuLive,
}

/// Что разрешено в текущей фазе.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCapabilities {
    pub show_main_ui: bool,
    pub edit_project_config: bool,
    pub write_config_to_ecu: bool,
    pub burn_to_flash: bool,
    pub poll_output_channels: bool,
    pub start_composite_logger: bool,
}

impl WorkspaceCapabilities {
    pub const fn gate() -> Self {
        Self {
            show_main_ui: false,
            edit_project_config: false,
            write_config_to_ecu: false,
            burn_to_flash: false,
            poll_output_channels: false,
            start_composite_logger: false,
        }
    }
}

/// Снимок для UI и Tauri (`workspace-state`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub phase: WorkspacePhase,
    pub project_path: Option<String>,
    pub project_name: String,
    pub project_dirty: bool,
    pub has_ecu_config_in_project: bool,
    pub offline_mode: bool,
    pub ecu_connected: bool,
    pub ecu_scanning: bool,
    /// Ждём от пользователя выбора INI (signature ECU не совпала с .ini).
    pub ini_pending_resolution: bool,
    pub config_source: ConfigSource,
    pub config_loaded: bool,
    pub config_loading: bool,
    /// RAM ECU изменена, flash ещё не записан (команда B).
    pub burn_pending: bool,
    pub capabilities: WorkspaceCapabilities,
}

#[derive(Debug, Clone)]
pub struct WorkspaceInputs {
    pub project: ProjectInfo,
    pub autoconnect: AutoConnectSnapshot,
    pub ecu_connected: bool,
    pub ini_pending_resolution: bool,
    pub config: ConfigSnapshot,
}

impl WorkspaceInputs {
    pub fn derive(&self) -> WorkspaceSnapshot {
        derive_workspace(self)
    }
}

/// Действия синхронизации железа/UI при смене фазы.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkspaceSyncPlan {
    pub stop_config: bool,
    pub stop_output_poll: bool,
    pub stop_composite: bool,
    pub clear_config_diff: bool,
    pub start_ecu_config_load: bool,
    pub start_output_poll: bool,
}

pub struct WorkspaceFsm {
    last: Option<WorkspaceSnapshot>,
}

impl WorkspaceFsm {
    pub fn new() -> Self {
        Self { last: None }
    }

    /// Пересчитать фазу и план действий при изменении входов.
    pub fn reconcile(
        &mut self,
        inputs: &WorkspaceInputs,
    ) -> (WorkspaceSnapshot, WorkspaceSyncPlan, bool) {
        let next = inputs.derive();
        let changed = self.last.as_ref() != Some(&next);
        let plan = if changed {
            transition_plan(self.last.as_ref(), &next)
        } else {
            WorkspaceSyncPlan::default()
        };
        if changed {
            self.last = Some(next.clone());
        }
        (next, plan, changed)
    }

    pub fn snapshot(&self) -> Option<&WorkspaceSnapshot> {
        self.last.as_ref()
    }

    pub fn reset(&mut self) {
        self.last = None;
    }
}

pub fn derive_workspace(inputs: &WorkspaceInputs) -> WorkspaceSnapshot {
    let project_open = inputs.project.path.is_some();
    let ecu_live = inputs.ecu_connected && !inputs.autoconnect.offline_mode;
    let ini_pending = inputs.ini_pending_resolution && ecu_live;

    if !project_open {
        // Autoconnect может подключить ECU до открытия .json — всё равно показываем выбор INI.
        if ini_pending {
            return WorkspaceSnapshot {
                phase: WorkspacePhase::EcuIniMismatch,
                project_path: None,
                project_name: inputs.project.name.clone(),
                project_dirty: inputs.project.dirty,
                has_ecu_config_in_project: false,
                offline_mode: inputs.autoconnect.offline_mode,
                ecu_connected: true,
                ecu_scanning: false,
                ini_pending_resolution: true,
                config_source: ConfigSource::None,
                config_loaded: false,
                config_loading: false,
                burn_pending: false,
                capabilities: capabilities_for_phase(WorkspacePhase::EcuIniMismatch, inputs),
            };
        }
        return WorkspaceSnapshot {
            phase: WorkspacePhase::Gate,
            project_path: None,
            project_name: inputs.project.name.clone(),
            project_dirty: inputs.project.dirty,
            has_ecu_config_in_project: inputs.project.has_ecu_config,
            offline_mode: inputs.autoconnect.offline_mode,
            ecu_connected: false,
            ecu_scanning: false,
            ini_pending_resolution: false,
            config_source: ConfigSource::None,
            config_loaded: false,
            config_loading: false,
            burn_pending: false,
            capabilities: WorkspaceCapabilities::gate(),
        };
    }

    let config_source = config_source_from_snapshot(&inputs.config);
    let phase = phase_from_inputs(inputs, config_source);
    let capabilities = capabilities_for_phase(phase, inputs);

    WorkspaceSnapshot {
        phase,
        project_path: inputs.project.path.clone(),
        project_name: inputs.project.name.clone(),
        project_dirty: inputs.project.dirty,
        has_ecu_config_in_project: inputs.project.has_ecu_config,
        offline_mode: inputs.autoconnect.offline_mode,
        ecu_connected: inputs.ecu_connected && !inputs.autoconnect.offline_mode,
        ecu_scanning: inputs.autoconnect.scanning && !inputs.autoconnect.offline_mode,
        ini_pending_resolution: inputs.ini_pending_resolution
            && inputs.ecu_connected
            && !inputs.autoconnect.offline_mode,
        config_source,
        config_loaded: inputs.config.loaded,
        config_loading: inputs.config.loading,
        burn_pending: false,
        capabilities,
    }
}

fn config_source_from_snapshot(snap: &ConfigSnapshot) -> ConfigSource {
    if !snap.loaded {
        return ConfigSource::None;
    }
    if snap.read_only {
        ConfigSource::ProjectFile
    } else {
        ConfigSource::EcuLive
    }
}

fn phase_from_inputs(inputs: &WorkspaceInputs, config_source: ConfigSource) -> WorkspacePhase {
    if inputs.autoconnect.offline_mode || !inputs.ecu_connected {
        if config_source == ConfigSource::ProjectFile {
            return WorkspacePhase::ConfigFromProject;
        }
        if inputs.autoconnect.scanning && !inputs.autoconnect.offline_mode {
            return WorkspacePhase::EcuScanning;
        }
        return WorkspacePhase::ProjectOnly;
    }

    // ECU подключена. Если INI ещё не выбран — приоритет у Mismatch.
    if inputs.ini_pending_resolution {
        return WorkspacePhase::EcuIniMismatch;
    }

    match config_source {
        // Снимок из проекта при live ECU — только preview, нужна загрузка page 0 с блока.
        ConfigSource::ProjectFile => WorkspacePhase::EcuConnectedIdle,
        ConfigSource::EcuLive => WorkspacePhase::ConfigFromEcu,
        ConfigSource::None if inputs.config.loading => WorkspacePhase::ConfigLoadingFromEcu,
        ConfigSource::None => WorkspacePhase::EcuConnectedIdle,
    }
}

fn capabilities_for_phase(phase: WorkspacePhase, inputs: &WorkspaceInputs) -> WorkspaceCapabilities {
    match phase {
        WorkspacePhase::Gate => WorkspaceCapabilities::gate(),
        WorkspacePhase::ProjectOnly => WorkspaceCapabilities {
            show_main_ui: true,
            edit_project_config: inputs.project.has_ecu_config,
            write_config_to_ecu: false,
            burn_to_flash: false,
            poll_output_channels: false,
            start_composite_logger: false,
        },
        WorkspacePhase::EcuScanning => WorkspaceCapabilities {
            show_main_ui: true,
            edit_project_config: config_source_from_snapshot(&inputs.config)
                == ConfigSource::ProjectFile,
            write_config_to_ecu: false,
            burn_to_flash: false,
            poll_output_channels: false,
            start_composite_logger: false,
        },
        WorkspacePhase::EcuIniMismatch => WorkspaceCapabilities {
            // Главный UI скрыт, поверх — модалка выбора INI; редактирование/чтение запрещено.
            show_main_ui: false,
            edit_project_config: false,
            write_config_to_ecu: false,
            burn_to_flash: false,
            poll_output_channels: false,
            start_composite_logger: false,
        },
        WorkspacePhase::EcuConnectedIdle => WorkspaceCapabilities {
            show_main_ui: true,
            edit_project_config: inputs.project.has_ecu_config,
            write_config_to_ecu: false,
            burn_to_flash: false,
            poll_output_channels: false,
            start_composite_logger: false,
        },
        WorkspacePhase::ConfigFromProject => WorkspaceCapabilities {
            show_main_ui: true,
            edit_project_config: true,
            write_config_to_ecu: false,
            burn_to_flash: false,
            poll_output_channels: inputs.ecu_connected && !inputs.autoconnect.offline_mode,
            start_composite_logger: inputs.ecu_connected && !inputs.autoconnect.offline_mode,
        },
        WorkspacePhase::ConfigLoadingFromEcu => WorkspaceCapabilities {
            show_main_ui: true,
            edit_project_config: false,
            write_config_to_ecu: false,
            burn_to_flash: false,
            poll_output_channels: false,
            start_composite_logger: false,
        },
        WorkspacePhase::ConfigFromEcu => WorkspaceCapabilities {
            show_main_ui: true,
            edit_project_config: false,
            write_config_to_ecu: true,
            burn_to_flash: true,
            poll_output_channels: true,
            start_composite_logger: true,
        },
    }
}

fn transition_plan(
    prev: Option<&WorkspaceSnapshot>,
    next: &WorkspaceSnapshot,
) -> WorkspaceSyncPlan {
    let mut plan = WorkspaceSyncPlan::default();

    let prev_phase = prev.map(|p| p.phase);
    let next_phase = next.phase;

    if prev_phase == Some(next_phase) {
        return plan;
    }

    match next_phase {
        WorkspacePhase::Gate => {
            plan.stop_config = prev
                .map(|p| p.config_source != ConfigSource::None)
                .unwrap_or(true);
            plan.stop_output_poll = true;
            plan.stop_composite = true;
            plan.clear_config_diff = true;
        }
        WorkspacePhase::ProjectOnly | WorkspacePhase::EcuScanning => {
            if prev
                .map(|p| p.config_source == ConfigSource::EcuLive)
                .unwrap_or(false)
            {
                plan.stop_config = true;
                plan.clear_config_diff = true;
            }
            plan.stop_output_poll = true;
            plan.stop_composite = true;
        }
        WorkspacePhase::EcuIniMismatch => {
            // Mismatch: link жив, но INI не применён — глушим все источники, ждём пользователя.
            plan.stop_config = prev
                .map(|p| p.config_source != ConfigSource::None)
                .unwrap_or(false);
            plan.stop_output_poll = true;
            plan.stop_composite = true;
            plan.clear_config_diff = true;
        }
        WorkspacePhase::EcuConnectedIdle => {
            if next.ecu_connected {
                plan.start_ecu_config_load = true;
            }
            plan.stop_output_poll = true;
            plan.stop_composite = true;
        }
        WorkspacePhase::ConfigFromProject => {
            plan.stop_output_poll = !next.capabilities.poll_output_channels;
            plan.stop_composite = !next.capabilities.start_composite_logger;
        }
        WorkspacePhase::ConfigLoadingFromEcu => {
            plan.stop_output_poll = true;
            plan.stop_composite = true;
        }
        WorkspacePhase::ConfigFromEcu => {
            plan.start_output_poll = true;
        }
    }

    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(
        path: Option<&str>,
        offline: bool,
        connected: bool,
        config: ConfigSnapshot,
    ) -> WorkspaceInputs {
        WorkspaceInputs {
            project: ProjectInfo {
                path: path.map(str::to_string),
                name: "P".into(),
                dirty: false,
                log_count: 0,
                timeline_clip_count: 0,
                has_ecu_config: path.is_some(),
            },
            autoconnect: AutoConnectSnapshot {
                offline_mode: offline,
                scanning: false,
                auto_connect_enabled: true,
                last_error: None,
                candidate_ports: vec![],
                busy_ports: vec![],
            },
            ecu_connected: connected,
            ini_pending_resolution: false,
            config,
        }
    }

    #[test]
    fn gate_without_project() {
        let snap = inputs(None, false, false, disconnected_config());
        assert_eq!(snap.derive().phase, WorkspacePhase::Gate);
    }

    #[test]
    fn project_config_when_offline() {
        let mut cfg = disconnected_config();
        cfg.loaded = true;
        cfg.read_only = true;
        let snap = inputs(Some("/p.json"), true, false, cfg);
        assert_eq!(snap.derive().phase, WorkspacePhase::ConfigFromProject);
        assert!(snap.derive().capabilities.edit_project_config);
    }

    #[test]
    fn project_preview_with_live_ecu_starts_ecu_load() {
        let mut cfg = disconnected_config();
        cfg.loaded = true;
        cfg.read_only = true;
        let snap = inputs(Some("/p.json"), false, true, cfg);
        assert_eq!(snap.derive().phase, WorkspacePhase::EcuConnectedIdle);
        assert!(!snap.derive().capabilities.burn_to_flash);
    }

    #[test]
    fn ini_pending_resolution_overrides_other_phases() {
        // ECU подключена, config был preview — фаза должна стать EcuIniMismatch.
        let mut cfg = disconnected_config();
        cfg.loaded = true;
        cfg.read_only = true;
        let mut inp = inputs(Some("/p.json"), false, true, cfg);
        inp.ini_pending_resolution = true;
        let snap = inp.derive();
        assert_eq!(snap.phase, WorkspacePhase::EcuIniMismatch);
        assert!(!snap.capabilities.show_main_ui);
        assert!(!snap.capabilities.poll_output_channels);
        assert!(snap.ini_pending_resolution);
    }

    #[test]
    fn ini_pending_ignored_when_offline_or_disconnected() {
        let mut inp_off = inputs(Some("/p.json"), true, false, disconnected_config());
        inp_off.ini_pending_resolution = true;
        // Offline / disconnected — Mismatch недостижим.
        assert_ne!(inp_off.derive().phase, WorkspacePhase::EcuIniMismatch);
        assert!(!inp_off.derive().ini_pending_resolution);
    }

    #[test]
    fn ini_pending_without_project_shows_mismatch_not_gate() {
        let mut inp = inputs(None, false, true, disconnected_config());
        inp.ini_pending_resolution = true;
        let snap = inp.derive();
        assert_eq!(snap.phase, WorkspacePhase::EcuIniMismatch);
        assert!(snap.ini_pending_resolution);
        assert!(snap.ecu_connected);
    }

    fn disconnected_config() -> ConfigSnapshot {
        let ini = crate::sources::output_channels::IniContext::disconnected();
        ConfigSnapshot::disconnected(&ini)
    }
}
