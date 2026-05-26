use std::collections::HashMap;

const DYNO_VIEW_WINDOW_SIZE: usize = 7;
const DYNO_VIEW_WINDOW_SIZE_RPM: usize = 10;
const DYNO_VIEW_TPS_MIN_FOR_RUN: f64 = 30.0;
const DYNO_VIEW_RPM_DIFF_SMOOTH: i32 = 30;
const DYNO_VIEW_LOG_TIME_SMOOTH_SEC: f64 = 0.05;
const DYNO_VIEW_TPS_DIFF_TO_RESET_RUN: f64 = 10.0;
const DYNO_VIEW_RPM_FALL_TO_RESET_RUN: i32 = 60;

#[derive(Debug, Clone, Copy)]
pub struct DynoConfig {
    pub dyno_rpm_step: u8,
    pub dyno_sae_temperature_c: i8,
    pub dyno_sae_relative_humidity: u8,
    pub dyno_sae_baro: f32,
    pub dyno_car_wheel_dia_inch: i8,
    pub dyno_car_wheel_aspect_ratio: i8,
    pub dyno_car_wheel_tire_width_mm: i16,
    pub dyno_car_gear_primary_reduction: f32,
    pub dyno_car_gear_ratio: f32,
    pub dyno_car_gear_final_drive: f32,
    pub dyno_car_car_mass_kg: i16,
    pub dyno_car_cargo_mass_kg: i16,
    pub dyno_car_coeff_of_drag: f32,
    pub dyno_car_frontal_area_m2: f32,
}

pub const DEFAULT_DYNO_CONFIG: DynoConfig = DynoConfig {
    dyno_rpm_step: 100,
    dyno_sae_temperature_c: 20,
    dyno_sae_relative_humidity: 80,
    dyno_sae_baro: 101.33,
    dyno_car_wheel_dia_inch: 18,
    dyno_car_wheel_aspect_ratio: 55,
    dyno_car_wheel_tire_width_mm: 180,
    dyno_car_gear_primary_reduction: 1.0,
    dyno_car_gear_ratio: 1.0,
    dyno_car_gear_final_drive: 3.5,
    dyno_car_car_mass_kg: 1200,
    dyno_car_cargo_mass_kg: 80,
    dyno_car_coeff_of_drag: 0.32,
    dyno_car_frontal_area_m2: 2.2,
};

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynoRunPoint {
    pub rpm: i32,
    pub torque_nm: f64,
    pub hp: f64,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynoRunOptions {
    pub ignore_tps_min: bool,
    pub min_rpm: u16,
}

impl Default for DynoRunOptions {
    fn default() -> Self {
        Self {
            ignore_tps_min: false,
            min_rpm: 0,
        }
    }
}

pub const DEFAULT_DYNO_RUN_OPTIONS: DynoRunOptions = DynoRunOptions {
    ignore_tps_min: false,
    min_rpm: 0,
};

#[derive(Debug, Clone, Copy)]
struct DynoPoint {
    rpm: i32,
    time: f64,
    tps: f64,
    v_ms: f64,
}

impl DynoPoint {
    const EMPTY: Self = Self {
        rpm: -1,
        time: -1.0,
        tps: -1.0,
        v_ms: 0.0,
    };
}

fn move_window(size: usize, data: &mut [f64; DYNO_VIEW_WINDOW_SIZE_RPM]) {
    for i in (1..size).rev() {
        data[i] = data[i - 1];
    }
}

fn move_window_small(data: &mut [f64; DYNO_VIEW_WINDOW_SIZE]) {
    for i in (1..DYNO_VIEW_WINDOW_SIZE).rev() {
        data[i] = data[i - 1];
    }
}

fn accumulate_window(size: usize, data: &[f64]) -> f64 {
    let mut sum = 0.0;
    for i in 0..size {
        sum += data[size - i - 1];
    }
    sum / size as f64
}

pub fn dyno_config_from_values(values: &HashMap<String, f64>) -> DynoConfig {
    fn num(values: &HashMap<String, f64>, key: &str, default: f64) -> f64 {
        values.get(key).copied().filter(|v| v.is_finite()).unwrap_or(default)
    }

    DynoConfig {
        dyno_rpm_step: num(values, "dynoRpmStep", DEFAULT_DYNO_CONFIG.dyno_rpm_step as f64) as u8,
        dyno_sae_temperature_c: num(
            values,
            "dynoSaeTemperatureC",
            DEFAULT_DYNO_CONFIG.dyno_sae_temperature_c as f64,
        ) as i8,
        dyno_sae_relative_humidity: num(
            values,
            "dynoSaeRelativeHumidity",
            DEFAULT_DYNO_CONFIG.dyno_sae_relative_humidity as f64,
        ) as u8,
        dyno_sae_baro: num(values, "dynoSaeBaro", DEFAULT_DYNO_CONFIG.dyno_sae_baro as f64) as f32,
        dyno_car_wheel_dia_inch: num(
            values,
            "dynoCarWheelDiaInch",
            DEFAULT_DYNO_CONFIG.dyno_car_wheel_dia_inch as f64,
        ) as i8,
        dyno_car_wheel_aspect_ratio: num(
            values,
            "dynoCarWheelAspectRatio",
            DEFAULT_DYNO_CONFIG.dyno_car_wheel_aspect_ratio as f64,
        ) as i8,
        dyno_car_wheel_tire_width_mm: num(
            values,
            "dynoCarWheelTireWidthMm",
            DEFAULT_DYNO_CONFIG.dyno_car_wheel_tire_width_mm as f64,
        ) as i16,
        dyno_car_gear_primary_reduction: num(
            values,
            "dynoCarGearPrimaryReduction",
            DEFAULT_DYNO_CONFIG.dyno_car_gear_primary_reduction as f64,
        ) as f32,
        dyno_car_gear_ratio: num(
            values,
            "dynoCarGearRatio",
            DEFAULT_DYNO_CONFIG.dyno_car_gear_ratio as f64,
        ) as f32,
        dyno_car_gear_final_drive: num(
            values,
            "dynoCarGearFinalDrive",
            DEFAULT_DYNO_CONFIG.dyno_car_gear_final_drive as f64,
        ) as f32,
        dyno_car_car_mass_kg: num(
            values,
            "dynoCarCarMassKg",
            DEFAULT_DYNO_CONFIG.dyno_car_car_mass_kg as f64,
        ) as i16,
        dyno_car_cargo_mass_kg: num(
            values,
            "dynoCarCargoMassKg",
            DEFAULT_DYNO_CONFIG.dyno_car_cargo_mass_kg as f64,
        ) as i16,
        dyno_car_coeff_of_drag: num(
            values,
            "dynoCarCoeffOfDrag",
            DEFAULT_DYNO_CONFIG.dyno_car_coeff_of_drag as f64,
        ) as f32,
        dyno_car_frontal_area_m2: num(
            values,
            "dynoCarFrontalAreaM2",
            DEFAULT_DYNO_CONFIG.dyno_car_frontal_area_m2 as f64,
        ) as f32,
    }
}

/// Порт `DynoView` из virtualdyno-c++.
pub struct DynoView {
    config: DynoConfig,
    run_options: DynoRunOptions,
    air_density_kg_m3: f64,
    wheel_overall_diameter_mm: u16,
    sae_correction_factor: f64,
    point: DynoPoint,
    point_prev: DynoPoint,
    count: usize,
    count_rpm: usize,
    prev_rpm: i32,
    tail_hp: [f64; DYNO_VIEW_WINDOW_SIZE],
    tail_torque: [f64; DYNO_VIEW_WINDOW_SIZE],
    tail_rpm: [f64; DYNO_VIEW_WINDOW_SIZE_RPM],
    initialized: bool,
    pub current_torque: f64,
    pub current_hp: f64,
}

impl DynoView {
    pub fn new(config: DynoConfig) -> Self {
        let mut view = Self {
            config,
            run_options: DEFAULT_DYNO_RUN_OPTIONS,
            air_density_kg_m3: 1.225,
            wheel_overall_diameter_mm: 0,
            sae_correction_factor: 1.0,
            point: DynoPoint::EMPTY,
            point_prev: DynoPoint::EMPTY,
            count: 0,
            count_rpm: 0,
            prev_rpm: 0,
            tail_hp: [0.0; DYNO_VIEW_WINDOW_SIZE],
            tail_torque: [0.0; DYNO_VIEW_WINDOW_SIZE],
            tail_rpm: [0.0; DYNO_VIEW_WINDOW_SIZE_RPM],
            initialized: false,
            current_torque: 0.0,
            current_hp: 0.0,
        };
        view.init();
        view
    }

    pub fn update_config(&mut self, config: DynoConfig) {
        self.config = config;
        self.initialized = false;
        self.init();
    }

    pub fn set_run_options(&mut self, options: DynoRunOptions) {
        self.run_options = options;
    }

    fn init(&mut self) {
        if self.initialized {
            return;
        }
        self.initialized = true;

        let c = &self.config;
        self.wheel_overall_diameter_mm = (c.dyno_car_wheel_dia_inch as f64 * 25.4
            + c.dyno_car_wheel_tire_width_mm as f64
                * c.dyno_car_wheel_aspect_ratio as f64
                * 0.01
                * 2.0) as u16;

        let temp = c.dyno_sae_temperature_c as f64;
        let sae_vapor_pressure = 6.1078
            * 10f64.powf((7.5 * temp) / (237.3 + temp))
            * 0.02953
            * (c.dyno_sae_relative_humidity as f64 / 100.0);

        let sae_baro_mmhg = 29.23 * (c.dyno_sae_baro as f64 / 100.0);
        let sae_baro_correction = 29.23 / (sae_baro_mmhg - sae_vapor_pressure);
        let sae_temp_correction = ((temp + 273.0) / 298.0).powf(0.5);
        self.sae_correction_factor =
            1.176 * (sae_baro_correction * sae_temp_correction) - 0.176;

        self.reset();
    }

    pub fn reset(&mut self) {
        self.point_prev = DynoPoint::EMPTY;
        self.count = 0;
        self.count_rpm = 0;
        self.current_torque = 0.0;
        self.current_hp = 0.0;
        self.tail_hp = [0.0; DYNO_VIEW_WINDOW_SIZE];
        self.tail_torque = [0.0; DYNO_VIEW_WINDOW_SIZE];
        self.tail_rpm = [0.0; DYNO_VIEW_WINDOW_SIZE_RPM];
    }

    pub fn on_rpm(&mut self, rpm: i32, time: f64, tps: f64) -> Option<DynoRunPoint> {
        let opts = self.run_options;

        if !opts.ignore_tps_min {
            if tps < DYNO_VIEW_TPS_MIN_FOR_RUN
                || self.point_prev.tps - tps > DYNO_VIEW_TPS_DIFF_TO_RESET_RUN
            {
                self.reset();
                return None;
            }
        }

        if opts.min_rpm > 0 && rpm < opts.min_rpm as i32 {
            if self.count > 0 {
                self.reset();
            }
            return None;
        }

        if self.point_prev.rpm > 0 && self.point_prev.time > 0.0 {
            if (rpm - self.prev_rpm).abs() < 1 {
                return None;
            }
            self.prev_rpm = rpm;

            if time - self.point_prev.time < DYNO_VIEW_LOG_TIME_SMOOTH_SEC {
                return None;
            }

            let rpm_diff_smooth = (rpm - self.point_prev.rpm).abs();
            if rpm_diff_smooth < DYNO_VIEW_RPM_DIFF_SMOOTH {
                return None;
            }

            move_window(DYNO_VIEW_WINDOW_SIZE_RPM, &mut self.tail_rpm);
            self.tail_rpm[0] = rpm as f64;

            self.count_rpm += 1;
            let accumulate_rpm_size = self.count_rpm.min(DYNO_VIEW_WINDOW_SIZE_RPM);
            self.point.rpm =
                accumulate_window(accumulate_rpm_size, &self.tail_rpm).round() as i32;

            if self.point.rpm + DYNO_VIEW_RPM_FALL_TO_RESET_RUN < self.point_prev.rpm {
                self.reset();
                return None;
            }

            let rpm_diff_step = (self.point.rpm - self.point_prev.rpm).abs();
            if rpm_diff_step < self.config.dyno_rpm_step as i32 {
                return None;
            }
        } else {
            self.point.rpm = rpm;
        }

        self.point.time = time;
        self.point.tps = tps;

        let gear = self.config.dyno_car_gear_primary_reduction as f64
            * self.config.dyno_car_gear_ratio as f64
            * self.config.dyno_car_gear_final_drive as f64;

        let engine_rps = self.point.rpm as f64 / 60.0;
        let axle_rps = engine_rps / gear;
        self.point.v_ms = axle_rps * (self.wheel_overall_diameter_mm as f64 / 1000.0) * 3.1416;

        if self.point_prev.rpm > 0 && self.point_prev.time > 0.0 {
            let dt = self.point.time - self.point_prev.time;
            let distance_m = ((self.point.v_ms + self.point_prev.v_ms) / 2.0) * dt;
            let mut a_ms2 = (self.point.v_ms - self.point_prev.v_ms) / dt;
            if a_ms2 < 0.0 {
                a_ms2 = 0.0;
            }

            let force_n =
                (self.config.dyno_car_cargo_mass_kg + self.config.dyno_car_car_mass_kg) as f64
                    * a_ms2;

            let mut force_drag_n = 0.5
                * self.air_density_kg_m3
                * (self.point.v_ms * self.point.v_ms)
                * self.config.dyno_car_frontal_area_m2 as f64
                * self.config.dyno_car_coeff_of_drag as f64;
            force_drag_n *= self.sae_correction_factor;

            let force_total_n = force_n + force_drag_n;
            let torque_wheel_nm = force_total_n
                * ((self.wheel_overall_diameter_mm as f64 / 2.0) / 1000.0);
            let torque_nm = torque_wheel_nm / gear;
            let torque_lb_ft = torque_nm * 0.737562;
            let hp = torque_lb_ft * self.point.rpm as f64 / 5252.0;

            let _ = distance_m;

            move_window_small(&mut self.tail_hp);
            move_window_small(&mut self.tail_torque);
            self.tail_torque[0] = torque_nm;
            self.tail_hp[0] = hp;

            if self.count < DYNO_VIEW_WINDOW_SIZE {
                self.count += 1;
            }

            let accumulate_size = self.count.min(DYNO_VIEW_WINDOW_SIZE);
            self.current_torque = accumulate_window(accumulate_size, &self.tail_torque);
            self.current_hp = accumulate_window(accumulate_size, &self.tail_hp);

            self.point_prev = self.point;
            return Some(DynoRunPoint {
                rpm: self.point.rpm,
                torque_nm: self.current_torque,
                hp: self.current_hp,
            });
        }

        self.point_prev = self.point;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_rpm_produces_point_after_accel() {
        let mut view = DynoView::new(DEFAULT_DYNO_CONFIG);
        view.set_run_options(DynoRunOptions {
            ignore_tps_min: true,
            min_rpm: 0,
        });

        let mut last = None;
        for (t, rpm) in [(0.0, 800), (0.2, 1500), (0.4, 2500), (0.6, 3500), (0.8, 4500)] {
            if let Some(p) = view.on_rpm(rpm, t, 50.0) {
                last = Some(p);
            }
        }
        assert!(last.is_some());
        let p = last.unwrap();
        assert!(p.rpm > 0);
        assert!(p.torque_nm > 0.0);
        assert!(p.hp > 0.0);
    }
}
