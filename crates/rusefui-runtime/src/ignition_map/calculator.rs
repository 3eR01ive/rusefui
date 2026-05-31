use std::collections::HashSet;

use super::coefficients::ModelCoefficients;
use super::engine::EngineParams;

#[derive(Debug, Clone)]
pub struct SparkCell {
    pub rpm: f64,
    pub map_kpa: f64,
    pub advance_deg: f64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SparkDiagnostics {
    pub burn_index: f64,
    pub burn_duration_deg: f64,
    pub flame_delay_deg: f64,
    pub mbt_deg: f64,
    pub fuel_factor: f64,
    pub cam_retard_deg: f64,
}

pub struct SparkAdvanceCalculator {
    engine: EngineParams,
    coef: ModelCoefficients,
    boost_scale: f64,
    burn_index: f64,
    burn_duration: f64,
    flame_delay: f64,
    mbt: f64,
    fuel_factor: f64,
}

impl SparkAdvanceCalculator {
    pub fn new(engine: EngineParams, coef: ModelCoefficients) -> Self {
        let boost_scale = coef.boost_scale(&engine.aspiration);
        let burn_index = Self::compute_burn_index(&engine, &coef);
        let burn_duration = coef.burn_duration_ref_deg * burn_index;
        let flame_delay = Self::compute_flame_delay(&engine, &coef);
        let mbt = burn_duration / 2.0 + flame_delay - coef.peak_pressure_target_deg;
        let fuel_factor = coef
            .fuel_factors
            .get(&engine.fuel)
            .copied()
            .unwrap_or(1.0);

        Self {
            engine,
            coef,
            boost_scale,
            burn_index,
            burn_duration,
            flame_delay,
            mbt,
            fuel_factor,
        }
    }

    pub fn diagnostics(&self) -> SparkDiagnostics {
        SparkDiagnostics {
            burn_index: round4(self.burn_index),
            burn_duration_deg: round2(self.burn_duration),
            flame_delay_deg: round2(self.flame_delay),
            mbt_deg: round2(self.mbt),
            fuel_factor: self.fuel_factor,
            cam_retard_deg: round2(self.cam_retard()),
        }
    }

    fn factor(map: &std::collections::HashMap<String, f64>, key: &str, label: &str) -> Result<f64, String> {
        map.get(key)
            .copied()
            .ok_or_else(|| format!("Unknown {label}: {key:?}"))
    }

    fn compute_burn_index(engine: &EngineParams, coef: &ModelCoefficients) -> f64 {
        let chamber = Self::factor(&coef.chamber_factors, &engine.chamber_type, "chamber_type")
            .unwrap_or(1.0);
        let spark = Self::factor(&coef.spark_factors, &engine.spark_location, "spark_location")
            .unwrap_or(1.0);
        let valves = coef
            .valve_factors
            .get(&engine.valves_per_cylinder)
            .copied()
            .unwrap_or(1.0);

        let bore_term = (engine.bore_mm / coef.bore_reference_mm).powf(coef.bore_exponent);
        let cr_term = (coef.compression_reference / engine.compression_ratio)
            .powf(coef.compression_exponent);

        chamber * spark * valves * bore_term * cr_term
    }

    fn compute_flame_delay(engine: &EngineParams, coef: &ModelCoefficients) -> f64 {
        let mut delay = coef.flame_delay_base_deg;
        for correction in &coef.flame_delay_cr_corrections {
            if engine.compression_ratio > correction.min_cr {
                delay += correction.delta_deg;
            }
        }
        delay
    }

    fn rpm_correction(&self, rpm: f64) -> f64 {
        if rpm <= 0.0 {
            return 0.0;
        }
        self.coef.rpm_correction_factor * (rpm / self.coef.rpm_reference).log2()
    }

    fn load_correction(&self, map_kpa: f64) -> f64 {
        let ref_map = self.coef.load_reference_map_kpa;
        if map_kpa <= ref_map {
            let steps = (ref_map - map_kpa) / 10.0;
            return steps * self.coef.vacuum_deg_per_10_kpa;
        }
        if self.boost_scale <= 0.0 {
            return 0.0;
        }
        let boost_bar = (map_kpa - ref_map) / 100.0;
        let steps = boost_bar / 0.1;
        steps * self.coef.boost_deg_per_0_1_bar * self.boost_scale
    }

    fn cam_retard(&self) -> f64 {
        let overlap = match self.engine.overlap_deg {
            Some(v) => v,
            None => return 0.0,
        };
        let delta = overlap - self.coef.stock_overlap_deg;
        if delta <= 0.0 {
            return 0.0;
        }
        delta * self.coef.overlap_retard_per_deg
    }

    fn apply_limits(&self, rpm: f64, map_kpa: f64, advance: f64) -> f64 {
        let mut out = advance;
        if map_kpa >= self.coef.wot_map_threshold_kpa {
            out = out.min(self.coef.max_wot_deg);
        } else {
            out = out.min(self.coef.max_partial_load_deg);
        }
        if rpm <= self.coef.idle_rpm_max && map_kpa <= self.coef.idle_map_max_kpa {
            out = out.max(self.coef.min_idle_deg);
        }
        out.max(self.coef.min_advance_deg)
    }

    fn plausibility_warnings(&self, rpm: f64, map_kpa: f64, advance: f64) -> Vec<String> {
        let c = &self.coef;
        let mut warnings = Vec::new();
        let is_wot = map_kpa >= c.wot_map_threshold_kpa;
        let is_idle = rpm <= c.idle_rpm_max && map_kpa <= c.idle_map_max_kpa;

        if is_wot && advance > c.plausibility_max_wot_deg {
            warnings.push(format!(
                "WOT advance {advance:.1}° > {}° (rpm={rpm:.0}, map={map_kpa:.0} kPa)",
                c.plausibility_max_wot_deg
            ));
        }
        if self.engine.is_forced_induction() && advance > c.plausibility_max_turbo_deg {
            warnings.push(format!(
                "Turbo advance {advance:.1}° > {}° (rpm={rpm:.0}, map={map_kpa:.0} kPa)",
                c.plausibility_max_turbo_deg
            ));
        }
        if is_idle && advance > c.plausibility_max_idle_deg {
            warnings.push(format!(
                "Idle advance {advance:.1}° > {}° (rpm={rpm:.0}, map={map_kpa:.0} kPa)",
                c.plausibility_max_idle_deg
            ));
        }
        if advance < c.plausibility_min_operating_deg {
            warnings.push(format!(
                "Advance {advance:.1}° < {}° (rpm={rpm:.0}, map={map_kpa:.0} kPa)",
                c.plausibility_min_operating_deg
            ));
        }
        warnings
    }

    pub fn advance_at(&self, rpm: f64, map_kpa: f64) -> SparkCell {
        let mut advance = self.mbt
            + self.rpm_correction(rpm)
            + self.load_correction(map_kpa)
            - self.cam_retard();
        advance *= self.fuel_factor;
        advance = self.apply_limits(rpm, map_kpa, advance);
        let warnings = self.plausibility_warnings(rpm, map_kpa, advance);

        SparkCell {
            rpm,
            map_kpa,
            advance_deg: (advance * 10.0).round() / 10.0,
            warnings,
        }
    }

    pub fn generate_map(
        &self,
        rpm_axis: &[f64],
        map_axis: &[f64],
    ) -> (Vec<Vec<f64>>, Vec<String>) {
        let mut values = Vec::with_capacity(rpm_axis.len());
        let mut seen = HashSet::new();
        let mut warnings = Vec::new();

        for &rpm in rpm_axis {
            let mut row = Vec::with_capacity(map_axis.len());
            for &map_kpa in map_axis {
                let cell = self.advance_at(rpm, map_kpa);
                for w in cell.warnings {
                    if seen.insert(w.clone()) {
                        warnings.push(w);
                    }
                }
                row.push(cell.advance_deg);
            }
            values.push(row);
        }
        (values, warnings)
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ignition_map::coefficients::ModelCoefficients;

    fn turbo_engine() -> EngineParams {
        EngineParams {
            bore_mm: 91.1,
            stroke_mm: 76.0,
            rod_length_mm: Some(141.0),
            cylinder_count: 6,
            displacement_cc: Some(2972.0),
            compression_ratio: 8.0,
            valves_per_cylinder: 4,
            spark_location: "center".into(),
            chamber_type: "pentroof".into(),
            intake_duration_deg: Some(251.0),
            exhaust_duration_deg: Some(247.0),
            overlap_deg: Some(33.0),
            fuel: "gasoline_98".into(),
            aspiration: "turbocharged".into(),
        }
    }

    #[test]
    fn matches_python_reference_cells() {
        let coef = ModelCoefficients::default_embedded().expect("coefficients");
        let calc = SparkAdvanceCalculator::new(turbo_engine(), coef);

        let c = calc.advance_at(600.0, 100.0);
        assert!((c.advance_deg - 8.7).abs() < 0.05, "600/100: {}", c.advance_deg);

        let c2 = calc.advance_at(4000.0, 200.0);
        assert!((c2.advance_deg - 14.6).abs() < 0.05, "4000/200: {}", c2.advance_deg);

        let diag = calc.diagnostics();
        assert!((diag.burn_index - 1.2189).abs() < 0.001);
        assert!((diag.mbt_deg - 16.9).abs() < 0.1);
    }
}
