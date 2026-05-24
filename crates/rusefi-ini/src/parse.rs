use std::collections::HashMap;

use crate::defines::parse_ini_defines;
use crate::tables::{parse_array_shape, parse_table_and_curve_editors};
use crate::enum_options::parse_enum_options;
use crate::error::IniError;
use crate::model::{
    ArrayField, BitsField, ConfigFieldKind, EnumField, FieldKind, IniFile, OutputChannelField,
    OutputChannels, ScalarField, ScalarType,
};

#[derive(PartialEq, Eq)]
enum Section {
    None,
    MegaTune,
    TunerStudio,
    Constants,
    OutputChannels,
}

pub fn parse_ini(text: &str) -> Result<IniFile, IniError> {
    let defines = parse_ini_defines(text);
    let (tables, curves) = parse_table_and_curve_editors(text);
    let mut section = Section::None;
    let mut signature = None;
    let mut och_block_size = 2044u16;
    let mut blocking_factor = 1024u16;
    let mut page_size = 64_000u32;
    let mut page_read_has_page_index = true;
    let mut page_chunk_write_has_page_index = true;
    let mut fields = Vec::new();
    let mut config_fields = HashMap::new();

    for (line_no, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            section = match &line[1..line.len() - 1] {
                "MegaTune" => Section::MegaTune,
                "TunerStudio" => Section::TunerStudio,
                "Constants" => Section::Constants,
                "OutputChannels" => Section::OutputChannels,
                _ => Section::None,
            };
            continue;
        }

        if section == Section::MegaTune || section == Section::TunerStudio {
            if let Some(sig) = parse_key_value(line, "signature") {
                signature = Some(sig);
            }
            if section == Section::TunerStudio {
                if let Some(factor) = parse_key_u16(line, "blockingFactor") {
                    blocking_factor = factor;
                }
                if let Some(size) = parse_key_first_u32(line, "pageSize") {
                    page_size = size;
                }
            }
            continue;
        }

        if section == Section::Constants {
            if let Some(factor) = parse_key_u16(line, "blockingFactor") {
                blocking_factor = factor;
                continue;
            }
            if let Some(size) = parse_key_first_u32(line, "pageSize") {
                page_size = size;
                continue;
            }
            if let Some(cmd) = parse_key_first_quoted(line, "pageReadCommand") {
                // Как Java: pageReadCommand.length() == 7 → старый формат без page
                page_read_has_page_index = cmd.len() != 7;
                continue;
            }
            if let Some(cmd) = parse_key_first_quoted(line, "pageChunkWrite") {
                page_chunk_write_has_page_index = cmd.contains("%2i");
                continue;
            }
        }

        if section != Section::OutputChannels && section != Section::Constants {
            continue;
        }

        if section == Section::OutputChannels {
            if let Some(size) = parse_key_u16(line, "ochBlockSize") {
                och_block_size = size;
                continue;
            }
        }

        if line.contains('=') && (line.contains("scalar,") || line.contains("bits,") || line.contains("array,")) {
            match parse_config_or_output_line(line, &defines) {
                Ok(field) => {
                    if section == Section::OutputChannels {
                        if !matches!(field.kind, FieldKind::Array(_)) {
                            fields.push(OutputChannelField {
                                name: field.name,
                                kind: field.kind,
                            });
                        }
                    } else if section == Section::Constants {
                        if let Some((name, kind)) = field.into_config_kind() {
                            config_fields.insert(name, kind);
                        }
                    }
                }
                Err(e) => {
                    return Err(IniError::Parse(format!(
                        "line {}: {e} — `{line}`",
                        line_no + 1
                    )));
                }
            }
            continue;
        }
    }

    let mut output_channels = OutputChannels {
        och_block_size,
        fields,
        by_name: HashMap::new(),
    };
    output_channels.index_fields();

    Ok(IniFile {
        signature,
        blocking_factor,
        page_size,
        page_read_has_page_index,
        page_chunk_write_has_page_index,
        output_channels,
        config_fields,
        tables,
        curves,
    })
}

struct ParsedIniField {
    name: String,
    kind: FieldKind,
    enum_options: Vec<crate::model::EnumOption>,
}

impl ParsedIniField {
    fn into_config_kind(self) -> Option<(String, ConfigFieldKind)> {
        match self.kind {
            FieldKind::Scalar(scalar) => Some((self.name, ConfigFieldKind::Scalar(scalar))),
            FieldKind::Bits(bits) => {
                if self.enum_options.is_empty() {
                    None
                } else {
                    Some((
                        self.name,
                        ConfigFieldKind::Enum(EnumField {
                            bits,
                            options: self.enum_options,
                        }),
                    ))
                }
            }
            FieldKind::Array(array) => Some((self.name, ConfigFieldKind::Array(array))),
        }
    }
}

fn parse_key_value(line: &str, key: &str) -> Option<String> {
    let (k, v) = line.split_once('=')?;
    if k.trim() != key {
        return None;
    }
    let mut v = v.trim();
    if let Some(idx) = v.find(';') {
        v = v[..idx].trim();
    }
    let v = v.trim_matches('"');
    Some(v.to_string())
}

fn parse_key_u16(line: &str, key: &str) -> Option<u16> {
    let v = parse_key_value(line, key)?;
    v.split(',').next()?.trim().parse().ok()
}

fn parse_key_first_u32(line: &str, key: &str) -> Option<u32> {
    let v = parse_key_value(line, key)?;
    v.split(',').next()?.trim().parse().ok()
}

fn parse_key_first_quoted(line: &str, key: &str) -> Option<String> {
    let v = parse_key_value(line, key)?;
    let first = v.split(',').next()?.trim();
    Some(first.trim_matches('"').to_string())
}

fn parse_config_or_output_line(
    line: &str,
    defines: &HashMap<String, Vec<String>>,
) -> Result<ParsedIniField, String> {
    let (name, rest) = line.split_once('=').ok_or("missing '='")?;
    let name = name.trim().to_string();
    let parts = split_ini_args(rest.trim())?;
    if parts.is_empty() {
        return Err("empty definition".into());
    }

    let mut enum_options = Vec::new();

    let kind = match parts[0].as_str() {
        "scalar" => {
            if parts.len() < 3 {
                return Err("scalar needs at least type and offset".into());
            }
            let ty = ScalarType::parse(&parts[1]).ok_or("unknown scalar type")?;
            let offset: u32 = parts[2].parse().map_err(|_| "bad offset")?;
            let units = parts.get(3).cloned().unwrap_or_default();
            let scale: f64 = parts
                .get(4)
                .and_then(|s| s.parse().ok())
                .unwrap_or(1.0);
            let translate: f64 = parts
                .get(5)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            FieldKind::Scalar(ScalarField {
                ty,
                offset,
                units,
                scale,
                translate,
            })
        }
        "bits" => {
            if parts.len() < 4 {
                return Err("bits needs type, offset, [lo:hi]".into());
            }
            let ty = ScalarType::parse(&parts[1]).ok_or("unknown bits type")?;
            let offset: u32 = parts[2].parse().map_err(|_| "bad offset")?;
            let (low, high) = parse_bit_range(&parts[3])?;
            enum_options = parse_enum_options(&parts[4..], defines);
            FieldKind::Bits(BitsField {
                ty,
                offset,
                bit_low: low,
                bit_high: high,
            })
        }
        "array" => {
            if parts.len() < 4 {
                return Err("array needs type, offset, shape".into());
            }
            let ty = ScalarType::parse(&parts[1]).ok_or("unknown array type")?;
            let offset: u32 = parts[2].parse().map_err(|_| "bad offset")?;
            let shape = parse_array_shape(&parts[3]).ok_or("bad array shape")?;
            let units = parts.get(4).cloned().unwrap_or_default();
            let scale: f64 = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(1.0);
            let translate: f64 = parts.get(6).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let lo: f64 = parts.get(7).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let hi: f64 = parts.get(8).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let digits: u8 = parts.get(9).and_then(|s| s.parse().ok()).unwrap_or(0);
            FieldKind::Array(ArrayField {
                ty,
                offset,
                shape,
                units,
                scale,
                translate,
                lo,
                hi,
                digits,
            })
        }
        other => return Err(format!("unknown field kind: {other}")),
    };

    Ok(ParsedIniField {
        name,
        kind,
        enum_options,
    })
}

fn parse_bit_range(s: &str) -> Result<(u8, u8), String> {
    let s = s.trim();
    let inner = s
        .strip_prefix('[')
        .and_then(|x| x.strip_suffix(']'))
        .ok_or("bit range must be [lo:hi]")?;
    let (a, b) = inner
        .split_once(':')
        .ok_or("bit range must be lo:hi")?;
    let low: u8 = a.trim().parse().map_err(|_| "bad bit low")?;
    let high: u8 = b.trim().parse().map_err(|_| "bad bit high")?;
    Ok((low, high))
}

/// Разбор аргументов INI с учётом кавычек (после `field =`, `dialog =`, …).
pub fn split_ini_args(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;

    for ch in s.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(ch);
            }
            ',' if !in_quotes => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }

    Ok(out
        .into_iter()
        .map(|p| {
            let t = p.trim();
            if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
                t[1..t.len() - 1].to_string()
            } else {
                t.to_string()
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_proteus_fixture() {
        let ini = IniFile::load_test_proteus().expect("fixture ini");
        assert_eq!(
            ini.signature.as_deref(),
            Some("rusEFI master.2025.09.02.proteus_f7.4139280449")
        );
        assert_eq!(ini.output_channels.och_block_size, 2044);
        assert!(ini.output_channels.fields.len() > 100);
        assert!(ini.output_channels.field("RPMValue").is_some());
        assert!(ini.output_channels.field("coolant").is_some());
        assert_eq!(ini.page_size, 63900);
        assert!(ini.page_read_has_page_index);
        assert!(ini.config_fields.len() > 200);
        assert!(
            ini.config_fields
                .get("triggerSimulatorRpm")
                .is_some_and(|k| matches!(k, ConfigFieldKind::Scalar(_)))
        );
        assert!(
            ini.config_fields
                .get("injectionMode")
                .is_some_and(|k| matches!(k, ConfigFieldKind::Enum(_)))
        );
    }

    #[test]
    fn page_read_command_legacy_format() {
        let text = r#"
[Constants]
pageReadCommand = "R%2o%2c"
pageSize = 63900
"#;
        let ini = parse_ini(text).unwrap();
        assert!(!ini.page_read_has_page_index);
    }

    #[test]
    fn page_chunk_write_legacy_format() {
        let text = r#"
[Constants]
pageChunkWrite = "C%2o%2c%v"
pageSize = 63900
"#;
        let ini = parse_ini(text).unwrap();
        assert!(!ini.page_chunk_write_has_page_index);
    }

    #[test]
    fn page_chunk_write_with_page_index() {
        let text = r#"
[Constants]
pageChunkWrite = "C%2i%2o%2c%v"
pageSize = 63900
"#;
        let ini = parse_ini(text).unwrap();
        assert!(ini.page_chunk_write_has_page_index);
    }

    #[test]
    fn page_read_command_with_page_index() {
        let text = r#"
[Constants]
pageReadCommand = "R%2i%2o%2c", "R%2i%2o%2c"
pageSize = 63900
"#;
        let ini = parse_ini(text).unwrap();
        assert!(ini.page_read_has_page_index);
    }
}
