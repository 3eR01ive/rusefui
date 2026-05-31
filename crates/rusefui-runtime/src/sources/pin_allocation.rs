//! Индекс занятых пинов по пулам INI (`$output_pin_e_list`, `$gpio_list`, …).

use std::collections::HashMap;

use rusefi_ini::ConfigFieldKind;

/// Значение «пин не выбран» в rusEFI.
pub const PIN_NONE_VALUE: u32 = 0;

fn is_placeholder_label(label: &str) -> bool {
    label.trim().eq_ignore_ascii_case("INVALID")
}

fn label_for_value(field: &ConfigFieldKind, value: u32) -> Option<String> {
    let ConfigFieldKind::Enum(e) = field else {
        return None;
    };
    e.options
        .iter()
        .find(|o| o.value == value)
        .map(|o| o.label.clone())
}

fn should_track(field: &ConfigFieldKind, value: u32) -> bool {
    let ConfigFieldKind::Enum(e) = field else {
        return false;
    };
    if e.enum_define.is_none() {
        return false;
    }
    if value == PIN_NONE_VALUE {
        return false;
    }
    let Some(label) = label_for_value(field, value) else {
        return true;
    };
    !is_placeholder_label(&label)
}

/// `pool → pin value → имена полей config`, которые его заняли.
pub fn build_pin_usage(
    config_fields: &HashMap<String, ConfigFieldKind>,
    values: &HashMap<String, f64>,
) -> HashMap<String, HashMap<u32, Vec<String>>> {
    let mut index: HashMap<String, HashMap<u32, Vec<String>>> = HashMap::new();

    for (field_name, kind) in config_fields {
        let ConfigFieldKind::Enum(e) = kind else {
            continue;
        };
        let Some(pool) = e.enum_define.as_deref() else {
            continue;
        };
        let Some(&raw) = values.get(field_name) else {
            continue;
        };
        let value = raw as u32;
        if !should_track(kind, value) {
            continue;
        }

        let pool_map = index.entry(pool.to_string()).or_default();
        let users = pool_map.entry(value).or_default();
        if !users.iter().any(|u| u == field_name) {
            users.push(field_name.clone());
        }
    }

    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusefi_ini::{BitsField, EnumField, EnumOption, ScalarType};

    fn output_pin_enum(field_name: &str) -> (String, ConfigFieldKind) {
        (
            field_name.to_string(),
            ConfigFieldKind::Enum(EnumField {
                bits: BitsField {
                    ty: ScalarType::U16,
                    offset: 0,
                    page: 0,
                    bit_low: 0,
                    bit_high: 8,
                },
                options: vec![
                    EnumOption {
                        value: 0,
                        label: "NONE".into(),
                    },
                    EnumOption {
                        value: 41,
                        label: "Ign 5".into(),
                    },
                ],
                enum_define: Some("output_pin_e_list".into()),
            }),
        )
    }

    #[test]
    fn detects_duplicate_output_pins() {
        let mut fields = HashMap::new();
        fields.insert(output_pin_enum("fanPin").0, output_pin_enum("fanPin").1);
        fields.insert(output_pin_enum("vvtPins1").0, output_pin_enum("vvtPins1").1);

        let mut values = HashMap::new();
        values.insert("fanPin".into(), 41.0);
        values.insert("vvtPins1".into(), 41.0);

        let usage = build_pin_usage(&fields, &values);
        let pool = usage.get("output_pin_e_list").unwrap();
        let users = pool.get(&41).unwrap();
        assert_eq!(users.len(), 2);
        assert!(users.contains(&"fanPin".to_string()));
        assert!(users.contains(&"vvtPins1".to_string()));
    }

    #[test]
    fn ignores_none_and_invalid() {
        let mut fields = HashMap::new();
        fields.insert(output_pin_enum("a").0, output_pin_enum("a").1);
        let mut e_invalid = output_pin_enum("b").1;
        if let ConfigFieldKind::Enum(ref mut e) = e_invalid {
            e.options.push(EnumOption {
                value: 2,
                label: "INVALID".into(),
            });
        }
        fields.insert("b".into(), e_invalid);
        let mut values = HashMap::new();
        values.insert("a".into(), 0.0);
        values.insert("b".into(), 2.0);

        let usage = build_pin_usage(&fields, &values);
        assert!(usage.is_empty() || usage.values().all(|m| m.is_empty()));
    }
}
