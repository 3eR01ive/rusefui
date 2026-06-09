//! Скан таблицы УОЗ: сравнение с моделью автогенерации + допуск.

use super::calculator::SparkAdvanceCalculator;
use super::engine::EngineParams;
use super::ModelCoefficients;

/// Допуск к модели (° BTDC): таблица может отличаться на пару градусов.
pub const MODEL_MARGIN_DEG: f64 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlausibilityKind {
    AboveModel,
    BelowModel,
}

#[derive(Debug, Clone)]
pub struct PlausibilityViolation {
    pub kind: PlausibilityKind,
    pub rpm: f64,
    pub map_kpa: f64,
    pub advance_deg: f64,
    pub expected_deg: f64,
}

/// Сканирует таблицу: флаг, если ячейка выходит за `model ± margin`.
pub fn scan_ignition_table(
    engine: &EngineParams,
    rpm_axis: &[f64],
    load_axis: &[f64],
    table: &[f64],
    margin_deg: f64,
) -> Result<Vec<PlausibilityViolation>, String> {
    let coef = ModelCoefficients::default_embedded()?;
    let calc = SparkAdvanceCalculator::new(engine.clone(), coef);
    let cols = rpm_axis.len();
    let rows = load_axis.len();
    if cols == 0 || rows == 0 || table.len() != rows * cols {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for (row, &map_kpa) in load_axis.iter().enumerate() {
        for (col, &rpm) in rpm_axis.iter().enumerate() {
            let advance_deg = table[row * cols + col];
            if advance_deg == 0.0 {
                continue;
            }
            let expected_deg = calc.advance_at(rpm, map_kpa);
            let upper = expected_deg + margin_deg;
            let lower = expected_deg - margin_deg;
            if advance_deg > upper {
                out.push(PlausibilityViolation {
                    kind: PlausibilityKind::AboveModel,
                    rpm,
                    map_kpa,
                    advance_deg,
                    expected_deg,
                });
            } else if advance_deg < lower {
                out.push(PlausibilityViolation {
                    kind: PlausibilityKind::BelowModel,
                    rpm,
                    map_kpa,
                    advance_deg,
                    expected_deg,
                });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ignition_map::EngineParams;

    fn sample_engine() -> EngineParams {
        EngineParams {
            displacement_cc: Some(2000.0),
            compression_ratio: 10.0,
            ..EngineParams::default()
        }
    }

    #[test]
    fn scan_flags_above_model() {
        let engine = sample_engine();
        let coef = ModelCoefficients::default_embedded().unwrap();
        let calc = SparkAdvanceCalculator::new(engine.clone(), coef);
        let rpm = 7000.0;
        let map = 108.0;
        let expected = calc.advance_at(rpm, map);
        let table = vec![expected + MODEL_MARGIN_DEG + 1.0];
        let v = scan_ignition_table(
            &engine,
            &[rpm],
            &[map],
            &table,
            MODEL_MARGIN_DEG,
        )
        .unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].kind, PlausibilityKind::AboveModel);
    }

    #[test]
    fn scan_accepts_within_margin() {
        let engine = sample_engine();
        let coef = ModelCoefficients::default_embedded().unwrap();
        let calc = SparkAdvanceCalculator::new(engine.clone(), coef);
        let rpm = 1073.0;
        let map = 33.0;
        let expected = calc.advance_at(rpm, map);
        let table = vec![expected + 1.0];
        let v = scan_ignition_table(
            &engine,
            &[rpm],
            &[map],
            &table,
            MODEL_MARGIN_DEG,
        )
        .unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn scan_flags_below_model() {
        let engine = sample_engine();
        let coef = ModelCoefficients::default_embedded().unwrap();
        let calc = SparkAdvanceCalculator::new(engine.clone(), coef);
        let rpm = 3000.0;
        let map = 50.0;
        let expected = calc.advance_at(rpm, map);
        let table = vec![expected - MODEL_MARGIN_DEG - 2.0];
        let v = scan_ignition_table(
            &engine,
            &[rpm],
            &[map],
            &table,
            MODEL_MARGIN_DEG,
        )
        .unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].kind, PlausibilityKind::BelowModel);
    }
}
