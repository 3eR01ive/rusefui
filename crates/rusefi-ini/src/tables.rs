//! Парсинг `[TableEditor]` и `[CurveEditor]` из INI.

use std::collections::HashMap;

use crate::menu::ini_rhs;
use crate::model::{IniCurveDef, IniTableDef};
use crate::parse::split_ini_args;

#[derive(PartialEq, Eq)]
enum EditorSection {
    None,
    Table,
    Curve,
}

pub fn parse_table_and_curve_editors(
    text: &str,
) -> (HashMap<String, IniTableDef>, HashMap<String, IniCurveDef>) {
    let mut section = EditorSection::None;
    let mut tables = HashMap::new();
    let mut curves = HashMap::new();
    let mut current_table: Option<IniTableDef> = None;
    let mut current_curve: Option<IniCurveDef> = None;

    for raw in text.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            flush_table(&mut current_table, &mut tables);
            flush_curve(&mut current_curve, &mut curves);
            section = match &trimmed[1..trimmed.len() - 1] {
                "TableEditor" => EditorSection::Table,
                "CurveEditor" => EditorSection::Curve,
                _ => EditorSection::None,
            };
            continue;
        }

        match section {
            EditorSection::Table => {
                if let Some(rest) = ini_rhs(trimmed, "table") {
                    flush_table(&mut current_table, &mut tables);
                    if let Ok(parts) = split_ini_args(rest) {
                        let id = parts.first().cloned().unwrap_or_default();
                        let map_id = parts.get(1).cloned();
                        let title = parts
                            .get(2)
                            .map(|s| s.trim_matches('"').to_string())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| id.clone());
                        current_table = Some(IniTableDef {
                            id: id.clone(),
                            title,
                            map_id,
                            x_bins: None,
                            y_bins: None,
                            z_bins: String::new(),
                            x_label: None,
                            y_label: None,
                        });
                    }
                    continue;
                }
                if let Some(table) = current_table.as_mut() {
                    apply_table_line(trimmed, table);
                }
            }
            EditorSection::Curve => {
                if let Some(rest) = ini_rhs(trimmed, "curve") {
                    flush_curve(&mut current_curve, &mut curves);
                    if let Ok(parts) = split_ini_args(rest) {
                        let id = parts.first().cloned().unwrap_or_default();
                        let title = parts
                            .get(1)
                            .map(|s| s.trim_matches('"').to_string())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| id.clone());
                        current_curve = Some(IniCurveDef {
                            id: id.clone(),
                            title,
                            x_bins: String::new(),
                            y_bins: String::new(),
                            x_label: None,
                            y_label: None,
                        });
                    }
                    continue;
                }
                if let Some(curve) = current_curve.as_mut() {
                    apply_curve_line(trimmed, curve);
                }
            }
            EditorSection::None => {}
        }
    }

    flush_table(&mut current_table, &mut tables);
    flush_curve(&mut current_curve, &mut curves);
    (tables, curves)
}

fn flush_table(current: &mut Option<IniTableDef>, out: &mut HashMap<String, IniTableDef>) {
    if let Some(table) = current.take() {
        if !table.z_bins.is_empty() {
            out.insert(table.id.clone(), table);
        }
    }
}

fn flush_curve(current: &mut Option<IniCurveDef>, out: &mut HashMap<String, IniCurveDef>) {
    if let Some(curve) = current.take() {
        if !curve.x_bins.is_empty() && !curve.y_bins.is_empty() {
            out.insert(curve.id.clone(), curve);
        }
    }
}

fn apply_table_line(line: &str, table: &mut IniTableDef) {
    if let Some(rest) = ini_rhs(line, "xyLabels") {
        let labels = parse_label_pair(rest);
        table.x_label = labels.0;
        table.y_label = labels.1;
        return;
    }
    if let Some(rest) = ini_rhs(line, "xBins") {
        table.x_bins = parse_bins_field(rest);
        return;
    }
    if let Some(rest) = ini_rhs(line, "yBins") {
        table.y_bins = parse_bins_field(rest);
        return;
    }
    if let Some(rest) = ini_rhs(line, "zBins") {
        table.z_bins = parse_bins_field(rest).unwrap_or_default();
    }
}

fn apply_curve_line(line: &str, curve: &mut IniCurveDef) {
    if let Some(rest) = ini_rhs(line, "columnLabel") {
        let labels = parse_label_pair(rest);
        curve.x_label = labels.0;
        curve.y_label = labels.1;
        return;
    }
    if let Some(rest) = ini_rhs(line, "xBins") {
        curve.x_bins = parse_bins_field(rest).unwrap_or_default();
        return;
    }
    if let Some(rest) = ini_rhs(line, "yBins") {
        curve.y_bins = parse_bins_field(rest).unwrap_or_default();
    }
}

fn parse_bins_field(rest: &str) -> Option<String> {
    let rest = rest.trim().trim_start_matches('=').trim();
    let parts = split_ini_args(rest).ok()?;
    let first = parts.first()?.trim().trim_start_matches('=').trim();
    if first.is_empty() || first.starts_with('{') {
        return None;
    }
    Some(first.to_string())
}

fn parse_label_pair(rest: &str) -> (Option<String>, Option<String>) {
    let parts = split_ini_args(rest).unwrap_or_default();
    let x = parts
        .first()
        .map(|s| s.trim_matches('"').to_string())
        .filter(|s| !s.is_empty() && !s.starts_with('{'));
    let y = parts
        .get(1)
        .map(|s| s.trim_matches('"').to_string())
        .filter(|s| !s.is_empty() && !s.starts_with('{'));
    (x, y)
}

pub fn parse_array_shape(s: &str) -> Option<crate::model::ArrayShape> {
    let inner = s.trim().trim_matches(['[', ']']);
    if inner.is_empty() {
        return None;
    }
    if let Some((a, b)) = inner.split_once('x') {
        let cols: usize = a.trim().parse().ok()?;
        let rows: usize = b.trim().parse().ok()?;
        Some(crate::model::ArrayShape::Matrix { cols, rows })
    } else {
        let n: usize = inner.parse().ok()?;
        Some(crate::model::ArrayShape::Vector(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ve_table_def() {
        let text = r#"
[TableEditor]
	table = veTableTbl, veTableMap, "VE Table", 1
		xyLabels = "RPM", "Load"
		xBins = veRpmBins, RPMValue
		yBins = veLoadBins, veTableYAxis
		zBins = veTable
"#;
        let (tables, _) = parse_table_and_curve_editors(text);
        let t = tables.get("veTableTbl").unwrap();
        assert_eq!(t.z_bins, "veTable");
        assert_eq!(t.x_bins.as_deref(), Some("veRpmBins"));
        assert_eq!(t.y_bins.as_deref(), Some("veLoadBins"));
    }

    #[test]
    fn parse_clt_curve() {
        let text = r#"
[CurveEditor]
	curve = cltFuelCorrCurve, "Warmup fuel"
		columnLabel = "CLT", "Mult"
		xBins = cltFuelCorrBins, coolant
		yBins = cltFuelCorr
"#;
        let (_, curves) = parse_table_and_curve_editors(text);
        let c = curves.get("cltFuelCorrCurve").unwrap();
        assert_eq!(c.x_bins, "cltFuelCorrBins");
        assert_eq!(c.y_bins, "cltFuelCorr");
    }
}
