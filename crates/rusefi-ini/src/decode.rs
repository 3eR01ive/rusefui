use std::collections::HashMap;

use crate::model::{BitsField, FieldKind, OutputChannels, ScalarField, ScalarType};

/// Декодирует scalar-поля конфигурации (секция `[Constants]`).
pub fn decode_config_scalars(
    scalars: &HashMap<String, ScalarField>,
    bytes: &[u8],
) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for (name, field) in scalars {
        if let Some(v) = decode_scalar(field, bytes) {
            out.insert(name.clone(), v);
        }
    }
    out
}

/// Кодирует пользовательское значение в raw bytes для записи через `C`.
pub fn encode_scalar_value(field: &ScalarField, value: f64) -> Option<Vec<u8>> {
    let raw = (value - field.translate) / field.scale;
    match field.ty {
        ScalarType::F32 => {
            let v = raw as f32;
            Some(v.to_le_bytes().to_vec())
        }
        ScalarType::U08 => Some([raw.round().clamp(0.0, 255.0) as u8].to_vec()),
        ScalarType::S08 => Some([(raw.round().clamp(-128.0, 127.0) as i8) as u8].to_vec()),
        ScalarType::U16 => Some((raw.round().clamp(0.0, 65535.0) as u16).to_le_bytes().to_vec()),
        ScalarType::S16 => Some((raw.round().clamp(-32768.0, 32767.0) as i16).to_le_bytes().to_vec()),
        ScalarType::U32 => Some((raw.round().clamp(0.0, u32::MAX as f64) as u32).to_le_bytes().to_vec()),
        ScalarType::S32 => Some((raw.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32).to_le_bytes().to_vec()),
    }
}

/// Декодирует все scalar/bits поля из блока outputChannels.
pub fn decode_output_channels(
    channels: &OutputChannels,
    bytes: &[u8],
) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for field in &channels.fields {
        if let Some(v) = decode_field(&field.kind, bytes) {
            out.insert(field.name.clone(), v);
        }
    }
    out
}

fn decode_field(kind: &FieldKind, bytes: &[u8]) -> Option<f64> {
    match kind {
        FieldKind::Scalar(s) => decode_scalar(s, bytes),
        FieldKind::Bits(b) => decode_bits(b, bytes),
    }
}

fn decode_scalar(field: &ScalarField, bytes: &[u8]) -> Option<f64> {
    let raw = read_raw(field.ty, field.offset, bytes)?;
    Some(raw * field.scale + field.translate)
}

fn decode_bits(field: &BitsField, bytes: &[u8]) -> Option<f64> {
    let raw = read_raw(field.ty, field.offset, bytes)? as u32;
    let width = field.bit_high.saturating_sub(field.bit_low) + 1;
    let mask = if width >= 32 {
        u32::MAX
    } else {
        (1u32 << width) - 1
    };
    let shifted = (raw >> field.bit_low) & mask;
    Some(shifted as f64)
}

fn read_raw(ty: ScalarType, offset: u32, bytes: &[u8]) -> Option<f64> {
    let off = offset as usize;
    let size = ty.size_bytes();
    if off + size > bytes.len() {
        return None;
    }
    let slice = &bytes[off..off + size];
    if ty.is_float() {
        return Some(f32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as f64);
    }
    let signed = ty.is_signed();
    let v = match (size, signed) {
        (1, false) => slice[0] as f64,
        (1, true) => slice[0] as i8 as f64,
        (2, false) => u16::from_le_bytes([slice[0], slice[1]]) as f64,
        (2, true) => i16::from_le_bytes([slice[0], slice[1]]) as f64,
        (4, false) => u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as f64,
        (4, true) => i32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as f64,
        _ => return None,
    };
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IniFile;

    #[test]
    fn decode_rpm_from_fixture_offsets() {
        let ini = IniFile::load_test_proteus().unwrap();
        let rpm = ini.output_channels.field("RPMValue").unwrap();
        let ScalarField { offset, ty, scale, .. } = match &rpm.kind {
            FieldKind::Scalar(s) => s,
            _ => panic!("not scalar"),
        };
        assert_eq!(*offset, 4);
        assert_eq!(*ty, ScalarType::U16);

        let mut bytes = vec![0u8; 64];
        bytes[4] = 0x40;
        bytes[5] = 0x1F;
        let map = decode_output_channels(&ini.output_channels, &bytes);
        assert_eq!(map.get("RPMValue"), Some(&(8000.0 * scale)));
    }

    #[test]
    fn decode_trigger_simulator_rpm_from_constants() {
        let ini = IniFile::load_test_proteus().unwrap();
        let field = ini.config_scalars.get("triggerSimulatorRpm").unwrap();
        assert_eq!(field.offset, 436);

        let mut bytes = vec![0u8; 512];
        bytes[436] = 0x20;
        bytes[437] = 0x03;
        let map = decode_config_scalars(&ini.config_scalars, &bytes);
        assert_eq!(map.get("triggerSimulatorRpm"), Some(&800.0));
    }
}
