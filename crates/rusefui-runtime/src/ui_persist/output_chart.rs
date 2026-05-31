use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ComponentUiPersist;

pub const PERSIST_KEY_OUTPUT_CHART: &str = "output-chart";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogGraphGroupJson {
    pub id: String,
    pub field_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRangeInputJson {
    pub min: String,
    pub max: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogUiSettings {
    #[serde(default = "default_window_seconds")]
    pub window_seconds: u32,
    #[serde(default = "default_chart_height")]
    pub chart_height: u32,
    #[serde(default = "default_zoom_step")]
    pub zoom_step_pct: u8,
    #[serde(default)]
    pub settings_expanded: bool,
    #[serde(default)]
    pub graph_groups: Vec<LogGraphGroupJson>,
    #[serde(default = "default_active_graph")]
    pub active_graph_id: String,
    #[serde(default)]
    pub range_inputs: HashMap<String, LogRangeInputJson>,
    #[serde(default)]
    pub follow_live: bool,
    /// 0 — взять `window_seconds` (legacy / только окно из props).
    #[serde(default)]
    pub span_sec: f64,
}

fn default_window_seconds() -> u32 {
    30
}
fn default_chart_height() -> u32 {
    220
}
fn default_zoom_step() -> u8 {
    10
}
fn default_active_graph() -> String {
    "g1".into()
}

impl Default for LogUiSettings {
    fn default() -> Self {
        Self {
            window_seconds: 30,
            chart_height: 220,
            zoom_step_pct: 10,
            settings_expanded: false,
            graph_groups: vec![LogGraphGroupJson {
                id: "g1".into(),
                field_names: vec!["RPMValue".into(), "coolant".into()],
            }],
            active_graph_id: "g1".into(),
            range_inputs: HashMap::new(),
            follow_live: true,
            span_sec: 0.0,
        }
    }
}

pub struct OutputChartUiPersist;

impl ComponentUiPersist for OutputChartUiPersist {
    fn persist_key(&self) -> &'static str {
        PERSIST_KEY_OUTPUT_CHART
    }

    fn default_value(&self) -> Value {
        serde_json::to_value(LogUiSettings::default()).expect("LogUiSettings serializes")
    }

    fn parse(&self, value: Value) -> Result<Value, String> {
        let settings: LogUiSettings = serde_json::from_value(value)
            .map_err(|e| format!("{PERSIST_KEY_OUTPUT_CHART}: {e}"))?;
        serde_json::to_value(settings).map_err(|e| format!("{PERSIST_KEY_OUTPUT_CHART}: {e}"))
    }
}
