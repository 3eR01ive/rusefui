use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineParams {
    pub bore_mm: f64,
    pub stroke_mm: f64,
    #[serde(default)]
    pub rod_length_mm: Option<f64>,
    #[serde(default = "default_cylinder_count")]
    pub cylinder_count: u32,
    #[serde(default)]
    pub displacement_cc: Option<f64>,
    pub compression_ratio: f64,
    #[serde(default = "default_valves")]
    pub valves_per_cylinder: u32,
    #[serde(default = "default_spark_location")]
    pub spark_location: String,
    #[serde(default = "default_chamber_type")]
    pub chamber_type: String,
    #[serde(default)]
    pub intake_duration_deg: Option<f64>,
    #[serde(default)]
    pub exhaust_duration_deg: Option<f64>,
    #[serde(default)]
    pub overlap_deg: Option<f64>,
    #[serde(default = "default_fuel")]
    pub fuel: String,
    #[serde(default = "default_aspiration")]
    pub aspiration: String,
}

fn default_cylinder_count() -> u32 {
    4
}

fn default_valves() -> u32 {
    4
}

fn default_spark_location() -> String {
    "center".into()
}

fn default_chamber_type() -> String {
    "pentroof".into()
}

fn default_fuel() -> String {
    "gasoline_95".into()
}

fn default_aspiration() -> String {
    "naturally_aspirated".into()
}

impl Default for EngineParams {
    fn default() -> Self {
        Self {
            bore_mm: 86.0,
            stroke_mm: 86.0,
            rod_length_mm: None,
            cylinder_count: 4,
            displacement_cc: None,
            compression_ratio: 10.0,
            valves_per_cylinder: 4,
            spark_location: default_spark_location(),
            chamber_type: default_chamber_type(),
            intake_duration_deg: None,
            exhaust_duration_deg: None,
            overlap_deg: None,
            fuel: default_fuel(),
            aspiration: default_aspiration(),
        }
    }
}
