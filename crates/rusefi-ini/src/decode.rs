use std::collections::HashMap;

use crate::model::{
    ArrayField, BitsField, ConfigFieldKind, FieldKind, OutputChannels, ScalarField,
    ScalarType, StringField,
};

/// Декодирует поля конфигурации (секция `[Constants]`).
pub fn decode_config_fields(
    fields: &HashMap<String, ConfigFieldKind>,
    bytes: &[u8],
) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for (name, field) in fields {
        if let Some(v) = decode_config_field(field, bytes) {
            out.insert(name.clone(), v);
        }
    }
    out
}

/// Декодирует строковые поля page 0.
pub fn decode_config_strings(
    fields: &HashMap<String, ConfigFieldKind>,
    bytes: &[u8],
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (name, field) in fields {
        if let ConfigFieldKind::String(s) = field {
            if let Some(v) = decode_string(s, bytes) {
                out.insert(name.clone(), v);
            }
        }
    }
    out
}

/// Декодирует только scalar-поля (совместимость).
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

/// Декодирует одно config-поле из образа page 0.
pub fn decode_config_at(field: &ConfigFieldKind, page: &[u8]) -> Option<f64> {
    decode_config_field(field, page)
}

fn decode_config_field(field: &ConfigFieldKind, bytes: &[u8]) -> Option<f64> {
    match field {
        ConfigFieldKind::Scalar(s) => decode_scalar(s, bytes),
        ConfigFieldKind::Enum(e) => decode_bits(&e.bits, bytes),
        ConfigFieldKind::Array(_) | ConfigFieldKind::String(_) => None,
    }
}

fn decode_string(field: &StringField, bytes: &[u8]) -> Option<String> {
    let off = field.offset as usize;
    let len = field.length as usize;
    if len == 0 || off + len > bytes.len() {
        return None;
    }
    let slice = &bytes[off..off + len];
    let end = slice.iter().position(|&b| b == 0).unwrap_or(len);
    Some(String::from_utf8_lossy(&slice[..end]).into_owned())
}

/// Кодирует строку для записи в page 0 (нуль-терминатор + padding).
pub fn encode_string_value(field: &StringField, value: &str) -> Option<Vec<u8>> {
    let len = field.length as usize;
    if len == 0 {
        return None;
    }
    let mut out = vec![0u8; len];
    let bytes = value.as_bytes();
    let copy_len = bytes.len().min(len.saturating_sub(1));
    out[..copy_len].copy_from_slice(&bytes[..copy_len]);
    Some(out)
}

/// Декодирует все элементы массива config.
pub fn decode_array(field: &ArrayField, bytes: &[u8]) -> Vec<f64> {
    let count = field.shape.element_count();
    let size = field.ty.size_bytes();
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let off = field.offset as usize + i * size;
        let raw = read_raw_at(field.ty, off, bytes).unwrap_or(0.0);
        out.push(raw * field.scale + field.translate);
    }
    out
}

/// Кодирует один элемент массива для записи через `C`.
pub fn encode_array_element(
    field: &ArrayField,
    index: usize,
    value: f64,
) -> Option<(u32, Vec<u8>)> {
    if index >= field.shape.element_count() {
        return None;
    }
    let raw = (value - field.translate) / field.scale;
    let bytes = encode_scalar_value(
        &ScalarField {
            ty: field.ty,
            offset: 0,
            units: String::new(),
            scale: 1.0,
            translate: 0.0,
        },
        raw,
    )?;
    let offset = field.offset + (index * field.ty.size_bytes()) as u32;
    Some((offset, bytes))
}

/// Кодирует значение config-поля (scalar или enum/bits).
pub fn encode_config_value(
    field: &ConfigFieldKind,
    value: f64,
    current_bytes: &[u8],
) -> Option<Vec<u8>> {
    match field {
        ConfigFieldKind::Scalar(s) => encode_scalar_value(s, value),
        ConfigFieldKind::Enum(e) => encode_bits_value(&e.bits, value, current_bytes),
        ConfigFieldKind::Array(_) | ConfigFieldKind::String(_) => None,
    }
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
        FieldKind::Array(a) => decode_array(a, bytes).first().copied(),
        FieldKind::String(_) => None,
    }
}

/// Декодирует одно scalar-поле из сырого образа page 0.
pub fn decode_scalar_at(field: &ScalarField, page: &[u8]) -> Option<f64> {
    decode_scalar(field, page)
}

fn decode_scalar(field: &ScalarField, bytes: &[u8]) -> Option<f64> {
    let raw = read_raw_at(field.ty, field.offset as usize, bytes)?;
    Some(raw * field.scale + field.translate)
}

/// Кодирует bits-поле с read-modify-write по текущему образу page.
pub fn encode_bits_value(field: &BitsField, value: f64, current_bytes: &[u8]) -> Option<Vec<u8>> {
    let size = field.ty.size_bytes();
    let off = field.offset as usize;
    let mut buf = vec![0u8; size];
    if off + size <= current_bytes.len() {
        buf.copy_from_slice(&current_bytes[off..off + size]);
    }
    let raw = read_raw_at(field.ty, 0, &buf)? as u32;
    let width = field.bit_high.saturating_sub(field.bit_low) + 1;
    let mask = if width >= 32 {
        u32::MAX
    } else {
        (1u32 << width) - 1
    };
    let v = (value as u32) & mask;
    let cleared = raw & !(mask << field.bit_low);
    let new_raw = cleared | (v << field.bit_low);
    write_raw(field.ty, 0, new_raw, &mut buf)?;
    Some(buf)
}

fn write_raw(ty: ScalarType, offset: u32, value: u32, bytes: &mut [u8]) -> Option<()> {
    let off = offset as usize;
    let size = ty.size_bytes();
    if off + size > bytes.len() {
        return None;
    }
    match (size, ty.is_signed()) {
        (1, false) => bytes[off] = value as u8,
        (1, true) => bytes[off] = value as i8 as u8,
        (2, false) => bytes[off..off + 2].copy_from_slice(&(value as u16).to_le_bytes()),
        (2, true) => bytes[off..off + 2].copy_from_slice(&(value as i16).to_le_bytes()),
        (4, false) => bytes[off..off + 4].copy_from_slice(&value.to_le_bytes()),
        (4, true) => bytes[off..off + 4].copy_from_slice(&(value as i32).to_le_bytes()),
        _ => return None,
    }
    Some(())
}

fn decode_bits(field: &BitsField, bytes: &[u8]) -> Option<f64> {
    let raw = read_raw_at(field.ty, field.offset as usize, bytes)? as u32;
    let width = field.bit_high.saturating_sub(field.bit_low) + 1;
    let mask = if width >= 32 {
        u32::MAX
    } else {
        (1u32 << width) - 1
    };
    let shifted = (raw >> field.bit_low) & mask;
    Some(shifted as f64)
}

fn read_raw_at(ty: ScalarType, offset: usize, bytes: &[u8]) -> Option<f64> {
    let size = ty.size_bytes();
    if offset + size > bytes.len() {
        return None;
    }
    let slice = &bytes[offset..offset + size];
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
    use crate::model::ConfigFieldKind;
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
        let ConfigFieldKind::Scalar(field) = ini.config_fields.get("triggerSimulatorRpm").unwrap()
        else {
            panic!("not scalar");
        };
        assert_eq!(field.offset, 436);

        let mut bytes = vec![0u8; 512];
        bytes[436] = 0x20;
        bytes[437] = 0x03;
        let map = decode_config_fields(&ini.config_fields, &bytes);
        assert_eq!(map.get("triggerSimulatorRpm"), Some(&800.0));

        let ConfigFieldKind::Enum(injection) = ini.config_fields.get("injectionMode").unwrap()
        else {
            panic!("injectionMode should be enum");
        };
        assert_eq!(injection.options.len(), 4);
        bytes[455] = 0x02;
        let map = decode_config_fields(&ini.config_fields, &bytes);
        assert_eq!(map.get("injectionMode"), Some(&2.0));
    }

    #[test]
    fn decode_vehicle_name_string() {
        let ini = IniFile::load_test_proteus().unwrap();
        let ConfigFieldKind::String(field) = ini.config_fields.get("vehicleName").unwrap()
        else {
            panic!("vehicleName should be string");
        };
        assert_eq!(field.offset, 1240);
        assert_eq!(field.length, 32);

        let mut bytes = vec![0u8; 1300];
        let name = b"Orange Miata";
        bytes[1240..1240 + name.len()].copy_from_slice(name);
        let strings = decode_config_strings(&ini.config_fields, &bytes);
        assert_eq!(strings.get("vehicleName"), Some(&"Orange Miata".to_string()));
    }
}
