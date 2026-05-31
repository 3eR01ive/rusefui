mod calculator;
mod coefficients;
mod engine;

pub use calculator::{SparkAdvanceCalculator, SparkCell, SparkDiagnostics};
pub use coefficients::ModelCoefficients;
pub use engine::EngineParams;

/// Сгенерировать карту УОЗ для осей ECU (строки = load/MAP, столбцы = RPM).
pub fn generate_table_values(
    engine: &EngineParams,
    rpm_axis: &[f64],
    load_axis: &[f64],
) -> Result<Vec<f64>, String> {
    let coef = ModelCoefficients::default_embedded()?;
    let calc = SparkAdvanceCalculator::new(engine.clone(), coef);
    let cols = rpm_axis.len();
    let rows = load_axis.len();
    let mut flat = Vec::with_capacity(rows * cols);

    for storage_row in 0..rows {
        let map_kpa = load_axis[storage_row];
        for col in 0..cols {
            let rpm = rpm_axis[col];
            flat.push(calc.advance_at(rpm, map_kpa).advance_deg);
        }
    }

    Ok(flat)
}
