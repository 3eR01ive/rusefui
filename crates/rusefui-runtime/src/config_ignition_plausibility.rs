//! Checklist «Подозрительное»: УОЗ vs модель автогенерации.

use std::collections::HashMap;

use crate::config_checklist::{
    field_labels_for, resolve_editor, resolve_group, ChecklistIssue, ChecklistItem, ChecklistRules,
    LevelDefinition,
};
use crate::config_vars::{logic as var, ConfigVarResolver};
use crate::ignition_map::{
    EngineParams,
    scan_ignition_table, PlausibilityKind, PlausibilityViolation, MODEL_MARGIN_DEG,
};
use crate::sources::config::{ConfigFieldInfo, ConfigSnapshot, ConfigSource};

const SUSPICIOUS_LEVEL: &str = "suspicious";

struct PlausCheckDef {
    id: &'static str,
    label: &'static str,
    message: &'static str,
    kind: PlausibilityKind,
}

const CHECKS: &[PlausCheckDef] = &[
    PlausCheckDef {
        id: "suspicious_ignition_above_model",
        label: "УОЗ выше модели автогенерации",
        message: "В таблице есть ячейки с углом заметно выше расчёта по параметрам генерации",
        kind: PlausibilityKind::AboveModel,
    },
    PlausCheckDef {
        id: "suspicious_ignition_below_model",
        label: "УОЗ ниже модели автогенерации",
        message: "В таблице есть ячейки с углом заметно ниже расчёта по параметрам генерации",
        kind: PlausibilityKind::BelowModel,
    },
];

pub(crate) fn collect_ignition_plausibility_items(
    snapshot: &ConfigSnapshot,
    rules: &ChecklistRules,
    config: &ConfigSource,
    field_info: &HashMap<String, ConfigFieldInfo>,
    engine: &EngineParams,
) -> (Vec<ChecklistItem>, Vec<ChecklistIssue>) {
    if !snapshot.loaded {
        return (Vec::new(), Vec::new());
    }

    let vars = ConfigVarResolver::new(&rules.vars, &rules.conflict_constants, field_info);
    let Some(table_field) = vars.field_name(var::IGNITION_TABLE) else {
        return (Vec::new(), Vec::new());
    };
    let Some(rpm_field) = vars.field_name(var::IGNITION_RPM_BINS) else {
        return (Vec::new(), Vec::new());
    };
    let Some(load_field) = vars.field_name(var::IGNITION_LOAD_BINS) else {
        return (Vec::new(), Vec::new());
    };

    let Ok(rpm_axis) = config.get_array(rpm_field) else {
        return (Vec::new(), Vec::new());
    };
    let Ok(load_axis) = config.get_array(load_field) else {
        return (Vec::new(), Vec::new());
    };
    let Ok(table) = config.get_array(table_field) else {
        return (Vec::new(), Vec::new());
    };
    if table.iter().all(|&v| v == 0.0) {
        return (Vec::new(), Vec::new());
    }

    let violations = match scan_ignition_table(
        engine,
        &rpm_axis,
        &load_axis,
        &table,
        MODEL_MARGIN_DEG,
    ) {
        Ok(v) => v,
        Err(_) => return (Vec::new(), Vec::new()),
    };

    let level_def = rules
        .levels
        .get(SUSPICIOUS_LEVEL)
        .cloned()
        .unwrap_or_else(|| LevelDefinition {
            title: "Подозрительное".to_string(),
            description: None,
            severity: "warning".to_string(),
        });

    let fields = vec![table_field.to_string()];
    let field_labels = field_labels_for(rules, &fields);
    let editor = resolve_editor(rules, table_field);
    let (group, group_title, group_order) = resolve_group(rules, Some("ignition"));

    let mut items = Vec::with_capacity(CHECKS.len());
    let mut issues = Vec::new();

    for check in CHECKS {
        let worst = worst_violation(&violations, check.kind);
        let ok = worst.is_none();
        let value_display = worst
            .map(format_violation_display)
            .unwrap_or_else(|| "в норме".to_string());

        items.push(ChecklistItem {
            id: check.id.to_string(),
            level: SUSPICIOUS_LEVEL.to_string(),
            group: group.clone(),
            group_title: group_title.clone(),
            group_order,
            label: check.label.to_string(),
            ok,
            message: check.message.to_string(),
            value_display,
            fields: fields.clone(),
            field_labels: field_labels.clone(),
            editor: editor.clone(),
            editors: Vec::new(),
        });

        if let Some(worst) = worst {
            issues.push(ChecklistIssue {
                id: check.id.to_string(),
                level: SUSPICIOUS_LEVEL.to_string(),
                level_title: level_def.title.clone(),
                severity: level_def.severity.clone(),
                message: format_issue_message(check.message, worst),
                fields: fields.clone(),
                field_labels: field_labels.clone(),
            });
        }
    }

    (items, issues)
}

fn worst_violation<'a>(
    violations: &'a [PlausibilityViolation],
    kind: PlausibilityKind,
) -> Option<&'a PlausibilityViolation> {
    violations.iter().filter(|v| v.kind == kind).max_by(|a, b| {
        let dev_a = (a.advance_deg - a.expected_deg).abs();
        let dev_b = (b.advance_deg - b.expected_deg).abs();
        dev_a
            .partial_cmp(&dev_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn format_violation_display(v: &PlausibilityViolation) -> String {
    format!(
        "{:.1}° при {:.0} об/мин / {:.0} kPa (модель {:.1}°, допуск ±{:.0}°)",
        v.advance_deg, v.rpm, v.map_kpa, v.expected_deg, MODEL_MARGIN_DEG,
    )
}

fn format_issue_message(base: &str, v: &PlausibilityViolation) -> String {
    format!("{base}: {}", format_violation_display(v))
}
