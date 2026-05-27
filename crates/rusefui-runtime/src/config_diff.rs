//! Сравнение config проекта и ECU после подключения.

use std::collections::HashMap;

use rusefi_ini::{encode_config_value, encode_string_value, ConfigFieldKind};
use serde::{Deserialize, Serialize};

use crate::sources::output_channels::IniContext;

const VALUE_EPS: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffSide {
    Project,
    Ecu,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDiffEntry {
    pub field: String,
    /// `scalar` | `enum`
    pub ty: String,
    pub project: f64,
    pub ecu: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDiffSnapshot {
    pub active: bool,
    pub entries: Vec<ConfigDiffEntry>,
    pub choices: HashMap<String, DiffSide>,
}

pub fn values_differ(a: f64, b: f64) -> bool {
    if !a.is_finite() || !b.is_finite() {
        return a.to_bits() != b.to_bits();
    }
    (a - b).abs() > VALUE_EPS
}

/// Поля scalar/enum, присутствующие в обоих снимках и отличающиеся по значению.
pub fn compute_config_diff(
    project: &HashMap<String, f64>,
    ecu: &HashMap<String, f64>,
    config_fields: &HashMap<String, ConfigFieldKind>,
) -> Vec<ConfigDiffEntry> {
    let mut entries = Vec::new();

    for (name, kind) in config_fields {
        let ty = match kind {
            ConfigFieldKind::Scalar(_) => "scalar",
            ConfigFieldKind::Enum(_) => "enum",
            ConfigFieldKind::Array(_) | ConfigFieldKind::String(_) => continue,
        };
        let (Some(&pv), Some(&ev)) = (project.get(name), ecu.get(name)) else {
            continue;
        };
        if values_differ(pv, ev) {
            entries.push(ConfigDiffEntry {
                field: name.clone(),
                ty: ty.into(),
                project: pv,
                ecu: ev,
            });
        }
    }

    entries.sort_by(|a, b| a.field.cmp(&b.field));
    entries
}

pub fn encode_scalar_into_page(
    ini: &IniContext,
    raw: &mut Vec<u8>,
    field: &str,
    value: f64,
) -> Result<(), String> {
    let kind = ini
        .config_fields
        .get(field)
        .ok_or_else(|| format!("unknown config field: {field}"))?;
    match kind {
        ConfigFieldKind::Array(_) => {
            return Err(format!("{field}: таблицы/кривые пока не поддерживаются в diff"));
        }
        ConfigFieldKind::Scalar(_) | ConfigFieldKind::Enum(_) => {}
        ConfigFieldKind::String(_) => {
            return Err(format!("{field}: используйте encode_string_into_page"));
        }
    }
    let offset = match kind {
        ConfigFieldKind::Scalar(s) => s.offset,
        ConfigFieldKind::Enum(e) => e.bits.offset,
        ConfigFieldKind::Array(a) => a.offset,
        ConfigFieldKind::String(_) => unreachable!(),
    } as usize;
    let encoded = encode_config_value(kind, value, raw)
        .ok_or_else(|| format!("cannot encode {field}"))?;
    if offset + encoded.len() > raw.len() {
        raw.resize(offset + encoded.len(), 0);
    }
    raw[offset..offset + encoded.len()].copy_from_slice(&encoded);
    Ok(())
}

pub fn encode_string_into_page(
    ini: &IniContext,
    raw: &mut Vec<u8>,
    field: &str,
    value: &str,
) -> Result<(), String> {
    let kind = ini
        .config_fields
        .get(field)
        .ok_or_else(|| format!("unknown config field: {field}"))?;
    let ConfigFieldKind::String(s) = kind else {
        return Err(format!("{field} is not a string field"));
    };
    let offset = s.offset as usize;
    let encoded = encode_string_value(s, value)
        .ok_or_else(|| format!("cannot encode string {field}"))?;
    if offset + encoded.len() > raw.len() {
        raw.resize(offset + encoded.len(), 0);
    }
    raw[offset..offset + encoded.len()].copy_from_slice(&encoded);
    Ok(())
}

#[derive(Debug, Default)]
pub struct ConfigDiffStore {
    active: bool,
    entries: Vec<ConfigDiffEntry>,
    choices: HashMap<String, DiffSide>,
}

impl ConfigDiffStore {
    pub fn snapshot(&self) -> ConfigDiffSnapshot {
        ConfigDiffSnapshot {
            active: self.active,
            entries: self.entries.clone(),
            choices: self.choices.clone(),
        }
    }

    pub fn clear(&mut self) {
        self.active = false;
        self.entries.clear();
        self.choices.clear();
    }

    pub fn start(&mut self, entries: Vec<ConfigDiffEntry>) {
        self.entries = entries;
        self.choices.clear();
        for e in &self.entries {
            self.choices.insert(e.field.clone(), DiffSide::Ecu);
        }
        self.active = !self.entries.is_empty();
    }

    pub fn set_choice(&mut self, field: &str, side: DiffSide) -> Result<(), String> {
        if !self.entries.iter().any(|e| e.field == field) {
            return Err(format!("поле {field:?} не в списке diff"));
        }
        self.choices.insert(field.to_string(), side);
        Ok(())
    }

    pub fn set_all_choices(&mut self, side: DiffSide) {
        for e in &self.entries {
            self.choices.insert(e.field.clone(), side);
        }
    }

    pub fn entry_for(&self, field: &str) -> Option<&ConfigDiffEntry> {
        self.active
            .then(|| self.entries.iter().find(|e| e.field == field))
            .flatten()
    }

    pub fn choice_for(&self, field: &str) -> Option<DiffSide> {
        self.choices.get(field).copied()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn values_differ_epsilon() {
        assert!(!values_differ(1.0, 1.0 + 1e-9));
        assert!(values_differ(1.0, 1.01));
    }

    #[test]
    fn compute_diff_empty_when_equal() {
        let fields = HashMap::new();
        let mut v = HashMap::new();
        v.insert("a".into(), 1.0);
        assert!(compute_config_diff(&v, &v, &fields).is_empty());
    }
}
