use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ComponentUiPersist;

pub const PERSIST_KEY_COMPOSITE_CHART: &str = "composite-chart";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositeChartUiSettings {
    /// Запускать trigger logger при подключении ECU.
    #[serde(default)]
    pub autostart: bool,
}

impl Default for CompositeChartUiSettings {
    fn default() -> Self {
        Self { autostart: false }
    }
}

pub struct CompositeChartUiPersist;

impl ComponentUiPersist for CompositeChartUiPersist {
    fn persist_key(&self) -> &'static str {
        PERSIST_KEY_COMPOSITE_CHART
    }

    fn default_value(&self) -> Value {
        serde_json::to_value(CompositeChartUiSettings::default()).unwrap()
    }

    fn parse(&self, value: Value) -> Result<Value, String> {
        let s: CompositeChartUiSettings =
            serde_json::from_value(value).map_err(|e| e.to_string())?;
        Ok(serde_json::to_value(s).map_err(|e| e.to_string())?)
    }
}
