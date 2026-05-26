//! Парсинг записей tooth/composite logger (5 байт на событие).

use crate::commands::COMPOSITE_PACKET_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompositeRecord {
    pub time_us: u64,
    pub pri_level: bool,
    pub sec_level: bool,
    pub trigger: bool,
    pub sync: bool,
    pub coil: bool,
    pub injector: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CompositeParseState {
    prev_time: u32,
    time_adder: u64,
}

/// Разобрать сырой payload CRC-ответа на `read_composite_buffer` / `l`+READ.
pub fn parse_composite_records(
    payload: &[u8],
    state: &mut CompositeParseState,
) -> Vec<CompositeRecord> {
    let mut out = Vec::new();
    let mut ptr = 0usize;
    while ptr + COMPOSITE_PACKET_SIZE <= payload.len() {
        let ts = u32::from_be_bytes([
            payload[ptr],
            payload[ptr + 1],
            payload[ptr + 2],
            payload[ptr + 3],
        ]);
        let flags = payload[ptr + 4];
        ptr += COMPOSITE_PACKET_SIZE;

        if ts < state.prev_time {
            state.time_adder = state.time_adder.wrapping_add(0x1_0000_0000);
        }
        state.prev_time = ts;
        let time_us = state.time_adder + u64::from(ts);

        out.push(CompositeRecord {
            time_us,
            pri_level: flags & 0x01 != 0,
            sec_level: flags & 0x02 != 0,
            trigger: flags & 0x04 != 0,
            sync: flags & 0x08 != 0,
            coil: flags & 0x10 != 0,
            injector: flags & 0x20 != 0,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uint32_overflow_extends_time() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&0xFFFF_FFF0u32.to_be_bytes());
        payload.push(0);
        payload.extend_from_slice(&0x0000_0010u32.to_be_bytes());
        payload.push(0);

        let mut st = CompositeParseState::default();
        let rec = parse_composite_records(&payload, &mut st);
        assert_eq!(rec.len(), 2);
        assert_eq!(rec[0].time_us, 0xFFFF_FFF0);
        assert_eq!(rec[1].time_us, 0x1_0000_0010);
    }
}
