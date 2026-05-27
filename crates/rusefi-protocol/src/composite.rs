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

/// Разобрать сырое тело CRC-ответа на `read_composite_buffer` (`l` + READ).
///
/// На проводе — `SWAP_UINT64(entry->x)`: восстанавливаем `u64`, как на LE-MCU до swap,
/// и раскладываем packed `composite_logger_s` из `tooth_logger.h`.
pub fn parse_composite_records(
    payload: &[u8],
    state: &mut CompositeParseState,
) -> Vec<CompositeRecord> {
    let mut out = Vec::new();
    let mut ptr = 0usize;
    while ptr + COMPOSITE_PACKET_SIZE <= payload.len() {
        let chunk: [u8; COMPOSITE_PACKET_SIZE] = payload[ptr..ptr + COMPOSITE_PACKET_SIZE]
            .try_into()
            .expect("chunk len checked");
        ptr += COMPOSITE_PACKET_SIZE;

        let raw = u64::from_be_bytes(chunk);
        let ts_raw = raw as u32;

        if ts_raw < state.prev_time {
            state.time_adder = state.time_adder.wrapping_add(0x1_0000_0000);
        }
        state.prev_time = ts_raw;
        let time_us = state.time_adder + u64::from(ts_raw);

        let fb = ((raw >> 32) & 0xFF) as u8;
        let coil_b = ((raw >> 40) & 0xFF) as u8;
        let inj_b = ((raw >> 48) & 0xFF) as u8;

        out.push(CompositeRecord {
            time_us,
            pri_level: fb & 0x01 != 0,
            sec_level: fb & 0x02 != 0,
            cam_trigger: fb & 0x04 != 0,
            sync: fb & 0x08 != 0,
            tdc: fb & 0x10 != 0,
            coil: coil_b != 0,
            injector: inj_b != 0,
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
