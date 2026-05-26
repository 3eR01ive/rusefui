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
    /// Левый край графика (после подписей каналов) на линии TDC.
    #[serde(default)]
    pub align_tdc: bool,
    /// Авто-стоп через N с после «Старт»; 0 = выкл.
    #[serde(default)]
    pub auto_stop_sec: u32,
}

impl Default for CompositeChartUiSettings {
    fn default() -> Self {
        Self {
            autostart: false,
            align_tdc: false,
            auto_stop_sec: 0,
        }
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
