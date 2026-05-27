use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ComponentUiPersist;

pub const PERSIST_KEY_DYNO: &str = "dyno";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynoUiSettings {
    #[serde(default)]
    pub ignore_tps_min: bool,
    #[serde(default)]
    pub min_rpm: u16,
    #[serde(default)]
    pub smooth_strength: u8,
    #[serde(default = "default_chart_height")]
    pub chart_height: u32,
}

fn default_chart_height() -> u32 {
    360
}

impl Default for DynoUiSettings {
    fn default() -> Self {
        Self {
            ignore_tps_min: false,
            min_rpm: 0,
            smooth_strength: 0,
            chart_height: default_chart_height(),
        }
    }
}

pub struct DynoUiPersist;

impl ComponentUiPersist for DynoUiPersist {
    fn persist_key(&self) -> &'static str {
        PERSIST_KEY_DYNO
    }

    fn default_value(&self) -> Value {
        serde_json::to_value(DynoUiSettings::default()).expect("DynoUiSettings serializes")
    }

    fn parse(&self, value: Value) -> Result<Value, String> {
        let settings: DynoUiSettings =
            serde_json::from_value(value).map_err(|e| format!("{PERSIST_KEY_DYNO}: {e}"))?;
        let mut s = settings;
        if s.chart_height < 180 {
            s.chart_height = default_chart_height();
        }
        if s.smooth_strength > 20 {
            s.smooth_strength = 20;
        }
        serde_json::to_value(s).map_err(|e| format!("{PERSIST_KEY_DYNO}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dyno_ui_roundtrip() {
        let p = DynoUiPersist;
        let raw = serde_json::to_value(DynoUiSettings {
            ignore_tps_min: true,
            min_rpm: 2000,
            smooth_strength: 5,
            chart_height: 400,
        })
        .unwrap();
        let normalized = p.parse(raw).unwrap();
        let back: DynoUiSettings = serde_json::from_value(normalized).unwrap();
        assert!(back.ignore_tps_min);
        assert_eq!(back.min_rpm, 2000);
        assert_eq!(back.smooth_strength, 5);
        assert_eq!(back.chart_height, 400);
    }
}
