//! Реестр персистентных настроек UI компонентов в файле проекта.

mod composite_chart;
mod dyno;
mod knock;
mod output_chart;
mod simulation;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use composite_chart::{
    CompositeChartUiSettings, PERSIST_KEY_COMPOSITE_CHART,
};
pub use dyno::{DynoUiSettings, PERSIST_KEY_DYNO};
pub use knock::{KnockUiSettings, PERSIST_KEY_KNOCK};
pub use output_chart::{
    LogGraphGroupJson, LogRangeInputJson, LogUiSettings, PERSIST_KEY_OUTPUT_CHART,
};
pub use simulation::{RampCurveKind, SimulationUiSettings, PERSIST_KEY_SIMULATION};

/// Секции UI в JSON проекта: ключ = [`ComponentUiPersist::persist_key`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectUi {
    #[serde(default)]
    pub sections: HashMap<String, Value>,
}

pub trait ComponentUiPersist: Send + Sync {
    fn persist_key(&self) -> &'static str;
    fn default_value(&self) -> Value;
    fn parse(&self, value: Value) -> Result<Value, String>;
}

fn registry() -> &'static [&'static dyn ComponentUiPersist] {
    static ENTRIES: &[&dyn ComponentUiPersist] = &[
        &output_chart::OutputChartUiPersist,
        &composite_chart::CompositeChartUiPersist,
        &dyno::DynoUiPersist,
        &knock::KnockUiPersist,
        &simulation::SimulationUiPersist,
    ];
    ENTRIES
}

pub fn persist_keys() -> Vec<&'static str> {
    registry().iter().map(|e| e.persist_key()).collect()
}

fn find_entry(key: &str) -> Result<&'static dyn ComponentUiPersist, String> {
    registry()
        .iter()
        .copied()
        .find(|e| e.persist_key() == key)
        .ok_or_else(|| format!("Неизвестный persistKey: {key}"))
}

/// Заполнить все зарегистрированные секции значениями по умолчанию (новый проект).
pub fn init_document_ui(ui: &mut ProjectUi) {
    for entry in registry() {
        ui.sections
            .entry(entry.persist_key().to_string())
            .or_insert_with(|| entry.default_value());
    }
}

pub fn get(ui: &ProjectUi, key: &str) -> Result<Value, String> {
    let entry = find_entry(key)?;
    let raw = ui
        .sections
        .get(key)
        .ok_or_else(|| format!("В проекте нет секции ui.sections[{key:?}]"))?
        .clone();
    entry.parse(raw)
}

pub fn set(ui: &mut ProjectUi, key: &str, value: Value) -> Result<(), String> {
    let entry = find_entry(key)?;
    let normalized = entry.parse(value)?;
    ui.sections.insert(key.to_string(), normalized);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_chart_roundtrip() {
        let mut ui = ProjectUi::default();
        init_document_ui(&mut ui);
        let v = get(&ui, PERSIST_KEY_OUTPUT_CHART).unwrap();
        let mut parsed: LogUiSettings = serde_json::from_value(v).unwrap();
        parsed.zoom_step_pct = 15;
        set(
            &mut ui,
            PERSIST_KEY_OUTPUT_CHART,
            serde_json::to_value(&parsed).unwrap(),
        )
        .unwrap();
        let back: LogUiSettings =
            serde_json::from_value(get(&ui, PERSIST_KEY_OUTPUT_CHART).unwrap()).unwrap();
        assert_eq!(back.zoom_step_pct, 15);
    }

    #[test]
    fn unknown_key_errors() {
        let ui = ProjectUi::default();
        assert!(get(&ui, "nope").is_err());
    }
}
