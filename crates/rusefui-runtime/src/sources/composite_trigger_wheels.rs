//! Усреднённый диск: i-е событие после TDC во всех циклах → одно среднее (угол).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::composite_logger::CompositeEventJson;

/// Полный цикл коленвала между метками TDC (4-такт).
pub const CRANK_CYCLE_DEG: f64 = 720.0;
/// Диск коленвала: один полуцикл = 360° коленвала.
pub const CRANK_WHEEL_ARC_DEG: f64 = 360.0;
/// Диск распредвала: полный цикл TDC→TDC = 720° на окружности.
pub const CAM_WHEEL_ARC_DEG: f64 = 720.0;

const MIN_CYCLE_US: u64 = 2_000;
const MAX_CYCLE_US: u64 = 4_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WheelEdgeMode {
    Both,
    Rise,
    Fall,
}

impl WheelEdgeMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "rise" => Self::Rise,
            "fall" => Self::Fall,
            _ => Self::Both,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WheelEdgeKind {
    Rise,
    Fall,
    Tooth,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WheelTooth {
    pub angle_deg: f64,
    pub kind: WheelEdgeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fall_angle_deg: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerWheelDisk {
    pub label: String,
    pub teeth: Vec<WheelTooth>,
    pub arc_span_deg: f64,
    pub logical_tdc_deg: f64,
    pub offset_tdc_deg: Option<f64>,
    pub events_per_cycle: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerWheelsView {
    pub crank: TriggerWheelDisk,
    pub cam: TriggerWheelDisk,
    pub cycles_used: u32,
    pub cycles_seen: u32,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputeTriggerWheelsParams {
    pub events: Vec<CompositeEventJson>,
    pub edge_mode: String,
    #[serde(default)]
    pub trigger_angle_advance_deg: Option<f64>,
    #[serde(default)]
    pub physical_tdc_deg: Option<f64>,
}

fn normalize_arc(deg: f64, span: f64) -> f64 {
    if span <= 0.0 {
        return 0.0;
    }
    let mut d = deg.rem_euclid(span);
    if d >= span - 1e-6 {
        d = 0.0;
    }
    d
}

fn circular_mean_arc(angles: &[f64], span: f64) -> f64 {
    if angles.is_empty() {
        return 0.0;
    }
    let scale = std::f64::consts::TAU / span;
    let (s, c) = angles.iter().fold((0.0_f64, 0.0_f64), |(s, c), a| {
        let r = a * scale;
        (s + r.sin(), c + r.cos())
    });
    normalize_arc(s.atan2(c) / scale, span)
}

fn find_tdc_times(events: &[CompositeEventJson]) -> Vec<u64> {
    let mut from_field: Vec<u64> = events
        .iter()
        .filter_map(|e| {
            if e.tdc_cycle.is_some_and(|c| c > 0) {
                Some(e.t_us)
            } else {
                None
            }
        })
        .collect();
    if !from_field.is_empty() {
        from_field.sort_unstable();
        from_field.dedup();
        return from_field;
    }

    if events.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    let mut prev = events[0].trg;
    if prev {
        out.push(events[0].t_us);
    }
    for e in events.iter().skip(1) {
        if e.trg && !prev {
            out.push(e.t_us);
        }
        prev = e.trg;
    }
    out
}

fn sync_at(events: &[CompositeEventJson], t_us: u64) -> bool {
    if events.is_empty() {
        return false;
    }
    let mut val = events[0].sync;
    for e in events {
        if e.t_us > t_us {
            break;
        }
        val = e.sync;
    }
    val
}

fn extract_edges_in_range(
    events: &[CompositeEventJson],
    t0: u64,
    t1: u64,
    pri: bool,
) -> Vec<(u64, WheelEdgeKind)> {
    let mut out = Vec::new();
    let mut prev: Option<bool> = None;
    for e in events {
        if e.t_us <= t0 {
            prev = Some(if pri { e.pri } else { e.sec });
            continue;
        }
        if e.t_us >= t1 {
            break;
        }
        let cur = if pri { e.pri } else { e.sec };
        if let Some(p) = prev {
            if !p && cur {
                out.push((e.t_us, WheelEdgeKind::Rise));
            } else if p && !cur {
                out.push((e.t_us, WheelEdgeKind::Fall));
            }
        }
        prev = Some(cur);
    }
    out
}

/// Схлопнуть дребезг: два фронта одного типа ближе min_us — одно событие.
fn debounce_edges(edges: Vec<(u64, WheelEdgeKind)>, min_us: u64) -> Vec<(u64, WheelEdgeKind)> {
    if min_us == 0 {
        return edges;
    }
    let mut out = Vec::new();
    let mut last_rise: Option<u64> = None;
    let mut last_fall: Option<u64> = None;
    for (t, k) in edges {
        match k {
            WheelEdgeKind::Rise => {
                if last_rise.is_none_or(|l| t.saturating_sub(l) >= min_us) {
                    out.push((t, k));
                    last_rise = Some(t);
                }
            }
            WheelEdgeKind::Fall => {
                if last_fall.is_none_or(|l| t.saturating_sub(l) >= min_us) {
                    out.push((t, k));
                    last_fall = Some(t);
                }
            }
            WheelEdgeKind::Tooth => {}
        }
    }
    out
}

fn edges_to_angles(
    edges: &[(u64, WheelEdgeKind)],
    t0: u64,
    period_us: u64,
    arc_span_deg: f64,
) -> Vec<(f64, WheelEdgeKind)> {
    if period_us == 0 {
        return vec![];
    }
    let p = period_us as f64;
    edges
        .iter()
        .map(|(t, k)| {
            let frac = (*t - t0) as f64 / p;
            (normalize_arc(frac * arc_span_deg, arc_span_deg), *k)
        })
        .collect()
}

fn edges_of_kind_in_order(edges: &[(f64, WheelEdgeKind)], kind: WheelEdgeKind) -> Vec<f64> {
    edges
        .iter()
        .filter(|(_, k)| *k == kind)
        .map(|(a, _)| *a)
        .collect()
}

fn tooth_pairs_in_order(edges: &[(f64, WheelEdgeKind)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut pending_rise: Option<f64> = None;
    for (a, k) in edges {
        match k {
            WheelEdgeKind::Rise => pending_rise = Some(*a),
            WheelEdgeKind::Fall => {
                if let Some(r) = pending_rise.take() {
                    out.push((r, *a));
                }
            }
            WheelEdgeKind::Tooth => {}
        }
    }
    out
}

fn most_common_count(counts: &[usize]) -> usize {
    if counts.is_empty() {
        return 0;
    }
    let mut freq: HashMap<usize, usize> = HashMap::new();
    for &c in counts {
        *freq.entry(c).or_insert(0) += 1;
    }
    freq.into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(k, _)| k)
        .unwrap_or(0)
}

/// Ровно `n` маркеров: средний угол i-го события по всем циклам, где ровно `n` событий.
fn average_by_index_rise(
    cycles: &[Vec<(f64, WheelEdgeKind)>],
    n: usize,
    arc_span_deg: f64,
) -> (Vec<WheelTooth>, u32) {
    let filtered: Vec<&Vec<(f64, WheelEdgeKind)>> = cycles
        .iter()
        .filter(|c| edges_of_kind_in_order(c, WheelEdgeKind::Rise).len() == n)
        .collect();
    let used = filtered.len() as u32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let angles: Vec<f64> = filtered
            .iter()
            .map(|c| edges_of_kind_in_order(c, WheelEdgeKind::Rise)[i])
            .collect();
        out.push(WheelTooth {
            angle_deg: circular_mean_arc(&angles, arc_span_deg),
            kind: WheelEdgeKind::Rise,
            fall_angle_deg: None,
        });
    }
    (out, used)
}

fn average_by_index_fall(
    cycles: &[Vec<(f64, WheelEdgeKind)>],
    n: usize,
    arc_span_deg: f64,
) -> (Vec<WheelTooth>, u32) {
    let filtered: Vec<&Vec<(f64, WheelEdgeKind)>> = cycles
        .iter()
        .filter(|c| edges_of_kind_in_order(c, WheelEdgeKind::Fall).len() == n)
        .collect();
    let used = filtered.len() as u32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let angles: Vec<f64> = filtered
            .iter()
            .map(|c| edges_of_kind_in_order(c, WheelEdgeKind::Fall)[i])
            .collect();
        out.push(WheelTooth {
            angle_deg: circular_mean_arc(&angles, arc_span_deg),
            kind: WheelEdgeKind::Fall,
            fall_angle_deg: None,
        });
    }
    (out, used)
}

fn average_by_index_tooth(
    cycles: &[Vec<(f64, WheelEdgeKind)>],
    n: usize,
    arc_span_deg: f64,
) -> (Vec<WheelTooth>, u32) {
    let filtered: Vec<&Vec<(f64, WheelEdgeKind)>> = cycles
        .iter()
        .filter(|c| tooth_pairs_in_order(c).len() == n)
        .collect();
    let used = filtered.len() as u32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let rises: Vec<f64> = filtered.iter().map(|c| tooth_pairs_in_order(c)[i].0).collect();
        let falls: Vec<f64> = filtered.iter().map(|c| tooth_pairs_in_order(c)[i].1).collect();
        out.push(WheelTooth {
            angle_deg: circular_mean_arc(&rises, arc_span_deg),
            kind: WheelEdgeKind::Tooth,
            fall_angle_deg: Some(circular_mean_arc(&falls, arc_span_deg)),
        });
    }
    (out, used)
}

fn count_for_mode(cycle: &[(f64, WheelEdgeKind)], mode: WheelEdgeMode) -> usize {
    match mode {
        WheelEdgeMode::Rise => edges_of_kind_in_order(cycle, WheelEdgeKind::Rise).len(),
        WheelEdgeMode::Fall => edges_of_kind_in_order(cycle, WheelEdgeKind::Fall).len(),
        WheelEdgeMode::Both => tooth_pairs_in_order(cycle).len(),
    }
}

fn build_disk(
    label: &str,
    samples: &[Vec<(f64, WheelEdgeKind)>],
    mode: WheelEdgeMode,
    arc_span_deg: f64,
    offset_mark: Option<f64>,
) -> (TriggerWheelDisk, u32) {
    let counts: Vec<usize> = samples
        .iter()
        .map(|c| count_for_mode(c, mode))
        .filter(|&n| n > 0)
        .collect();

    let n = most_common_count(&counts);
    if n == 0 {
        return (
            TriggerWheelDisk {
                label: label.to_string(),
                teeth: vec![],
                arc_span_deg,
                logical_tdc_deg: 0.0,
                offset_tdc_deg: offset_mark,
                events_per_cycle: 0,
            },
            0,
        );
    }

    let (teeth, used) = match mode {
        WheelEdgeMode::Rise => average_by_index_rise(samples, n, arc_span_deg),
        WheelEdgeMode::Fall => average_by_index_fall(samples, n, arc_span_deg),
        WheelEdgeMode::Both => average_by_index_tooth(samples, n, arc_span_deg),
    };

    debug_assert_eq!(teeth.len(), n);

    (
        TriggerWheelDisk {
            label: label.to_string(),
            teeth,
            arc_span_deg,
            logical_tdc_deg: 0.0,
            offset_tdc_deg: offset_mark,
            events_per_cycle: n as u32,
        },
        used,
    )
}

fn offset_tdc_deg(
    physical_tdc_deg: Option<f64>,
    trigger_angle_advance_deg: Option<f64>,
    arc_span_deg: f64,
) -> Option<f64> {
    if let Some(p) = physical_tdc_deg {
        return Some(normalize_arc(p, arc_span_deg));
    }
    trigger_angle_advance_deg.map(|adv| normalize_arc(-adv, arc_span_deg))
}

pub fn compute_trigger_wheels(params: &ComputeTriggerWheelsParams) -> TriggerWheelsView {
    let mode = WheelEdgeMode::parse(&params.edge_mode);
    let events = &params.events;
    let offset_crank = offset_tdc_deg(
        params.physical_tdc_deg,
        params.trigger_angle_advance_deg,
        CRANK_WHEEL_ARC_DEG,
    );
    let offset_cam = offset_tdc_deg(
        params.physical_tdc_deg,
        params.trigger_angle_advance_deg,
        CAM_WHEEL_ARC_DEG,
    );

    let empty = |label: &str, arc: f64, off: Option<f64>| TriggerWheelDisk {
        label: label.to_string(),
        teeth: vec![],
        arc_span_deg: arc,
        logical_tdc_deg: 0.0,
        offset_tdc_deg: off,
        events_per_cycle: 0,
    };

    if events.len() < 4 {
        return TriggerWheelsView {
            crank: empty("Коленвал", CRANK_WHEEL_ARC_DEG, offset_crank),
            cam: empty("Распредвал", CAM_WHEEL_ARC_DEG, offset_cam),
            cycles_used: 0,
            cycles_seen: 0,
            message: Some("Мало событий в логе".into()),
        };
    }

    let tdc_times = find_tdc_times(events);
    if tdc_times.len() < 2 {
        return TriggerWheelsView {
            crank: empty("Коленвал", CRANK_WHEEL_ARC_DEG, offset_crank),
            cam: empty("Распредвал", CAM_WHEEL_ARC_DEG, offset_cam),
            cycles_used: 0,
            cycles_seen: 0,
            message: Some("Нет пары TDC".into()),
        };
    }

    let mut crank_samples: Vec<Vec<(f64, WheelEdgeKind)>> = Vec::new();
    let mut cam_samples: Vec<Vec<(f64, WheelEdgeKind)>> = Vec::new();
    let mut full_cycles_seen = 0u32;
    let mut crank_half_cycles_seen = 0u32;

    for w in tdc_times.windows(2) {
        let t0 = w[0];
        let t1 = w[1];
        let period = t1.saturating_sub(t0);
        if period < MIN_CYCLE_US || period > MAX_CYCLE_US {
            continue;
        }
        if !sync_at(events, t0) {
            continue;
        }
        full_cycles_seen += 1;

        let half_us = period / 2;
        let t_mid = t0.saturating_add(half_us);
        let min_edge_crank = (half_us / 80).max(50);
        let min_edge_cam = (period / 80).max(50);

        // Коленвал: два полуцикла на каждый TDC→TDC (360° + 360°), усреднение по всем полуциклам.
        for (t_start, t_end) in [(t0, t_mid), (t_mid, t1)] {
            let pri_raw = extract_edges_in_range(events, t_start, t_end, true);
            let pri = debounce_edges(pri_raw, min_edge_crank);
            if pri.is_empty() {
                continue;
            }
            crank_half_cycles_seen += 1;
            crank_samples.push(edges_to_angles(
                &pri,
                t_start,
                half_us,
                CRANK_WHEEL_ARC_DEG,
            ));
        }

        // Распредвал: целый цикл TDC→TDC, диск 720°.
        let sec_raw = extract_edges_in_range(events, t0, t1, false);
        let sec = debounce_edges(sec_raw, min_edge_cam);
        if !sec.is_empty() {
            cam_samples.push(edges_to_angles(&sec, t0, period, CAM_WHEEL_ARC_DEG));
        }
    }

    let (crank, crank_half_used) =
        build_disk("Коленвал", &crank_samples, mode, CRANK_WHEEL_ARC_DEG, offset_crank);
    let (cam, cam_full_used) =
        build_disk("Распредвал", &cam_samples, mode, CAM_WHEEL_ARC_DEG, offset_cam);

    let mode_lbl = match mode {
        WheelEdgeMode::Rise => "↑",
        WheelEdgeMode::Fall => "↓",
        WheelEdgeMode::Both => "↕",
    };
    let message = if crank.events_per_cycle == 0 && cam.events_per_cycle == 0 {
        Some("Нет одинаковых циклов для усреднения".into())
    } else {
        Some(format!(
            "{mode_lbl} · коленвал 360°: {} линий, усреднено по {crank_half_used} полуциклам \
             (всего {crank_half_cycles_seen}, ~2× на TDC→TDC) · распредвал 720°: {} линий, \
             {cam_full_used} полных циклов из {full_cycles_seen}",
            crank.events_per_cycle,
            cam.events_per_cycle,
        ))
    };

    TriggerWheelsView {
        crank,
        cam,
        cycles_used: crank_half_used.max(cam_full_used),
        cycles_seen: full_cycles_seen,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(t: u64, pri: bool, sync: bool) -> CompositeEventJson {
        CompositeEventJson {
            t_us: t,
            pri,
            sec: false,
            trg: false,
            sync,
            coil: false,
            inj: false,
            tdc_cycle: None,
        }
    }

    #[test]
    fn crank_two_half_cycles_per_full_cycle() {
        let mut events = Vec::new();
        for c in 0..3u64 {
            let base = c * 200_000;
            events.push(CompositeEventJson {
                t_us: base,
                pri: false,
                sec: false,
                trg: false,
                sync: true,
                coil: false,
                inj: false,
                tdc_cycle: Some(c + 1),
            });
            for i in 0..8u32 {
                let t = base + 10_000 + u64::from(i) * 22_000;
                events.push(ev(t, true, true));
                events.push(ev(t + 3_000, false, true));
            }
            events.push(CompositeEventJson {
                t_us: base + 200_000,
                pri: false,
                sec: false,
                trg: false,
                sync: true,
                coil: false,
                inj: false,
                tdc_cycle: Some(c + 2),
            });
        }
        let view = compute_trigger_wheels(&ComputeTriggerWheelsParams {
            events,
            edge_mode: "rise".into(),
            trigger_angle_advance_deg: None,
            physical_tdc_deg: None,
        });
        assert_eq!(view.crank.arc_span_deg, 360.0);
        assert_eq!(view.cam.arc_span_deg, 720.0);
        assert_eq!(view.crank.teeth.len(), view.crank.events_per_cycle as usize);
        // 2 полных цикла × 2 полуцикла = 4 выборки для усреднения
        assert!(view.cycles_used >= 2);
    }
}
