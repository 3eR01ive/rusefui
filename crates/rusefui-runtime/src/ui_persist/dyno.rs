use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ComponentUiPersist;
use crate::dyno::DEFAULT_DYNO_CONFIG;

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
    #[serde(default)]
    pub settings_open: bool,
    #[serde(default = "default_chart_rpm_min")]
    pub chart_rpm_min: u16,
    #[serde(default = "default_chart_rpm_max")]
    pub chart_rpm_max: u16,
    #[serde(default = "default_chart_nm_min")]
    pub chart_nm_min: u16,
    #[serde(default = "default_chart_nm_max")]
    pub chart_nm_max: u16,
    #[serde(default = "default_chart_hp_min")]
    pub chart_hp_min: u16,
    #[serde(default = "default_chart_hp_max")]
    pub chart_hp_max: u16,

    // ---- Параметры расчёта (перенесены из настроек MCU в настройки компонента) ----
    #[serde(default = "d_rpm_step")]
    pub dyno_rpm_step: u8,
    #[serde(default = "d_sae_temp")]
    pub dyno_sae_temperature_c: i8,
    #[serde(default = "d_sae_humidity")]
    pub dyno_sae_relative_humidity: u8,
    #[serde(default = "d_sae_baro")]
    pub dyno_sae_baro: f32,
    #[serde(default = "d_wheel_dia")]
    pub dyno_car_wheel_dia_inch: i8,
    #[serde(default = "d_wheel_aspect")]
    pub dyno_car_wheel_aspect_ratio: i8,
    #[serde(default = "d_tire_width")]
    pub dyno_car_wheel_tire_width_mm: i16,
    #[serde(default = "d_primary_reduction")]
    pub dyno_car_gear_primary_reduction: f32,
    #[serde(default = "d_gear_ratio")]
    pub dyno_car_gear_ratio: f32,
    #[serde(default = "d_final_drive")]
    pub dyno_car_gear_final_drive: f32,
    #[serde(default = "d_car_mass")]
    pub dyno_car_car_mass_kg: i16,
    #[serde(default = "d_cargo_mass")]
    pub dyno_car_cargo_mass_kg: i16,
    #[serde(default = "d_coeff_drag")]
    pub dyno_car_coeff_of_drag: f32,
    #[serde(default = "d_frontal_area")]
    pub dyno_car_frontal_area_m2: f32,
}

fn d_rpm_step() -> u8 { DEFAULT_DYNO_CONFIG.dyno_rpm_step }
fn d_sae_temp() -> i8 { DEFAULT_DYNO_CONFIG.dyno_sae_temperature_c }
fn d_sae_humidity() -> u8 { DEFAULT_DYNO_CONFIG.dyno_sae_relative_humidity }
fn d_sae_baro() -> f32 { DEFAULT_DYNO_CONFIG.dyno_sae_baro }
fn d_wheel_dia() -> i8 { DEFAULT_DYNO_CONFIG.dyno_car_wheel_dia_inch }
fn d_wheel_aspect() -> i8 { DEFAULT_DYNO_CONFIG.dyno_car_wheel_aspect_ratio }
fn d_tire_width() -> i16 { DEFAULT_DYNO_CONFIG.dyno_car_wheel_tire_width_mm }
fn d_primary_reduction() -> f32 { DEFAULT_DYNO_CONFIG.dyno_car_gear_primary_reduction }
fn d_gear_ratio() -> f32 { DEFAULT_DYNO_CONFIG.dyno_car_gear_ratio }
fn d_final_drive() -> f32 { DEFAULT_DYNO_CONFIG.dyno_car_gear_final_drive }
fn d_car_mass() -> i16 { DEFAULT_DYNO_CONFIG.dyno_car_car_mass_kg }
fn d_cargo_mass() -> i16 { DEFAULT_DYNO_CONFIG.dyno_car_cargo_mass_kg }
fn d_coeff_drag() -> f32 { DEFAULT_DYNO_CONFIG.dyno_car_coeff_of_drag }
fn d_frontal_area() -> f32 { DEFAULT_DYNO_CONFIG.dyno_car_frontal_area_m2 }

fn default_chart_height() -> u32 {
    360
}

fn default_chart_rpm_min() -> u16 {
    0
}

fn default_chart_rpm_max() -> u16 {
    8000
}

fn default_chart_nm_min() -> u16 {
    0
}

fn default_chart_nm_max() -> u16 {
    1000
}

fn default_chart_hp_min() -> u16 {
    0
}

fn default_chart_hp_max() -> u16 {
    1000
}

impl Default for DynoUiSettings {
    fn default() -> Self {
        Self {
            ignore_tps_min: false,
            min_rpm: 0,
            smooth_strength: 0,
            chart_height: default_chart_height(),
            settings_open: false,
            chart_rpm_min: default_chart_rpm_min(),
            chart_rpm_max: default_chart_rpm_max(),
            chart_nm_min: default_chart_nm_min(),
            chart_nm_max: default_chart_nm_max(),
            chart_hp_min: default_chart_hp_min(),
            chart_hp_max: default_chart_hp_max(),
            dyno_rpm_step: d_rpm_step(),
            dyno_sae_temperature_c: d_sae_temp(),
            dyno_sae_relative_humidity: d_sae_humidity(),
            dyno_sae_baro: d_sae_baro(),
            dyno_car_wheel_dia_inch: d_wheel_dia(),
            dyno_car_wheel_aspect_ratio: d_wheel_aspect(),
            dyno_car_wheel_tire_width_mm: d_tire_width(),
            dyno_car_gear_primary_reduction: d_primary_reduction(),
            dyno_car_gear_ratio: d_gear_ratio(),
            dyno_car_gear_final_drive: d_final_drive(),
            dyno_car_car_mass_kg: d_car_mass(),
            dyno_car_cargo_mass_kg: d_cargo_mass(),
            dyno_car_coeff_of_drag: d_coeff_drag(),
            dyno_car_frontal_area_m2: d_frontal_area(),
        }
    }
}

fn normalize_axes(s: &mut DynoUiSettings) {
    if s.chart_rpm_max <= s.chart_rpm_min {
        s.chart_rpm_min = default_chart_rpm_min();
        s.chart_rpm_max = default_chart_rpm_max();
    }
    if s.chart_nm_max <= s.chart_nm_min {
        s.chart_nm_min = default_chart_nm_min();
        s.chart_nm_max = default_chart_nm_max();
    }
    if s.chart_hp_max <= s.chart_hp_min {
        s.chart_hp_min = default_chart_hp_min();
        s.chart_hp_max = default_chart_hp_max();
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
        normalize_axes(&mut s);
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
            settings_open: true,
            chart_rpm_min: 1000,
            chart_rpm_max: 8000,
            chart_nm_min: 0,
            chart_nm_max: 500,
            chart_hp_min: 0,
            chart_hp_max: 550,
            ..Default::default()
        })
        .unwrap();
        let normalized = p.parse(raw).unwrap();
        let back: DynoUiSettings = serde_json::from_value(normalized).unwrap();
        assert!(back.ignore_tps_min);
        assert_eq!(back.min_rpm, 2000);
        assert_eq!(back.smooth_strength, 5);
        assert_eq!(back.chart_height, 400);
        assert!(back.settings_open);
        assert_eq!(back.chart_rpm_min, 1000);
        assert_eq!(back.chart_rpm_max, 8000);
        assert_eq!(back.chart_nm_max, 500);
        assert_eq!(back.chart_hp_max, 550);
    }

    #[test]
    fn dyno_ui_normalizes_invalid_axes() {
        let p = DynoUiPersist;
        let raw = serde_json::json!({
            "chartRpmMin": 7000,
            "chartRpmMax": 1000,
        });
        let normalized = p.parse(raw).unwrap();
        let back: DynoUiSettings = serde_json::from_value(normalized).unwrap();
        assert_eq!(back.chart_rpm_min, default_chart_rpm_min());
        assert_eq!(back.chart_rpm_max, default_chart_rpm_max());
    }
}
