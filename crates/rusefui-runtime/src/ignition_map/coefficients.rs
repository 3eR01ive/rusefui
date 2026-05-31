use serde::Deserialize;

const DEFAULT_COEFFICIENTS_JSON: &str =
    include_str!("../../resources/ignition_map_coefficients.json");

#[derive(Debug, Clone, Deserialize)]
struct CrCorrectionRaw {
    min_cr: f64,
    delta_deg: f64,
}

#[derive(Debug, Clone)]
pub struct CrCorrection {
    pub min_cr: f64,
    pub delta_deg: f64,
}

#[derive(Debug, Clone)]
pub struct ModelCoefficients {
    pub burn_duration_ref_deg: f64,
    pub chamber_factors: std::collections::HashMap<String, f64>,
    pub spark_factors: std::collections::HashMap<String, f64>,
    pub valve_factors: std::collections::HashMap<u32, f64>,
    pub bore_reference_mm: f64,
    pub bore_exponent: f64,
    pub compression_reference: f64,
    pub compression_exponent: f64,
    pub flame_delay_base_deg: f64,
    pub flame_delay_cr_corrections: Vec<CrCorrection>,
    pub peak_pressure_target_deg: f64,
    pub rpm_correction_factor: f64,
    pub rpm_reference: f64,
    pub load_reference_map_kpa: f64,
    pub vacuum_deg_per_10_kpa: f64,
    pub boost_deg_per_0_1_bar: f64,
    pub boost_aspiration_scale: std::collections::HashMap<String, f64>,
    pub fuel_factors: std::collections::HashMap<String, f64>,
    pub stock_overlap_deg: f64,
    pub overlap_retard_per_deg: f64,
    pub min_idle_deg: f64,
    pub max_wot_deg: f64,
    pub max_partial_load_deg: f64,
    pub wot_map_threshold_kpa: f64,
    pub idle_rpm_max: f64,
    pub idle_map_max_kpa: f64,
    pub min_advance_deg: f64,
    pub plausibility_max_wot_deg: f64,
    pub plausibility_max_turbo_deg: f64,
    pub plausibility_max_idle_deg: f64,
    pub plausibility_min_operating_deg: f64,
}

impl ModelCoefficients {
    pub fn default_embedded() -> Result<Self, String> {
        Self::from_json(DEFAULT_COEFFICIENTS_JSON)
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        let data: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("coefficients JSON: {e}"))?;

        let ref_eng = &data["reference_engine"];
        let burn = &data["burn_index"];
        let flame = &data["flame_delay"];
        let rpm = &data["rpm_correction"];
        let load = &data["load_correction"];
        let limits = &data["limits"];
        let plaus = &data["plausibility"];
        let cam = &data.get("cam_timing").cloned().unwrap_or_default();

        let mut cr_corrections: Vec<CrCorrectionRaw> = flame
            .get("cr_corrections")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        cr_corrections.sort_by(|a, b| {
            b.min_cr
                .partial_cmp(&a.min_cr)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let valve_factors: std::collections::HashMap<u32, f64> = data["valve_factors"]
            .as_object()
            .ok_or("valve_factors missing")?
            .iter()
            .map(|(k, v)| {
                Ok((
                    k.parse::<u32>().map_err(|_| format!("bad valve key {k}"))?,
                    v.as_f64().ok_or("valve factor not number")?,
                ))
            })
            .collect::<Result<_, String>>()?;

        let chamber_factors = json_str_map(&data["chamber_factors"])?;
        let spark_factors = json_str_map(&data["spark_factors"])?;
        let fuel_factors = data
            .get("fuel_factors")
            .map(json_str_map)
            .transpose()?
            .unwrap_or_default();

        let boost = &load["boost"];
        let boost_aspiration_scale = json_str_map(&boost["aspiration_scale"])?;

        Ok(Self {
            burn_duration_ref_deg: ref_eng["burn_duration_ref_deg"]
                .as_f64()
                .ok_or("burn_duration_ref_deg")?,
            chamber_factors,
            spark_factors,
            valve_factors,
            bore_reference_mm: burn["bore_reference_mm"].as_f64().ok_or("bore_reference_mm")?,
            bore_exponent: burn["bore_exponent"].as_f64().ok_or("bore_exponent")?,
            compression_reference: burn["compression_reference"]
                .as_f64()
                .ok_or("compression_reference")?,
            compression_exponent: burn["compression_exponent"]
                .as_f64()
                .ok_or("compression_exponent")?,
            flame_delay_base_deg: flame["base_deg"].as_f64().ok_or("flame_delay.base_deg")?,
            flame_delay_cr_corrections: cr_corrections
                .into_iter()
                .map(|c| CrCorrection {
                    min_cr: c.min_cr,
                    delta_deg: c.delta_deg,
                })
                .collect(),
            peak_pressure_target_deg: data["peak_pressure_target_deg"]
                .as_f64()
                .ok_or("peak_pressure_target_deg")?,
            rpm_correction_factor: rpm["factor"].as_f64().ok_or("rpm.factor")?,
            rpm_reference: rpm["reference_rpm"].as_f64().ok_or("rpm.reference_rpm")?,
            load_reference_map_kpa: load["reference_map_kpa"]
                .as_f64()
                .ok_or("load.reference_map_kpa")?,
            vacuum_deg_per_10_kpa: load["vacuum"]["deg_per_10_kpa"]
                .as_f64()
                .ok_or("vacuum.deg_per_10_kpa")?,
            boost_deg_per_0_1_bar: boost["deg_per_0_1_bar"]
                .as_f64()
                .ok_or("boost.deg_per_0_1_bar")?,
            boost_aspiration_scale,
            fuel_factors,
            stock_overlap_deg: cam["stock_overlap_deg"].as_f64().unwrap_or(20.0),
            overlap_retard_per_deg: cam["overlap_retard_per_deg"].as_f64().unwrap_or(0.05),
            min_idle_deg: limits["min_idle_deg"].as_f64().ok_or("limits.min_idle_deg")?,
            max_wot_deg: limits["max_wot_deg"].as_f64().ok_or("limits.max_wot_deg")?,
            max_partial_load_deg: limits["max_partial_load_deg"]
                .as_f64()
                .ok_or("limits.max_partial_load_deg")?,
            wot_map_threshold_kpa: limits["wot_map_threshold_kpa"]
                .as_f64()
                .ok_or("limits.wot_map_threshold_kpa")?,
            idle_rpm_max: limits["idle_rpm_max"].as_f64().ok_or("limits.idle_rpm_max")?,
            idle_map_max_kpa: limits["idle_map_max_kpa"]
                .as_f64()
                .ok_or("limits.idle_map_max_kpa")?,
            min_advance_deg: limits["min_advance_deg"].as_f64().unwrap_or(-5.0),
            plausibility_max_wot_deg: plaus["max_wot_deg"].as_f64().ok_or("plaus.max_wot_deg")?,
            plausibility_max_turbo_deg: plaus["max_turbo_deg"].as_f64().ok_or("plaus.max_turbo_deg")?,
            plausibility_max_idle_deg: plaus["max_idle_deg"].as_f64().ok_or("plaus.max_idle_deg")?,
            plausibility_min_operating_deg: plaus["min_operating_deg"]
                .as_f64()
                .ok_or("plaus.min_operating_deg")?,
        })
    }

    pub fn boost_scale(&self, aspiration: &str) -> f64 {
        self.boost_aspiration_scale
            .get(aspiration)
            .or_else(|| self.boost_aspiration_scale.get("naturally_aspirated"))
            .copied()
            .unwrap_or(0.0)
    }
}

fn json_str_map(value: &serde_json::Value) -> Result<std::collections::HashMap<String, f64>, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| format!("expected object, got {value}"))?;
    obj.iter()
        .map(|(k, v)| {
            Ok((
                k.clone(),
                v.as_f64()
                    .ok_or_else(|| format!("factor {k} not a number"))?,
            ))
        })
        .collect()
}
