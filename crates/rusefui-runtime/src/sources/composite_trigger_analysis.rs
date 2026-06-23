//! Анализатор сбоев триггера по composite-логу.
//!
//! Самообучается по сигналу (без хардкода под конкретное колесо): берёт фронты
//! выбранного канала, находит «узкий зуб» (локальная медиана периодов), выводит
//! структуру оборота (секции между широкими пропусками) и ловит отклонения:
//! потерянный фронт (период ≈ кратному узкого = два зуба слились в фейковый
//! «широкий»), лишний фронт (≈½ узкого) и несовпадение счёта зубьев между
//! метками синхронизации.
//!
//! Цель — ответить «на каком именно зубе ломается декодирование» для любого
//! триггера, как это делает прошивочный isSyncPoint, но по записанному сигналу.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::composite_logger::CompositeEventJson;

/// Период считается «широким» (пропуск/слияние), если он ≥ этого × узкого.
const WIDE_FACTOR: f64 = 1.5;
/// Период считается «коротким» (лишний фронт/шум), если он ≤ этого × узкого.
const SHORT_FACTOR: f64 = 0.6;
/// Полупериод окна для локальной медианы «узкого зуба».
const NARROW_WIN: usize = 16;
/// Доля секций, при которой длина считается «штатной» (модальной).
const MODAL_MIN_SHARE: f64 = 0.12;
/// Период, многократно превышающий узкий, — стык сессии (не зуб), пропускаем.
const SESSION_GAP_FACTOR: f64 = 8.0;
/// Шаг гистограммы по оборотам.
const RPM_BUCKET: u32 = 250;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeTriggerParams {
    pub events: Vec<CompositeEventJson>,
    /// Канал: "pri" (по умолчанию) или "sec".
    #[serde(default)]
    pub channel: Option<String>,
    /// Фронт: "rise" (по умолчанию), "fall" или "both".
    #[serde(default)]
    pub edge_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerFault {
    /// Индекс зуба (периода) в потоке записи — для скролла/подсветки.
    pub tooth_index: u32,
    /// Порядковый номер физического зуба в цикле колеса (0-based, от метки
    /// синхры). None — если до первой синхры или структура не выведена.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle_tooth: Option<u32>,
    /// Номинальный слот (угол/шаг) в цикле от метки синхры — широкий пропуск
    /// занимает round(wide_ratio) слотов. None — как и cycle_tooth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle_slot: Option<u32>,
    /// Время фронта (µs, как в логе).
    pub t_us: u64,
    /// Доля позиции в проанализированном диапазоне (0..1) — для скролла
    /// независимо от базы времени (live ECU µs vs elapsed сек в review).
    pub pos: f64,
    /// Мгновенные обороты двигателя в точке сбоя.
    pub rpm: f64,
    /// "missedEdge" | "extraEdge" | "syncMismatch".
    pub kind: String,
    /// Человекочитаемое описание.
    pub detail: String,
    /// Измеренный период / локальный узкий зуб.
    pub ratio: f64,
    /// Для syncMismatch — насчитанное число зубьев между метками.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub teeth_counted: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RpmBucket {
    pub rpm_from: u32,
    pub count_total: u32,
    pub count_faults: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerAnalysis {
    pub channel: String,
    pub edge_mode: String,
    pub edges_used: u32,
    /// Удалось ли вывести структуру колеса.
    pub learned: bool,
    /// Физических зубьев за оборот (напр. 22).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub teeth_per_rev: Option<u32>,
    /// Номинальных слотов за оборот (зубья + пропуски, напр. 24) — для RPM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nominal_slots: Option<u32>,
    /// Отношение широкого пропуска к узкому (напр. ~2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wide_ratio: Option<f64>,
    /// Штатные длины секций между пропусками (напр. [7, 15]).
    pub section_pattern: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub narrow_us_min: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub narrow_us_max: Option<u64>,
    /// Сколько широких пропусков найдено всего.
    pub wide_gaps_total: u32,
    pub faults: Vec<TriggerFault>,
    pub fault_count: u32,
    pub fault_by_kind: BTreeMap<String, u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_fault_rpm: Option<f64>,
    pub rpm_histogram: Vec<RpmBucket>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl TriggerAnalysis {
    fn empty(channel: &str, edge_mode: &str, msg: &str) -> Self {
        Self {
            channel: channel.to_string(),
            edge_mode: edge_mode.to_string(),
            edges_used: 0,
            learned: false,
            teeth_per_rev: None,
            nominal_slots: None,
            wide_ratio: None,
            section_pattern: vec![],
            narrow_us_min: None,
            narrow_us_max: None,
            wide_gaps_total: 0,
            faults: vec![],
            fault_count: 0,
            fault_by_kind: BTreeMap::new(),
            first_fault_rpm: None,
            rpm_histogram: vec![],
            message: Some(msg.to_string()),
        }
    }
}

/// Уровень канала в событии.
fn level(ev: &CompositeEventJson, sec: bool) -> bool {
    if sec { ev.sec } else { ev.pri }
}

/// Времена выбранных фронтов канала (дедуплицируем повторы уровня).
fn edge_times(events: &[CompositeEventJson], sec: bool, mode: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut prev: Option<bool> = None;
    for ev in events {
        let lv = level(ev, sec);
        if let Some(p) = prev {
            if lv != p {
                let take = match mode {
                    "fall" => !lv,         // переход в 0
                    "both" => true,
                    _ => lv,               // "rise": переход в 1
                };
                if take {
                    out.push(ev.t_us);
                }
            }
        }
        prev = Some(lv);
    }
    out
}

fn median(v: &mut [u64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2] as f64
    } else {
        (v[n / 2 - 1] as f64 + v[n / 2] as f64) / 2.0
    }
}

/// Локальный «узкий зуб» для индекса периода: медиана окна (узкие в большинстве).
fn local_narrow(periods: &[u64], i: usize) -> f64 {
    let lo = i.saturating_sub(NARROW_WIN);
    let hi = (i + NARROW_WIN + 1).min(periods.len());
    let mut win: Vec<u64> = periods[lo..hi].to_vec();
    median(&mut win)
}

/// Штатные длины секций (модальные) + период повтора паттерна.
fn learn_pattern(sections: &[u32]) -> (Vec<u32>, u32) {
    if sections.is_empty() {
        return (vec![], 0);
    }
    let mut freq: BTreeMap<u32, u32> = BTreeMap::new();
    for &s in sections {
        *freq.entry(s).or_insert(0) += 1;
    }
    let min_count = ((sections.len() as f64) * MODAL_MIN_SHARE).ceil() as u32;
    let mut modal: Vec<u32> = freq
        .iter()
        .filter(|(_, &c)| c >= min_count.max(1))
        .map(|(&len, _)| len)
        .collect();
    modal.sort_unstable();
    // teeth_per_rev = сумма уникальных штатных длин (каждая раз за оборот).
    let teeth: u32 = modal.iter().sum();
    (modal, teeth)
}

pub fn analyze_trigger(params: &AnalyzeTriggerParams) -> TriggerAnalysis {
    let channel = params.channel.as_deref().unwrap_or("pri");
    let sec = channel == "sec";
    let mode = params.edge_mode.as_deref().unwrap_or("rise");

    let edges = edge_times(&params.events, sec, mode);
    if edges.len() < 8 {
        return TriggerAnalysis::empty(channel, mode, "Мало фронтов в логе");
    }

    // Периоды между фронтами + время конца каждого периода.
    let mut periods: Vec<u64> = Vec::with_capacity(edges.len() - 1);
    let mut times: Vec<u64> = Vec::with_capacity(edges.len() - 1);
    for w in edges.windows(2) {
        let dt = w[1].saturating_sub(w[0]);
        periods.push(dt);
        times.push(w[1]);
    }

    // Классификация периодов относительно локального узкого зуба.
    let n = periods.len();
    let mut ratios = vec![0.0_f64; n];
    let mut narrows = vec![0.0_f64; n];
    let mut narrow_min = u64::MAX;
    let mut narrow_max = 0u64;
    for i in 0..n {
        let nb = local_narrow(&periods, i);
        narrows[i] = nb;
        ratios[i] = if nb > 0.0 { periods[i] as f64 / nb } else { 0.0 };
        if nb > 0.0 {
            let nbu = nb as u64;
            narrow_min = narrow_min.min(nbu);
            narrow_max = narrow_max.max(nbu);
        }
    }

    // Широкие пропуски — индексы периодов ≥ WIDE_FACTOR (но не стыки сессии).
    let wide_idx: Vec<usize> = (0..n)
        .filter(|&i| ratios[i] >= WIDE_FACTOR && ratios[i] < SESSION_GAP_FACTOR)
        .collect();

    // Отношение широкого: медиана ratio широких пропусков.
    let wide_ratio = if wide_idx.is_empty() {
        None
    } else {
        let mut wr: Vec<u64> = wide_idx.iter().map(|&i| (ratios[i] * 1000.0) as u64).collect();
        Some(median(&mut wr) / 1000.0)
    };

    // Секции между соседними широкими пропусками.
    let sections: Vec<u32> = wide_idx
        .windows(2)
        .map(|w| (w[1] - w[0]) as u32)
        .collect();
    let (modal, teeth_per_rev) = learn_pattern(&sections);
    let learned = !modal.is_empty() && teeth_per_rev > 0;

    // Номинальных слотов = зубья + пропавшие слоты на пропуск (round(wide)-1).
    let wides_per_rev = modal.len() as u32; // по одной секции на пропуск за оборот
    let nominal_slots = if learned {
        let miss_per_wide = wide_ratio.map(|k| (k.round() as u32).saturating_sub(1)).unwrap_or(0);
        Some(teeth_per_rev + wides_per_rev * miss_per_wide)
    } else {
        None
    };
    // Делитель для RPM: номинальные слоты (J30: 24), иначе зубья, иначе мягкий фолбэк.
    let rpm_div = nominal_slots.or(if teeth_per_rev > 0 { Some(teeth_per_rev) } else { None });

    let rpm_at = |i: usize| -> f64 {
        let nb = narrows[i];
        match (rpm_div, nb > 0.0) {
            (Some(d), true) if d > 0 => 60_000_000.0 / (nb * d as f64),
            _ => 0.0,
        }
    };

    // Метки синхры = широкий пропуск, замыкающий короткую секцию (как точка
    // синхронизации прошивки). От них считаем порядковый номер зуба в цикле.
    let min_sec = modal.iter().min().copied();
    let wide_k = wide_ratio.map(|k| (k.round() as u32).max(1)).unwrap_or(1);
    let sync_marks: Vec<usize> = match min_sec {
        Some(ms) => wide_idx
            .windows(2)
            .filter(|w| (w[1] - w[0]) as u32 == ms)
            .map(|w| w[1])
            .collect(),
        None => vec![],
    };
    // Порядковый зуб и номинальный слот от ближайшей предыдущей метки синхры.
    let cycle_pos = |i: usize| -> Option<(u32, u32)> {
        let anchor = *sync_marks.iter().rev().find(|&&s| s <= i)?;
        let tooth = (i - anchor) as u32;
        let mut slot = 0u32;
        for j in (anchor + 1)..=i {
            let is_wide = ratios[j] >= WIDE_FACTOR && ratios[j] < SESSION_GAP_FACTOR;
            slot += if is_wide { wide_k } else { 1 };
        }
        Some((tooth, slot))
    };

    let mut faults: Vec<TriggerFault> = Vec::new();

    // 1) Лишние фронты — слишком короткий период (шумовой спайк).
    for i in 0..n {
        if ratios[i] > 0.0 && ratios[i] <= SHORT_FACTOR {
            faults.push(TriggerFault {
                tooth_index: i as u32,
                t_us: times[i],
                pos: 0.0,
                cycle_tooth: None,
                cycle_slot: None,
                rpm: rpm_at(i),
                kind: "extraEdge".into(),
                detail: format!("короткий период {:.2}× узкого — лишний/шумовой фронт", ratios[i]),
                ratio: round3(ratios[i]),
                teeth_counted: None,
            });
        }
    }

    // 2) Отклонения структуры: секция между пропусками вне штатного набора.
    if learned {
        let min_sec = *modal.iter().min().unwrap();
        let max_sec = *modal.iter().max().unwrap();
        for w in wide_idx.windows(2) {
            let a = w[0];
            let b = w[1];
            let seclen = (b - a) as u32;
            if modal.contains(&seclen) {
                continue;
            }
            let (kind, detail) = if seclen < min_sec {
                (
                    "missedEdge",
                    format!(
                        "секция {seclen} зуб. (ждали {modal:?}) — фейковый «широкий» внутри секции = потерян фронт"
                    ),
                )
            } else if seclen > max_sec {
                (
                    "missedEdge",
                    format!(
                        "секция {seclen} зуб. (ждали {modal:?}) — пропущен широкий пропуск = потерян фронт на нём"
                    ),
                )
            } else {
                (
                    "missedEdge",
                    format!("секция {seclen} зуб. вне штатного набора {modal:?}"),
                )
            };
            faults.push(TriggerFault {
                tooth_index: b as u32,
                t_us: times[b],
                pos: 0.0,
                cycle_tooth: None,
                cycle_slot: None,
                rpm: rpm_at(b),
                kind: kind.into(),
                detail,
                ratio: round3(ratios[b]),
                teeth_counted: Some(seclen),
            });
        }

        // 3) Счёт зубьев между метками синхры (короткая секция = точка синхры).
        for w in sync_marks.windows(2) {
            let count = (w[1] - w[0]) as u32;
            if count != teeth_per_rev {
                let b = w[1];
                faults.push(TriggerFault {
                    tooth_index: b as u32,
                    t_us: times[b],
                    pos: 0.0,
                    cycle_tooth: None,
                    cycle_slot: None,
                    rpm: rpm_at(b),
                    kind: "syncMismatch".into(),
                    detail: format!(
                        "между метками синхры {count} зуб. вместо {teeth_per_rev} — рассинхрон декодера"
                    ),
                    ratio: round3(ratios[b]),
                    teeth_counted: Some(count),
                });
            }
        }
    }

    faults.sort_by_key(|f| f.tooth_index);
    faults.dedup_by_key(|f| (f.tooth_index, f.kind.clone()));

    // Доля позиции в диапазоне фронтов — для скролла без привязки к базе времени.
    let t_first = *times.first().unwrap_or(&0);
    let t_last = *times.last().unwrap_or(&0);
    let t_span = t_last.saturating_sub(t_first) as f64;
    for f in &mut faults {
        if t_span > 0.0 {
            f.pos = ((f.t_us.saturating_sub(t_first)) as f64 / t_span).clamp(0.0, 1.0);
        }
        if let Some((tooth, slot)) = cycle_pos(f.tooth_index as usize) {
            f.cycle_tooth = Some(tooth);
            f.cycle_slot = Some(slot);
        }
    }

    // Гистограмма по оборотам.
    let mut buckets: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
    for i in 0..n {
        let r = rpm_at(i);
        if r <= 0.0 || !r.is_finite() {
            continue;
        }
        let bk = (r as u32 / RPM_BUCKET) * RPM_BUCKET;
        buckets.entry(bk).or_insert((0, 0)).0 += 1;
    }
    for f in &faults {
        if f.rpm > 0.0 && f.rpm.is_finite() {
            let bk = (f.rpm as u32 / RPM_BUCKET) * RPM_BUCKET;
            buckets.entry(bk).or_insert((0, 0)).1 += 1;
        }
    }
    let rpm_histogram: Vec<RpmBucket> = buckets
        .into_iter()
        .map(|(rpm_from, (ct, cf))| RpmBucket {
            rpm_from,
            count_total: ct,
            count_faults: cf,
        })
        .collect();

    let mut fault_by_kind: BTreeMap<String, u32> = BTreeMap::new();
    for f in &faults {
        *fault_by_kind.entry(f.kind.clone()).or_insert(0) += 1;
    }
    let first_fault_rpm = faults.iter().map(|f| f.rpm).find(|r| *r > 0.0);

    let message = if !learned {
        Some("Не удалось вывести структуру колеса (нет широких пропусков либо мало данных)".into())
    } else {
        None
    };

    TriggerAnalysis {
        channel: channel.to_string(),
        edge_mode: mode.to_string(),
        edges_used: edges.len() as u32,
        learned,
        teeth_per_rev: if teeth_per_rev > 0 { Some(teeth_per_rev) } else { None },
        nominal_slots,
        wide_ratio: wide_ratio.map(round3),
        section_pattern: modal,
        narrow_us_min: (narrow_min != u64::MAX).then_some(narrow_min),
        narrow_us_max: (narrow_max != 0).then_some(narrow_max),
        wide_gaps_total: wide_idx.len() as u32,
        fault_count: faults.len() as u32,
        faults,
        fault_by_kind,
        first_fault_rpm,
        rpm_histogram,
        message,
    }
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}
