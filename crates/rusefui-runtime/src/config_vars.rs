//! Логические переменные checklist: в коде только внутренние id, имена полей INI — в YAML.

use std::collections::HashMap;

use serde::Deserialize;

use crate::sources::config::{ConfigFieldInfo, ConfigSnapshot};
use crate::sources::pin_allocation::PIN_NONE_VALUE;

/// Внутренние id переменных (не имена полей rusEFI).
pub mod logic {
    pub const ENGINE_CYLINDER_COUNT: &str = "engine.cylinder_count";
    pub const ENGINE_FIRING_ORDER: &str = "engine.firing_order";
    pub const IGNITION_ENABLED: &str = "ignition.enabled";
    pub const IGNITION_MODE: &str = "ignition.mode";
    pub const IGNITION_OUTPUT_PINS: &str = "ignition.output_pins";
    pub const FUEL_INJECTION_ENABLED: &str = "fuel.injection_enabled";
    pub const FUEL_INJECTION_MODE: &str = "fuel.injection_mode";
    pub const FUEL_INJECTOR_PINS: &str = "fuel.injector_pins";
    pub const TRIGGER_TYPE: &str = "trigger.type";
    pub const TRIGGER_SECONDARY_INPUT: &str = "trigger.secondary_input";
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VarBinding {
    /// Источник данных (`config` — page 0 snapshot).
    pub source: String,
    /// Скalar / enum поле INI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter: Option<String>,
    /// Префикс индексированного массива пинов (`ignitionPins` + `1`…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IgnitionModeConstants {
    #[serde(default)]
    pub single_coil: u32,
    #[serde(default = "default_individual_mode")]
    pub individual: u32,
}

fn default_individual_mode() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InjectionModeConstants {
    #[serde(default = "default_single_point_mode")]
    pub single_point: u32,
    #[serde(default = "default_sequential_mode")]
    pub sequential: u32,
    #[serde(default = "default_skip_cylinder_pin_check")]
    pub skip_cylinder_pin_check: Vec<u32>,
}

fn default_single_point_mode() -> u32 {
    3
}

fn default_sequential_mode() -> u32 {
    1
}

fn default_skip_cylinder_pin_check() -> Vec<u32> {
    vec![3]
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TriggerConstants {
    #[serde(default)]
    pub types_needing_secondary: Vec<u32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConflictConstants {
    #[serde(default)]
    pub ignition: IgnitionModeConstants,
    #[serde(default)]
    pub injection: InjectionModeConstants,
    #[serde(default)]
    pub trigger: TriggerConstants,
}

pub struct ConfigVarResolver<'a> {
    vars: &'a HashMap<String, VarBinding>,
    constants: &'a ConflictConstants,
    field_info: &'a HashMap<String, ConfigFieldInfo>,
}

impl<'a> ConfigVarResolver<'a> {
    pub fn new(
        vars: &'a HashMap<String, VarBinding>,
        constants: &'a ConflictConstants,
        field_info: &'a HashMap<String, ConfigFieldInfo>,
    ) -> Self {
        Self {
            vars,
            constants,
            field_info,
        }
    }

    pub fn constants(&self) -> &ConflictConstants {
        self.constants
    }

    pub fn field_name(&self, var: &str) -> Option<&str> {
        let binding = self.vars.get(var)?;
        if binding.source != "config" {
            return None;
        }
        binding.parameter.as_deref()
    }

    pub fn indexed_field_name(&self, var: &str, index: usize) -> Option<String> {
        let binding = self.vars.get(var)?;
        if binding.source != "config" {
            return None;
        }
        let prefix = binding.prefix.as_deref()?;
        Some(format!("{prefix}{index}"))
    }

    pub fn max_index(&self, var: &str) -> usize {
        let Some(binding) = self.vars.get(var) else {
            return 0;
        };
        let Some(prefix) = binding.prefix.as_deref() else {
            return 0;
        };
        max_index_for_prefix(self.field_info, prefix)
    }

    pub fn scalar_value(&self, snapshot: &ConfigSnapshot, var: &str) -> Option<f64> {
        let field = self.field_name(var)?;
        snapshot.values.get(field).copied()
    }

    pub fn scalar_u32(&self, snapshot: &ConfigSnapshot, var: &str) -> u32 {
        self.scalar_value(snapshot, var)
            .map(|v| v as u32)
            .unwrap_or(0)
    }

    pub fn scalar_usize(&self, snapshot: &ConfigSnapshot, var: &str) -> usize {
        self.scalar_value(snapshot, var)
            .map(|v| v.round().max(0.0) as usize)
            .unwrap_or(0)
    }

    pub fn bool_enabled(&self, snapshot: &ConfigSnapshot, var: &str) -> bool {
        self.scalar_value(snapshot, var)
            .map(|v| v >= 1.0)
            .unwrap_or(false)
    }

    pub fn pin_assigned(&self, snapshot: &ConfigSnapshot, var: &str) -> bool {
        let Some(field) = self.field_name(var) else {
            return false;
        };
        snapshot
            .values
            .get(field)
            .map(|v| *v as u32 > PIN_NONE_VALUE)
            .unwrap_or(false)
    }

    pub fn indexed_pin_assigned(
        &self,
        snapshot: &ConfigSnapshot,
        var: &str,
        index: usize,
    ) -> bool {
        let Some(field) = self.indexed_field_name(var, index) else {
            return false;
        };
        snapshot
            .values
            .get(&field)
            .map(|v| *v as u32 > PIN_NONE_VALUE)
            .unwrap_or(false)
    }

    pub fn option_label(
        &self,
        var: &str,
        value: u32,
    ) -> Option<String> {
        let field = self.field_name(var)?;
        let info = self.field_info.get(field)?;
        let options = info.options.as_ref()?;
        options
            .iter()
            .find(|o| o.value == value)
            .map(|o| o.label.clone())
    }
}

pub fn max_index_for_prefix(
    field_info: &HashMap<String, ConfigFieldInfo>,
    prefix: &str,
) -> usize {
    let mut max = 0usize;
    for name in field_info.keys() {
        let Some(suffix) = name.strip_prefix(prefix) else {
            continue;
        };
        if suffix.is_empty() || !suffix.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if let Ok(index) = suffix.parse::<usize>() {
            max = max.max(index);
        }
    }
    max
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::config::ConfigFieldInfo;

    fn binding_param(parameter: &str) -> VarBinding {
        VarBinding {
            source: "config".to_string(),
            parameter: Some(parameter.to_string()),
            prefix: None,
        }
    }

    fn binding_prefix(prefix: &str) -> VarBinding {
        VarBinding {
            source: "config".to_string(),
            parameter: None,
            prefix: Some(prefix.to_string()),
        }
    }

    fn test_field_info(name: &str) -> ConfigFieldInfo {
        ConfigFieldInfo {
            name: name.to_string(),
            ty: "enum".to_string(),
            units: None,
            options: None,
            array_cols: None,
            array_rows: None,
            array_length: None,
            pin_pool: None,
        }
    }

    #[test]
    fn max_index_from_field_info() {
        let mut field_info = HashMap::new();
        for i in 1..=8 {
            field_info.insert(format!("ignitionPins{i}"), test_field_info(&format!("ignitionPins{i}")));
        }
        assert_eq!(max_index_for_prefix(&field_info, "ignitionPins"), 8);
    }

    #[test]
    fn resolver_maps_logic_var_to_ini_field() {
        let vars = HashMap::from([(
            logic::ENGINE_CYLINDER_COUNT.to_string(),
            binding_param("cylindersCount"),
        )]);
        let constants = ConflictConstants::default();
        let field_info = HashMap::new();
        let resolver = ConfigVarResolver::new(&vars, &constants, &field_info);
        assert_eq!(
            resolver.field_name(logic::ENGINE_CYLINDER_COUNT),
            Some("cylindersCount")
        );
    }

    #[test]
    fn resolver_builds_indexed_pin_names() {
        let vars = HashMap::from([(
            logic::IGNITION_OUTPUT_PINS.to_string(),
            binding_prefix("ignitionPins"),
        )]);
        let constants = ConflictConstants::default();
        let field_info = HashMap::new();
        let resolver = ConfigVarResolver::new(&vars, &constants, &field_info);
        assert_eq!(
            resolver.indexed_field_name(logic::IGNITION_OUTPUT_PINS, 3),
            Some("ignitionPins3".to_string())
        );
    }
}
