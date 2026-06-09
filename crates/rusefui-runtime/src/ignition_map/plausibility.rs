//! Пороги «подозрительного» УОЗ — из coefficients JSON (как в research/ignition-advance-static).

use super::coefficients::ModelCoefficients;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlausibilityKind {
    Wot,
    Turbo,
    Idle,
    MinOperating,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlausibilityViolation {
    pub kind: PlausibilityKind,
    pub rpm: f64,
    pub map_kpa: f64,
    pub advance_deg: f64,
    pub limit_deg: f64,
}

/// Сканирует 2D-таблицу (строка = load/Y, столбец = RPM/X), порядок как в `generate_table_values`.
pub fn scan_ignition_table(
    coef: &ModelCoefficients,
    rpm_axis: &[f64],
    load_axis: &[f64],
    table: &[f64],
    boost_likely: bool,
) -> Vec<PlausibilityViolation> {
    let cols = rpm_axis.len();
    let rows = load_axis.len();
    if cols == 0 || rows == 0 || table.len() != rows * cols {
        return Vec::new();
    }

    let mut out = Vec::new();
    for (row, &map_kpa) in load_axis.iter().enumerate() {
        for (col, &rpm) in rpm_axis.iter().enumerate() {
            let advance = table[row * cols + col];
            if advance == 0.0 {
                continue;
            }
            let is_wot = map_kpa >= coef.wot_map_threshold_kpa;
            let is_idle = rpm <= coef.idle_rpm_max && map_kpa <= coef.idle_map_max_kpa;

            if is_wot && advance > coef.plausibility_max_wot_deg {
                out.push(PlausibilityViolation {
                    kind: PlausibilityKind::Wot,
                    rpm,
                    map_kpa,
                    advance_deg: advance,
                    limit_deg: coef.plausibility_max_wot_deg,
                });
            }
            // Только ячейки в зоне наддува (MAP выше атмосферной опоры), не весь vacuum-столбец.
            if boost_likely
                && map_kpa > coef.load_reference_map_kpa + 5.0
                && advance > coef.plausibility_max_turbo_deg
            {
                out.push(PlausibilityViolation {
                    kind: PlausibilityKind::Turbo,
                    rpm,
                    map_kpa,
                    advance_deg: advance,
                    limit_deg: coef.plausibility_max_turbo_deg,
                });
            }
            if is_idle && advance > coef.plausibility_max_idle_deg {
                out.push(PlausibilityViolation {
                    kind: PlausibilityKind::Idle,
                    rpm,
                    map_kpa,
                    advance_deg: advance,
                    limit_deg: coef.plausibility_max_idle_deg,
                });
            }
            if advance < coef.plausibility_min_operating_deg {
                out.push(PlausibilityViolation {
                    kind: PlausibilityKind::MinOperating,
                    rpm,
                    map_kpa,
                    advance_deg: advance,
                    limit_deg: coef.plausibility_min_operating_deg,
                });
            }
        }
    }
    out
}

pub fn boost_likely_from_load_axis(coef: &ModelCoefficients, load_axis: &[f64]) -> bool {
    let ref_map = coef.load_reference_map_kpa;
    load_axis.iter().any(|&m| m > ref_map + 5.0)
}

pub fn worst_violation<'a>(
    violations: &'a [PlausibilityViolation],
    kind: PlausibilityKind,
) -> Option<&'a PlausibilityViolation> {
    violations
        .iter()
        .filter(|v| v.kind == kind)
        .max_by(|a, b| {
            a.advance_deg
                .abs()
                .partial_cmp(&b.advance_deg.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ignition_map::coefficients::ModelCoefficients;

    #[test]
    fn turbo_plausibility_only_in_boost_map_cells() {
        let coef = ModelCoefficients::default_embedded().expect("coef");
        let rpm = vec![7000.0];
        let load = vec![27.0, 220.0];
        let table = vec![42.3, 30.0];
        let v = scan_ignition_table(&coef, &rpm, &load, &table, true);
        assert!(
            !v.iter()
                .any(|x| x.kind == PlausibilityKind::Turbo && x.map_kpa < 100.0),
            "vacuum MAP must not use turbo threshold: {v:?}"
        );
        assert!(
            v.iter()
                .any(|x| x.kind == PlausibilityKind::Turbo && x.map_kpa > 200.0),
            "boost MAP should flag: {v:?}"
        );
    }

    #[test]
    fn flags_wot_and_idle_cells() {
        let coef = ModelCoefficients::default_embedded().expect("coef");
        let rpm = vec![600.0, 4000.0];
        let load = vec![30.0, 220.0];
        let table = vec![
            40.0, 12.0, // idle rpm + high advance
            12.0, 45.0, // WOT high advance
        ];
        let v = scan_ignition_table(&coef, &rpm, &load, &table, false);
        assert!(v.iter().any(|x| x.kind == PlausibilityKind::Idle));
        assert!(v.iter().any(|x| x.kind == PlausibilityKind::Wot));
    }
}
