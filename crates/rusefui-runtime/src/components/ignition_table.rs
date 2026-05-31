use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};

use crate::component::{ComponentLogic, ComponentMeta, EcuSyncOnMount, LogicComponentType};
use crate::ignition_map::{generate_table_values, EngineParams};
use crate::session::EcuSession;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct IgnitionTableViewState {
    params: EngineParams,
    generating: bool,
    can_generate: bool,
    message: Option<String>,
    z_field: Option<String>,
    x_field: Option<String>,
    y_field: Option<String>,
}

pub struct IgnitionTableLogic {
    session: Arc<EcuSession>,
    params: EngineParams,
    generating: bool,
    message: Option<String>,
    x_field: Option<String>,
    y_field: Option<String>,
    z_field: Option<String>,
}

impl IgnitionTableLogic {
    pub fn new(session: Arc<EcuSession>) -> Self {
        Self {
            session,
            params: EngineParams::default(),
            generating: false,
            message: None,
            x_field: None,
            y_field: None,
            z_field: None,
        }
    }

    fn config(&self) -> &crate::sources::config::ConfigSource {
        self.session.config()
    }

    fn can_generate(&self) -> bool {
        let snap = self.config().snapshot();
        if !snap.loaded || self.z_field.as_deref().unwrap_or("").is_empty() {
            return false;
        }
        self.session.is_connected() && !snap.read_only || snap.read_only
    }

    fn set_bind_from_payload(&mut self, payload: &Value) {
        if let Some(v) = payload.get("xBins").and_then(|v| v.as_str()) {
            self.x_field = Some(v.to_string());
        }
        if let Some(v) = payload.get("yBins").and_then(|v| v.as_str()) {
            self.y_field = Some(v.to_string());
        }
        if let Some(v) = payload.get("zBins").and_then(|v| v.as_str()) {
            self.z_field = Some(v.to_string());
        }
        if let Some(p) = payload.get("params") {
            if let Ok(next) = serde_json::from_value::<EngineParams>(p.clone()) {
                self.params = next;
            }
        }
    }

    fn apply_params_patch(&mut self, payload: &Value) -> Result<(), String> {
        let mut next = self.params.clone();
        macro_rules! patch_f64 {
            ($key:ident) => {
                if let Some(v) = payload.get(stringify!($key)).and_then(|v| v.as_f64()) {
                    next.$key = v;
                }
            };
        }
        macro_rules! patch_u32 {
            ($key:ident) => {
                if let Some(v) = payload.get(stringify!($key)).and_then(|v| v.as_u64()) {
                    next.$key = v as u32;
                }
            };
        }
        macro_rules! patch_opt_f64 {
            ($key:ident) => {
                if payload.get(stringify!($key)).map_or(false, |v| v.is_null()) {
                    next.$key = None;
                } else if let Some(v) = payload.get(stringify!($key)).and_then(|v| v.as_f64()) {
                    next.$key = Some(v);
                }
            };
        }
        macro_rules! patch_str {
            ($key:ident) => {
                if let Some(v) = payload.get(stringify!($key)).and_then(|v| v.as_str()) {
                    next.$key = v.to_string();
                }
            };
        }

        patch_f64!(bore_mm);
        patch_f64!(stroke_mm);
        patch_f64!(compression_ratio);
        patch_u32!(cylinder_count);
        patch_u32!(valves_per_cylinder);
        patch_opt_f64!(rod_length_mm);
        patch_opt_f64!(displacement_cc);
        patch_opt_f64!(intake_duration_deg);
        patch_opt_f64!(exhaust_duration_deg);
        patch_opt_f64!(overlap_deg);
        patch_str!(spark_location);
        patch_str!(chamber_type);
        patch_str!(fuel);
        patch_str!(aspiration);

        self.params = next;
        Ok(())
    }

    fn generate_map(&mut self) -> Result<(), String> {
        let z_name = self
            .z_field
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("zBins не задан")?
            .to_string();
        let x_name = self
            .x_field
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("xBins (RPM) не задан")?
            .to_string();
        let y_name = self
            .y_field
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("yBins (load) не задан")?
            .to_string();

        let snap = self.config().snapshot();
        if !snap.loaded {
            return Err("Config не загружен".into());
        }

        self.generating = true;
        self.message = None;

        let result = (|| -> Result<(), String> {
            let rpm_axis = self.config().get_array(&x_name)?;
            let load_axis = self.config().get_array(&y_name)?;
            if rpm_axis.is_empty() || load_axis.is_empty() {
                return Err("Оси таблицы пусты".into());
            }

            let values = generate_table_values(&self.params, &rpm_axis, &load_axis)?;

            let cols = rpm_axis.len();
            let rows = load_axis.len();
            if values.len() != rows * cols {
                return Err(format!(
                    "размер карты {} ≠ {}×{}",
                    values.len(),
                    rows,
                    cols
                ));
            }

            let updates: Vec<(usize, f64)> = values
                .into_iter()
                .enumerate()
                .map(|(i, v)| (i, v))
                .collect();

            let live = self.session.is_connected() && !snap.read_only;
            if live {
                self.config()
                    .write_array_values(&self.session, &z_name, &updates)?;
            } else if snap.read_only {
                self.config()
                    .set_array_values_local(&z_name, &updates)?;
            } else {
                return Err(
                    "Нет config для записи — откройте проект или подключите ECU".into(),
                );
            }

            self.message = Some(format!(
                "Сгенерировано {} ячеек ({}×{})",
                updates.len(),
                rows,
                cols
            ));
            Ok(())
        })();

        self.generating = false;
        if let Err(e) = &result {
            self.message = Some(e.clone());
        }
        result
    }

    fn view_state(&self) -> IgnitionTableViewState {
        IgnitionTableViewState {
            params: self.params.clone(),
            generating: self.generating,
            can_generate: self.can_generate(),
            message: self.message.clone(),
            z_field: self.z_field.clone(),
            x_field: self.x_field.clone(),
            y_field: self.y_field.clone(),
        }
    }

    fn to_json(&self) -> Value {
        serde_json::to_value(self.view_state()).unwrap_or(json!({}))
    }
}

impl ComponentLogic for IgnitionTableLogic {
    fn meta(&self) -> ComponentMeta {
        ComponentMeta {
            component_type: LogicComponentType::IgnitionTable.as_str().to_string(),
            has_rust_logic: true,
        }
    }

    fn state(&self) -> Value {
        self.to_json()
    }

    fn ecu_sync_on_mount(&self) -> EcuSyncOnMount {
        EcuSyncOnMount::OutputPollIfConfigLoaded
    }

    fn dispatch(&mut self, action: &str, payload: Value) -> Result<Value, String> {
        match action {
            "mount" => {
                if !payload.is_null() {
                    self.set_bind_from_payload(&payload);
                }
            }
            "set_bind" => {
                self.set_bind_from_payload(&payload);
            }
            "set_params" => {
                self.apply_params_patch(&payload)?;
            }
            "generate_map" => {
                self.generate_map()?;
            }
            _ => return Err(format!("unknown action: {action}")),
        }
        Ok(self.to_json())
    }
}
