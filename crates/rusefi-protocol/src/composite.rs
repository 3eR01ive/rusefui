//! Разбор tooth/composite logger: [`COMPOSITE_PACKET_SIZE`] байт на событие (как у ECU после `SWAP_UINT64`).

use crate::commands::COMPOSITE_PACKET_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompositeRecord {
    pub time_us: u64,
    pub pri_level: bool,
    pub sec_level: bool,
    /// Флаг камеры «trigger» в `composite_logger_s` (ветка CAM).
    pub cam_trigger: bool,
    pub sync: bool,
    /// Имеено `tdc` из ECU — в UI трактуем как канал «TDC» (`trg`).
    pub tdc: bool,
    pub coil: bool,
    pub injector: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CompositeParseState {
    prev_time: u32,
    time_adder: u64,
}

/// Раскладка записи composite на проводе — размер и битовые позиции полей.
///
/// Размер записи зависит от прошивки (`recordDef … recordLen` в INI: 8/10/…).
/// Прошивка реверсит все байты записи, поэтому запись = big-endian целое из
/// `record_len` байт, а поля берутся по `startBit` из `recordField` INI.
#[derive(Debug, Clone)]
pub struct CompositeLayout {
    pub record_len: usize,
    pub ref_time_bit: u32,
    pub ref_time_bits: u32,
    pub pri_bit: Option<u32>,
    pub sec_bit: Option<u32>,
    /// Канал «TDC» (`trg` на графике) — бит `tdc` из ECU.
    pub tdc_bit: Option<u32>,
    pub sync_bit: Option<u32>,
    /// Бит `trigger` (ветка CAM) — `cam_trigger`.
    pub cam_bit: Option<u32>,
    pub coil_bits: Vec<u32>,
    pub inj_bits: Vec<u32>,
}

impl Default for CompositeLayout {
    /// Совпадает с прошивкой текущего поколения (8-байтная запись).
    fn default() -> Self {
        Self {
            record_len: COMPOSITE_PACKET_SIZE,
            ref_time_bit: 0,
            ref_time_bits: 32,
            pri_bit: Some(32),
            sec_bit: Some(33),
            cam_bit: Some(34),
            sync_bit: Some(35),
            tdc_bit: Some(36),
            coil_bits: (40..48).collect(),
            inj_bits: (48..56).collect(),
        }
    }
}

/// Разобрать тело ответа `read_composite_buffer` с раскладкой по умолчанию (8 байт).
pub fn parse_composite_records(
    payload: &[u8],
    state: &mut CompositeParseState,
) -> Vec<CompositeRecord> {
    parse_composite_records_with(payload, state, &CompositeLayout::default())
}

/// Разобрать тело ответа `read_composite_buffer` с раскладкой из INI.
///
/// Запись `layout.record_len` байт собирается как big-endian целое (прошивка
/// реверсит байты записи), поля извлекаются по битовым позициям.
pub fn parse_composite_records_with(
    payload: &[u8],
    state: &mut CompositeParseState,
    layout: &CompositeLayout,
) -> Vec<CompositeRecord> {
    let n = layout.record_len;
    let mut out = Vec::new();
    // Запись должна влезать в u128 (composite — 8/10 байт). Иначе разбор невозможен.
    if n == 0 || n > 16 {
        return out;
    }

    let bit = |value: u128, b: Option<u32>| -> bool {
        match b {
            Some(b) => (value >> b) & 1 != 0,
            None => false,
        }
    };
    let ref_mask: u128 = if layout.ref_time_bits >= 128 {
        u128::MAX
    } else {
        (1u128 << layout.ref_time_bits) - 1
    };

    let mut ptr = 0usize;
    while ptr + n <= payload.len() {
        let mut value: u128 = 0;
        for &b in &payload[ptr..ptr + n] {
            value = (value << 8) | u128::from(b);
        }
        ptr += n;

        let ts_raw = ((value >> layout.ref_time_bit) & ref_mask) as u32;
        if ts_raw < state.prev_time {
            state.time_adder = state.time_adder.wrapping_add(0x1_0000_0000);
        }
        state.prev_time = ts_raw;
        let time_us = state.time_adder + u64::from(ts_raw);

        out.push(CompositeRecord {
            time_us,
            pri_level: bit(value, layout.pri_bit),
            sec_level: bit(value, layout.sec_bit),
            cam_trigger: bit(value, layout.cam_bit),
            sync: bit(value, layout.sync_bit),
            tdc: bit(value, layout.tdc_bit),
            coil: layout.coil_bits.iter().any(|&b| (value >> b) & 1 != 0),
            injector: layout.inj_bits.iter().any(|&b| (value >> b) & 1 != 0),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uint32_overflow_extends_time() {
        fn pack(ts_us: u32, fb: u8) -> [u8; 8] {
            let raw = u64::from(ts_us) | (u64::from(fb) << 32);
            raw.to_be_bytes()
        }

        let mut payload = Vec::new();
        payload.extend_from_slice(&pack(0xFFFF_FFF0, 0));
        payload.extend_from_slice(&pack(0x0000_0010, 0));

        let mut st = CompositeParseState::default();
        let rec = parse_composite_records(&payload, &mut st);
        assert_eq!(rec.len(), 2);
        assert_eq!(rec[0].time_us, 0xFFFF_FFF0);
        assert_eq!(rec[1].time_us, 0x1_0000_0010);
    }

    #[test]
    fn parses_10_byte_record_layout() {
        // Старая прошивка: запись 10 байт. Логическая запись (LE): ts(0..4),
        // flags(byte4), coil(byte5), inj(byte6), + 3 байта хвоста. На проводе все
        // 10 байт реверсированы → парсер собирает big-endian и берёт по битам.
        fn pack10(ts: u32, fb: u8, coil: u8, inj: u8) -> [u8; 10] {
            // логическая запись little-endian
            let mut logical = [0u8; 10];
            logical[0..4].copy_from_slice(&ts.to_le_bytes());
            logical[4] = fb;
            logical[5] = coil;
            logical[6] = inj;
            // на проводе — реверс всех байт записи
            let mut wire = logical;
            wire.reverse();
            wire
        }

        let layout = CompositeLayout {
            record_len: 10,
            ..CompositeLayout::default()
        };
        let mut payload = Vec::new();
        payload.extend_from_slice(&pack10(1000, 0x01, 0x02, 0x01)); // pri, coil2, inj1
        payload.extend_from_slice(&pack10(2000, 0x10, 0x00, 0x00)); // tdc

        let mut st = CompositeParseState::default();
        let rec = parse_composite_records_with(&payload, &mut st, &layout);
        assert_eq!(rec.len(), 2);
        assert_eq!(rec[0].time_us, 1000);
        assert!(rec[0].pri_level);
        assert!(rec[0].coil);
        assert!(rec[0].injector);
        assert_eq!(rec[1].time_us, 2000);
        assert!(rec[1].tdc);
        assert!(!rec[1].coil);
    }

    #[test]
    fn decodes_packed_flags_rpm_style_byte() {
        // pri + sync + coil/inj bitmask
        let ts: u32 = 123456;
        let fb: u8 = 0x01 | 0x08;
        let raw = u64::from(ts)
            | (u64::from(fb) << 32)
            | (0x03u64 << 40)
            | (0x01u64 << 48);
        let payload = raw.to_be_bytes();

        let mut st = CompositeParseState::default();
        let rec = parse_composite_records(&payload, &mut st);
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].time_us, u64::from(ts));
        assert!(rec[0].pri_level);
        assert!(rec[0].sync);
        assert!(rec[0].coil);
        assert!(rec[0].injector);
    }
}
