use std::collections::HashMap;

use crate::model::{BitsField, FieldKind, OutputChannels, ScalarField, ScalarType};

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
}
