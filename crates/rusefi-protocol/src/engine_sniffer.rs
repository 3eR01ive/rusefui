//! Разбор engine sniffer (`wave_chart`) из текстового `G`-потока rusEFI.
//!
//! Прошивка (`firmware/development/engine_sniffer.cpp`) публикует «логический
//! анализатор» как запись `wave_chart` в общем `G`-буфере:
//! `` `wave_chart` `` + `` ` `` + `<name>!<msg>!<time>!`-группы + `` ` ``.
//!
//! - `name` — имя канала (`t1`/`t2` для коленвала, имена пинов инжекторов/катушек,
//!   VVT, либо `r` = TDC, см. `TOP_DEAD_CENTER_MESSAGE`).
//! - `msg` — `u`/`d` (фронт вверх/вниз, `PROTOCOL_ES_UP`/`PROTOCOL_ES_DOWN`); для
//!   коленвала `u_<idx>`/`d_<idx>`; для TDC (`r`) это число оборотов (RPM).
//! - `time` — время от начала кадра в единицах `ENGINE_SNIFFER_UNIT_US` (10 µs).
//!
//! Каждый кадр самодостаточен (время с нуля) — это покадровый снимок, а не
//! непрерывный поток. Sniffer включается прошивкой автоматически при
//! `rpm < engineSnifferRpmThreshold`, отдельной команды на ECU не нужно.

/// `PROTOCOL_ENGINE_SNIFFER` + `LOG_DELIMITER` — начало кадра в `G`-потоке.
const FRAME_MARKER: &str = "wave_chart`";
/// `LOG_DELIMITER` — закрывающий разделитель записи.
const LOG_DELIMITER: char = '`';
/// `CHART_DELIMETER` из прошивки — разделитель полей внутри кадра.
const CHART_DELIMITER: char = '!';
/// `TOP_DEAD_CENTER_MESSAGE` — имя канала TDC (несёт RPM в `msg`).
const TDC_NAME: &str = "r";
/// `ENGINE_SNIFFER_UNIT_US` — единица времени на проводе (µs на тик).
const ENGINE_SNIFFER_UNIT_US: u64 = 10;

/// Верхняя граница буфера склейки: если кадры не приходят (RPM выше порога —
/// только `msg`), не растим carry бесконечно.
const MAX_CARRY: usize = 64 * 1024;
/// Сколько хвоста оставляем при переполнении carry.
const KEEP_CARRY: usize = 8 * 1024;

/// Одно событие логического анализатора.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnifferEvent {
    /// Имя канала (`t1`, `inj1`, `coil2`, vvt-имена, либо `r` для TDC).
    pub name: String,
    /// Время от начала кадра (µs).
    pub time_us: u64,
    /// `true` — фронт вверх (`u`), `false` — вниз (`d`). Для TDC всегда `true`.
    pub up: bool,
    /// `true`, если это TDC-маркер (`name == "r"`).
    pub tdc: bool,
    /// RPM из TDC-события (`msg`), иначе `None`.
    pub rpm: Option<u32>,
}

/// Состояние склейки `wave_chart` между чтениями `G` (кадр может прийти частями).
#[derive(Debug, Clone, Default)]
pub struct WaveChartParseState {
    carry: String,
}

impl WaveChartParseState {
    pub fn reset(&mut self) {
        self.carry.clear();
    }
}

/// Ближайшая валидная граница символа `>= idx`.
fn safe_cut(s: &str, mut idx: usize) -> usize {
    while idx < s.len() && !s.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

/// Разобрать тело одного кадра (`<name>!<msg>!<time>!`-группы).
fn parse_frame_payload(payload: &str) -> Vec<SnifferEvent> {
    let fields: Vec<&str> = payload.split(CHART_DELIMITER).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 <= fields.len() {
        let name = fields[i];
        let msg = fields[i + 1];
        let time_str = fields[i + 2];
        i += 3;

        if name.is_empty() {
            continue;
        }
        let Ok(time100) = time_str.trim().parse::<u64>() else {
            continue;
        };
        let time_us = time100.saturating_mul(ENGINE_SNIFFER_UNIT_US);

        if name == TDC_NAME {
            out.push(SnifferEvent {
                name: name.to_string(),
                time_us,
                up: true,
                tdc: true,
                rpm: msg.trim().parse::<u32>().ok(),
            });
        } else {
            out.push(SnifferEvent {
                name: name.to_string(),
                time_us,
                up: msg.starts_with('u'),
                tdc: false,
                rpm: None,
            });
        }
    }
    out
}

/// Разобрать кадры `wave_chart` из сырого `G`-текста.
///
/// Возвращает события **последнего полного кадра** (старые кадры пропускаются —
/// показываем актуальный снимок). Незавершённый хвост сохраняется в `state` и
/// дочитывается при следующем вызове.
pub fn parse_wave_chart(raw: &str, state: &mut WaveChartParseState) -> Vec<SnifferEvent> {
    state.carry.push_str(raw);
    if state.carry.len() > MAX_CARRY {
        let cut = safe_cut(&state.carry, state.carry.len() - KEEP_CARRY);
        state.carry.drain(..cut);
    }

    let buf = std::mem::take(&mut state.carry);

    let mut search = 0usize;
    let mut last_events: Option<Vec<SnifferEvent>> = None;
    let mut consumed_upto = 0usize;
    let mut partial_from: Option<usize> = None;

    while let Some(rel) = buf[search..].find(FRAME_MARKER) {
        let start = search + rel;
        let ps = start + FRAME_MARKER.len();
        match buf[ps..].find(LOG_DELIMITER) {
            Some(rel_end) => {
                let payload = &buf[ps..ps + rel_end];
                last_events = Some(parse_frame_payload(payload));
                consumed_upto = ps + rel_end + 1;
                search = consumed_upto;
            }
            None => {
                // Кадр ещё не закрыт — оставляем его начало в carry.
                partial_from = Some(start);
                break;
            }
        }
    }

    state.carry = match partial_from {
        Some(p) => buf[p..].to_string(),
        None => buf[consumed_upto..].to_string(),
    };

    last_events.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_frame_with_tdc() {
        let mut st = WaveChartParseState::default();
        let raw = "msg`hello`wave_chart`t1!u_0!10!t2!d!20!r!1500!30!`msg`bye`";
        let ev = parse_wave_chart(raw, &mut st);
        assert_eq!(
            ev,
            vec![
                SnifferEvent { name: "t1".into(), time_us: 100, up: true, tdc: false, rpm: None },
                SnifferEvent { name: "t2".into(), time_us: 200, up: false, tdc: false, rpm: None },
                SnifferEvent { name: "r".into(), time_us: 300, up: true, tdc: true, rpm: Some(1500) },
            ]
        );
    }

    #[test]
    fn no_frame_returns_empty() {
        let mut st = WaveChartParseState::default();
        assert!(parse_wave_chart("msg`only console`", &mut st).is_empty());
    }

    #[test]
    fn takes_latest_complete_frame() {
        let mut st = WaveChartParseState::default();
        let raw = "wave_chart`t1!u!1!`wave_chart`t2!d!2!`";
        let ev = parse_wave_chart(raw, &mut st);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].name, "t2");
        assert_eq!(ev[0].time_us, 20);
    }

    #[test]
    fn reassembles_frame_split_across_reads() {
        let mut st = WaveChartParseState::default();
        let first = parse_wave_chart("noise`wave_chart`t1!u!1!t2!d", &mut st);
        assert!(first.is_empty(), "кадр ещё не закрыт");

        let second = parse_wave_chart("!2!`tail", &mut st);
        assert_eq!(
            second,
            vec![
                SnifferEvent { name: "t1".into(), time_us: 10, up: true, tdc: false, rpm: None },
                SnifferEvent { name: "t2".into(), time_us: 20, up: false, tdc: false, rpm: None },
            ]
        );
    }

    #[test]
    fn carry_is_bounded_without_frames() {
        let mut st = WaveChartParseState::default();
        let chunk = "x".repeat(20_000);
        for _ in 0..10 {
            let _ = parse_wave_chart(&chunk, &mut st);
        }
        assert!(st.carry.len() <= MAX_CARRY);
    }
}
