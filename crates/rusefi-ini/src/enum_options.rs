//! Разбор списков значений enum для `bits` полей INI.

use std::collections::HashMap;

use crate::model::EnumOption;

/// Разбирает аргументы после `[lo:hi]` в определении `bits`.
pub fn parse_enum_options(parts: &[String], defines: &HashMap<String, Vec<String>>) -> Vec<EnumOption> {
    let mut options = Vec::new();
    let mut positional = 0u32;

    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(key) = part.strip_prefix('$') {
            if let Some(labels) = defines.get(key) {
                for (i, label) in labels.iter().enumerate() {
                    options.push(EnumOption {
                        value: i as u32,
                        label: label.clone(),
                    });
                }
            }
            continue;
        }
        if let Some((idx, label)) = part.split_once('=') {
            if let Ok(value) = idx.trim().parse::<u32>() {
                options.push(EnumOption {
                    value,
                    label: label.trim().trim_matches('"').to_string(),
                });
            }
            continue;
        }
        let label = part.trim_matches('"').to_string();
        options.push(EnumOption {
            value: positional,
            label,
        });
        positional += 1;
    }

    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positional_and_indexed() {
        let parts = vec![
            "\"Simultaneous\"".into(),
            "\"Sequential\"".into(),
            "22=\"BMW\"".into(),
        ];
        let opts = parse_enum_options(&parts, &HashMap::new());
        assert_eq!(opts[0].value, 0);
        assert_eq!(opts[0].label, "Simultaneous");
        assert_eq!(opts[2].value, 22);
        assert_eq!(opts[2].label, "BMW");
    }
}
