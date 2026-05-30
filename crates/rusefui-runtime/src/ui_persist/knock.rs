use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ComponentUiPersist;

pub const PERSIST_KEY_KNOCK: &str = "knock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnockUiSettings {
    #[serde(default = "default_ignore_tps_min")]
    pub ignore_tps_min: bool,
    #[serde(default = "default_min_rpm")]
    pub min_rpm: u16,
    #[serde(default = "default_cutoff_rpm")]
    pub cutoff_rpm: u16,
    #[serde(default = "default_threshold_gap")]
    pub threshold_gap_db: f64,
    #[serde(default = "default_temp_lambda")]
    pub temp_target_lambda: f64,
    #[serde(default = "default_temp_retard")]
    pub temp_ignition_retard_deg: f64,
    #[serde(default = "default_momentum_rpm_min")]
    pub momentum_safe_rpm_min: u16,
    #[serde(default = "default_momentum_rpm_max")]
    pub momentum_safe_rpm_max: u16,
    #[serde(default = "default_momentum_load")]
    pub momentum_min_load: f64,
    #[serde(default = "default_momentum_advance")]
    pub momentum_advance_add_deg: f64,
    #[serde(default = "default_momentum_duration")]
    pub momentum_duration_ms: u32,
    #[serde(default = "default_spectrogram_window")]
    pub spectrogram_window_ms: u32,
    #[serde(default = "default_spectrogram_autocontrast")]
    pub spectrogram_autocontrast: bool,
    #[serde(default = "default_spectrogram_gain")]
    pub spectrogram_gain_percent: u32,
    #[serde(default = "default_chart_height")]
    pub chart_height: u32,
    #[serde(default)]
    pub settings_open: bool,
}

fn default_ignore_tps_min() -> bool {
    true
}

fn default_min_rpm() -> u16 {
    800
}
fn default_cutoff_rpm() -> u16 {
    6500
}
fn default_threshold_gap() -> f64 {
    3.0
}
fn default_temp_lambda() -> f64 {
    0.85
}
fn default_temp_retard() -> f64 {
    8.0
}
fn default_momentum_rpm_min() -> u16 {
    2000
}
fn default_momentum_rpm_max() -> u16 {
    3500
}
fn default_momentum_load() -> f64 {
    40.0
}
fn default_momentum_advance() -> f64 {
    6.0
}
fn default_momentum_duration() -> u32 {
    800
}
fn default_spectrogram_window() -> u32 {
    500
}
fn default_spectrogram_autocontrast() -> bool {
    true
}
fn default_spectrogram_gain() -> u32 {
    100
}
fn default_chart_height() -> u32 {
    360
}

impl Default for KnockUiSettings {
    fn default() -> Self {
        Self {
            ignore_tps_min: true,
            min_rpm: default_min_rpm(),
            cutoff_rpm: default_cutoff_rpm(),
            threshold_gap_db: default_threshold_gap(),
            temp_target_lambda: default_temp_lambda(),
            temp_ignition_retard_deg: default_temp_retard(),
            momentum_safe_rpm_min: default_momentum_rpm_min(),
            momentum_safe_rpm_max: default_momentum_rpm_max(),
            momentum_min_load: default_momentum_load(),
            momentum_advance_add_deg: default_momentum_advance(),
            momentum_duration_ms: default_momentum_duration(),
            spectrogram_window_ms: default_spectrogram_window(),
            spectrogram_autocontrast: default_spectrogram_autocontrast(),
            spectrogram_gain_percent: default_spectrogram_gain(),
            chart_height: default_chart_height(),
            settings_open: false,
        }
    }
}

pub struct KnockUiPersist;

impl ComponentUiPersist for KnockUiPersist {
    fn persist_key(&self) -> &'static str {
        PERSIST_KEY_KNOCK
    }

    fn default_value(&self) -> Value {
        serde_json::to_value(KnockUiSettings::default()).expect("KnockUiSettings serializes")
    }

    fn parse(&self, value: Value) -> Result<Value, String> {
        let settings: KnockUiSettings =
            serde_json::from_value(value).map_err(|e| format!("{PERSIST_KEY_KNOCK}: {e}"))?;
        let mut s = settings;
        if s.chart_height < 180 {
            s.chart_height = default_chart_height();
        }
        if s.momentum_duration_ms == 0 {
            s.momentum_duration_ms = default_momentum_duration();
        }
        if s.spectrogram_window_ms < 50 {
            s.spectrogram_window_ms = default_spectrogram_window();
        }
        if s.spectrogram_gain_percent < 1 {
            s.spectrogram_gain_percent = default_spectrogram_gain();
        } else if s.spectrogram_gain_percent > 400 {
            s.spectrogram_gain_percent = 400;
        }
        serde_json::to_value(s).map_err(|e| format!("{PERSIST_KEY_KNOCK}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knock_ui_roundtrip() {
        let p = KnockUiPersist;
        let raw = serde_json::to_value(KnockUiSettings {
            cutoff_rpm: 7000,
            settings_open: true,
            ..Default::default()
        })
        .unwrap();
        let normalized = p.parse(raw).unwrap();
        let back: KnockUiSettings = serde_json::from_value(normalized).unwrap();
        assert_eq!(back.cutoff_rpm, 7000);
        assert!(back.settings_open);
    }
}
