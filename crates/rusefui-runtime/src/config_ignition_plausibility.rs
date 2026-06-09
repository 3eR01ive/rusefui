//! Checklist «Подозрительное»: пороги УОЗ в таблице зажигания ECU.

use std::collections::HashMap;

use crate::config_checklist::{
    field_labels_for, resolve_editor, resolve_group, ChecklistIssue, ChecklistItem, ChecklistRules,
    LevelDefinition,
};
use crate::config_vars::{logic as var, ConfigVarResolver};
use crate::ignition_map::{
    boost_likely_from_load_axis, scan_ignition_table, worst_violation, ModelCoefficients,
    PlausibilityKind, PlausibilityViolation,
};
use crate::sources::config::{ConfigFieldInfo, ConfigSnapshot, ConfigSource};

const SUSPICIOUS_LEVEL: &str = "suspicious";

struct SuspiciousDef {
    id: &'static str,
    kind: PlausibilityKind,
    label: &'static str,
    message: &'static str,
}

const CHECKS: &[SuspiciousDef] = &[
    SuspiciousDef {
        id: "suspicious_ignition_wot",
        kind: PlausibilityKind::Wot,
        label: "УОЗ на полной нагрузке",
        message: "УОЗ на WOT выше порога plausibility",
    },
    SuspiciousDef {
        id: "suspicious_ignition_turbo",
        kind: PlausibilityKind::Turbo,
        label: "УОЗ при наддуве",
        message: "УОЗ при наддуве выше порога plausibility",
    },
    SuspiciousDef {
        id: "suspicious_ignition_idle",
        kind: PlausibilityKind::Idle,
        label: "УОЗ на холостом",
        message: "УОЗ на холостом выше порога plausibility",
    },
    SuspiciousDef {
        id: "suspicious_ignition_min",
        kind: PlausibilityKind::MinOperating,
        label: "Слишком малый УОЗ",
        message: "УОЗ ниже минимального рабочего порога plausibility",
    },
];

pub fn collect_ignition_plausibility_items(
    snapshot: &ConfigSnapshot,
    rules: &ChecklistRules,
    config: &ConfigSource,
    field_info: &HashMap<String, ConfigFieldInfo>,
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

    let Ok(coef) = ModelCoefficients::default_embedded() else {
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

    let boost_likely = boost_likely_from_load_axis(&coef, &load_axis);
    let violations = scan_ignition_table(&coef, &rpm_axis, &load_axis, &table, boost_likely);

    let level_def = rules.levels.get(SUSPICIOUS_LEVEL).cloned().unwrap_or_else(|| {
        LevelDefinition {
            title: "Подозрительное".to_string(),
            description: None,
            severity: "warning".to_string(),
        }
    });

    let fields = vec![table_field.to_string()];
    let field_labels = field_labels_for(rules, &fields);
    let editor = resolve_editor(rules, table_field);
    let (group, group_title, group_order) = resolve_group(rules, Some("ignition"));

    let mut items = Vec::with_capacity(CHECKS.len());
    let mut issues = Vec::new();

    for check in CHECKS {
        if check.kind == PlausibilityKind::Turbo && !boost_likely {
            continue;
        }
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

fn format_violation_display(v: &PlausibilityViolation) -> String {
    format!(
        "{:.1}° при {:.0} об/мин / {:.0} kPa (порог {:.1}°)",
        v.advance_deg, v.rpm, v.map_kpa, v.limit_deg
    )
}

fn format_issue_message(base: &str, v: &PlausibilityViolation) -> String {
    format!("{base}: {}", format_violation_display(v))
}
