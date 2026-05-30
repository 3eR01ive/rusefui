use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ComponentUiPersist;

pub const PERSIST_KEY_SIMULATION: &str = "simulation";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum RampCurveKind {
    #[serde(rename = "linear")]
    Linear,
    #[serde(rename = "smooth")]
    Smooth,
}

impl Default for RampCurveKind {
    fn default() -> Self {
        Self::Linear
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationUiSettings {
    #[serde(default = "default_target_rpm")]
    pub target_rpm: u16,
    #[serde(default = "default_idle_rpm")]
    pub idle_rpm: u16,
    #[serde(default = "default_peak_rpm")]
    pub peak_rpm: u16,
    #[serde(default = "default_ramp_up_sec")]
    pub ramp_up_sec: f32,
    #[serde(default = "default_ramp_down_sec")]
    pub ramp_down_sec: f32,
    #[serde(default)]
    pub ramp_curve: RampCurveKind,
    /// Развёрнута ли панель расширенных настроек.
    #[serde(default)]
    pub settings_open: bool,
}

fn default_target_rpm() -> u16 {
    1500
}

fn default_idle_rpm() -> u16 {
    800
}

fn default_peak_rpm() -> u16 {
    4500
}

fn default_ramp_up_sec() -> f32 {
    4.0
}

fn default_ramp_down_sec() -> f32 {
    4.0
}

impl Default for SimulationUiSettings {
    fn default() -> Self {
        Self {
            target_rpm: default_target_rpm(),
            idle_rpm: default_idle_rpm(),
            peak_rpm: default_peak_rpm(),
            ramp_up_sec: default_ramp_up_sec(),
            ramp_down_sec: default_ramp_down_sec(),
            ramp_curve: RampCurveKind::Linear,
            settings_open: false,
        }
    }
}

fn clamp_rpm(v: u16, min: u16, max: u16) -> u16 {
    v.clamp(min, max)
}

fn clamp_ramp_sec(v: f32) -> f32 {
    v.clamp(0.1, 120.0)
}

pub struct SimulationUiPersist;

impl ComponentUiPersist for SimulationUiPersist {
    fn persist_key(&self) -> &'static str {
        PERSIST_KEY_SIMULATION
    }

    fn default_value(&self) -> Value {
        serde_json::to_value(SimulationUiSettings::default()).expect("SimulationUiSettings serializes")
    }

    fn parse(&self, value: Value) -> Result<Value, String> {
        let mut s: SimulationUiSettings =
            serde_json::from_value(value).map_err(|e| format!("{PERSIST_KEY_SIMULATION}: {e}"))?;
        const RPM_MIN: u16 = 0;
        const RPM_MAX: u16 = 30_000;
        s.target_rpm = clamp_rpm(s.target_rpm, RPM_MIN, RPM_MAX);
        s.idle_rpm = clamp_rpm(s.idle_rpm, RPM_MIN, RPM_MAX);
        s.peak_rpm = clamp_rpm(s.peak_rpm, RPM_MIN, RPM_MAX);
        s.ramp_up_sec = clamp_ramp_sec(s.ramp_up_sec);
        s.ramp_down_sec = clamp_ramp_sec(s.ramp_down_sec);
        serde_json::to_value(s).map_err(|e| format!("{PERSIST_KEY_SIMULATION}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulation_ui_roundtrip() {
        let p = SimulationUiPersist;
        let raw = serde_json::to_value(SimulationUiSettings {
            target_rpm: 2000,
            idle_rpm: 900,
            peak_rpm: 6000,
            ramp_up_sec: 3.5,
            ramp_down_sec: 2.0,
            ramp_curve: RampCurveKind::Smooth,
            settings_open: true,
        })
        .unwrap();
        let normalized = p.parse(raw).unwrap();
        let back: SimulationUiSettings = serde_json::from_value(normalized).unwrap();
        assert_eq!(back.target_rpm, 2000);
        assert_eq!(back.peak_rpm, 6000);
        assert!(back.settings_open);
    }
}
