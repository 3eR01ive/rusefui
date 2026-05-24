//! `#define` из INI (списки enum и pin list).

use std::collections::HashMap;

use crate::parse::split_ini_args;

/// Парсит `#define name="a", "b", …` в списки меток.
pub fn parse_ini_defines(text: &str) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("#define ") {
            continue;
        }
        let rest = line.strip_prefix("#define ").unwrap_or("").trim();
        let Some((name, value)) = rest.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if let Ok(labels) = split_ini_args(value.trim()) {
            if !labels.is_empty() {
                out.insert(name.to_string(), labels);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_define_list() {
        let text = r#"#define pin_output_mode_e_enum="default", "default inverted"
other = 1
"#;
        let defs = parse_ini_defines(text);
        let list = defs.get("pin_output_mode_e_enum").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], "default");
    }
}
