use std::collections::HashMap;

use crate::error::IniError;
use crate::model::{
    BitsField, FieldKind, IniFile, OutputChannelField, OutputChannels, ScalarField, ScalarType,
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
    let mut section = Section::None;
    let mut signature = None;
    let mut och_block_size = 2044u16;
    let mut blocking_factor = 1024u16;
    let mut fields = Vec::new();
    let mut config_scalars = HashMap::new();

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
            }
            continue;
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

        if line.contains('=') && line.contains("scalar,") {
            match parse_output_line(line) {
                Ok(field) => {
                    if section == Section::OutputChannels {
                        fields.push(field);
                    } else if let FieldKind::Scalar(scalar) = field.kind {
                        config_scalars.insert(field.name, scalar);
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

        if section == Section::OutputChannels
            && line.contains('=')
            && line.contains("bits,")
        {
            match parse_output_line(line) {
                Ok(field) => fields.push(field),
                Err(e) => {
                    return Err(IniError::Parse(format!(
                        "line {}: {e} — `{line}`",
                        line_no + 1
                    )));
                }
            }
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
        output_channels,
        config_scalars,
    })
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
    v.parse().ok()
}

fn parse_output_line(line: &str) -> Result<OutputChannelField, String> {
    let (name, rest) = line
        .split_once('=')
        .ok_or("missing '='")?;
    let name = name.trim().to_string();
    let parts = split_ini_args(rest.trim())?;
    if parts.is_empty() {
        return Err("empty definition".into());
    }

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
            FieldKind::Bits(BitsField {
                ty,
                offset,
                bit_low: low,
                bit_high: high,
            })
        }
        other => return Err(format!("unknown field kind: {other}")),
    };

    Ok(OutputChannelField { name, kind })
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

/// Разбор аргументов после `scalar,` / `bits,` с учётом кавычек.
fn split_ini_args(s: &str) -> Result<Vec<String>, String> {
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
        assert!(ini.config_scalars.get("triggerSimulatorRpm").is_some());
    }
}
