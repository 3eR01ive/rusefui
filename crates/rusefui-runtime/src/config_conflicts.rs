//! Взаимоисключающие настройки: дубли пинов, цилиндры vs выходы, режимы vs пины, триггер vs входы.

use std::collections::HashMap;

use crate::config_checklist::{
    field_labels_for, pin_label_for_value, resolve_editors, resolve_group, ChecklistEditor,
    ChecklistIssue, ChecklistItem, ChecklistRules, LevelDefinition,
};
use crate::sources::config::{ConfigFieldInfo, ConfigSnapshot};
use crate::sources::pin_allocation::PIN_NONE_VALUE;

const CONFLICT_LEVEL: &str = "conflicts";

/// Типы триггера, для которых в INI показан вторичный вход (`triggerInputPins2`).
const TRIGGER_NEEDS_SECONDARY: &[u32] = &[
    1, 3, 15, 16, 19, 25, 31, 35, 36, 37, 40, 49, 54, 63, 64,
];

const MAX_IGNITION_PINS: usize = 12;
const MAX_INJECTION_PINS: usize = 12;

struct ConflictDef {
    id: String,
    group: &'static str,
    label: String,
    message: String,
    value_display: String,
    /// Все связанные поля (подписи, issue).
    fields: Vec<String>,
    /// Что открыть справа — только стороны конфликта, без полей из startup checklist.
    editor_fields: Vec<String>,
}

pub fn collect_conflict_items(
    snapshot: &ConfigSnapshot,
    rules: &ChecklistRules,
    field_info: &HashMap<String, ConfigFieldInfo>,
) -> (Vec<ChecklistItem>, Vec<ChecklistIssue>) {
    let mut defs = Vec::new();
    defs.extend(pin_pool_conflicts(snapshot, rules, field_info));
    defs.extend(cylinder_pin_conflicts(snapshot));
    defs.extend(ignition_mode_pin_conflicts(snapshot, field_info));
    defs.extend(injection_mode_pin_conflicts(snapshot, field_info));
    defs.extend(trigger_input_conflicts(snapshot, field_info));

    defs.sort_by(|a, b| a.id.cmp(&b.id));

    let level_def = rules.levels.get(CONFLICT_LEVEL).cloned().unwrap_or_else(|| {
        LevelDefinition {
            title: "Конфликты настроек".to_string(),
            description: None,
            severity: "critical".to_string(),
        }
    });

    let mut items = Vec::with_capacity(defs.len());
    let mut issues = Vec::with_capacity(defs.len());

    for def in defs {
        let field_labels = field_labels_for(rules, &def.fields);
        let editors = resolve_editors(rules, &def.editor_fields);
        let editor = editors
            .first()
            .cloned()
            .unwrap_or_else(|| ChecklistEditor {
                panel: String::new(),
                component: None,
                field: def
                    .editor_fields
                    .first()
                    .or_else(|| def.fields.first())
                    .cloned()
                    .unwrap_or_default(),
            });
        let (group, group_title, group_order) = resolve_group(rules, Some(def.group));

        items.push(ChecklistItem {
            id: def.id.clone(),
            level: CONFLICT_LEVEL.to_string(),
            group: group.clone(),
            group_title: group_title.clone(),
            group_order,
            label: def.label.clone(),
            ok: false,
            message: def.message.clone(),
            value_display: def.value_display.clone(),
            fields: def.fields.clone(),
            field_labels: field_labels.clone(),
            editor,
            editors,
        });

        issues.push(ChecklistIssue {
            id: def.id,
            level: CONFLICT_LEVEL.to_string(),
            level_title: level_def.title.clone(),
            severity: level_def.severity.clone(),
            message: def.message,
            fields: def.fields,
            field_labels,
        });
    }

    (items, issues)
}

fn pin_pool_conflicts(
    snapshot: &ConfigSnapshot,
    rules: &ChecklistRules,
    field_info: &HashMap<String, ConfigFieldInfo>,
) -> Vec<ConflictDef> {
    let mut out = Vec::new();
    for (pool, pool_map) in &snapshot.pin_usage {
        for (value, users) in pool_map {
            if users.len() < 2 {
                continue;
            }
            let mut fields = users.clone();
            fields.sort();
            out.push(ConflictDef {
                id: format!("pin_conflict_{pool}_{value}"),
                group: "pins",
                label: pin_label_for_value(field_info, &fields, *value),
                message: "Один пин назначен нескольким функциям".to_string(),
                value_display: field_labels_for(rules, &fields).join(", "),
                fields: fields.clone(),
                editor_fields: fields,
            });
        }
    }
    out
}

fn cylinder_pin_conflicts(snapshot: &ConfigSnapshot) -> Vec<ConflictDef> {
    let mut out = Vec::new();
    let cylinders = cylinder_count(snapshot);
    if cylinders == 0 {
        return out;
    }

    if subsystem_enabled(snapshot, "isIgnitionEnabled") {
        for i in (cylinders + 1)..=MAX_IGNITION_PINS {
            let pin_field = format!("ignitionPins{i}");
            if pin_is_assigned(snapshot, &pin_field) {
                out.push(ConflictDef {
                    id: format!("conflict_cyl_ignition_pin_{i}"),
                    group: "engine",
                    label: format!("Зажигание: выход {i}"),
                    message: format!(
                        "Выход зажигания №{i} назначен, но цилиндров только {cylinders}"
                    ),
                    value_display: format!("{cylinders} цил., выход {i} занят"),
                    fields: vec!["cylindersCount".to_string(), pin_field.clone()],
                    editor_fields: vec![pin_field],
                });
            }
        }
    }

    if subsystem_enabled(snapshot, "isInjectionEnabled") && injection_mode(snapshot) != 3 {
        for i in (cylinders + 1)..=MAX_INJECTION_PINS {
            let pin_field = format!("injectionPins{i}");
            if pin_is_assigned(snapshot, &pin_field) {
                out.push(ConflictDef {
                    id: format!("conflict_cyl_injection_pin_{i}"),
                    group: "engine",
                    label: format!("Впрыск: выход {i}"),
                    message: format!(
                        "Выход форсунки №{i} назначен, но цилиндров только {cylinders}"
                    ),
                    value_display: format!("{cylinders} цил., выход {i} занят"),
                    fields: vec!["cylindersCount".to_string(), pin_field.clone()],
                    editor_fields: vec![pin_field],
                });
            }
        }
    }

    out
}

fn ignition_mode_pin_conflicts(
    snapshot: &ConfigSnapshot,
    field_info: &HashMap<String, ConfigFieldInfo>,
) -> Vec<ConflictDef> {
    let mut out = Vec::new();
    if !subsystem_enabled(snapshot, "isIgnitionEnabled") {
        return out;
    }

    let mode = ignition_mode(snapshot);
    let mode_label = scalar_option_label(field_info, "ignitionMode", mode)
        .unwrap_or_else(|| format!("режим {mode}"));

    if mode == 0 {
        for i in 2..=MAX_IGNITION_PINS {
            let pin_field = format!("ignitionPins{i}");
            if pin_is_assigned(snapshot, &pin_field) {
                out.push(ConflictDef {
                    id: format!("conflict_ignition_single_coil_{i}"),
                    group: "ignition",
                    label: format!("Single Coil + выход {i}"),
                    message: format!(
                        "В режиме «{mode_label}» используется только выход катушки 1"
                    ),
                    value_display: mode_label.clone(),
                    fields: vec!["ignitionMode".to_string(), pin_field.clone()],
                    editor_fields: vec![pin_field],
                });
            }
        }
    }

    if mode == 1 {
        let cylinders = cylinder_count(snapshot);
        for i in 1..=cylinders.min(MAX_IGNITION_PINS) {
            let pin_field = format!("ignitionPins{i}");
            if !pin_is_assigned(snapshot, &pin_field) {
                out.push(ConflictDef {
                    id: format!("conflict_ignition_individual_missing_{i}"),
                    group: "ignition",
                    label: format!("Individual: выход {i}"),
                    message: format!(
                        "В режиме «{mode_label}» нужен отдельный выход на каждый цилиндр"
                    ),
                    value_display: format!("цил. {i} без выхода"),
                    fields: vec!["ignitionMode".to_string(), pin_field.clone()],
                    editor_fields: vec![pin_field],
                });
            }
        }
    }

    out
}

fn injection_mode_pin_conflicts(
    snapshot: &ConfigSnapshot,
    field_info: &HashMap<String, ConfigFieldInfo>,
) -> Vec<ConflictDef> {
    let mut out = Vec::new();
    if !subsystem_enabled(snapshot, "isInjectionEnabled") {
        return out;
    }

    let mode = injection_mode(snapshot);
    let mode_label = scalar_option_label(field_info, "injectionMode", mode)
        .unwrap_or_else(|| format!("режим {mode}"));

    if mode == 3 {
        for i in 2..=MAX_INJECTION_PINS {
            let pin_field = format!("injectionPins{i}");
            if pin_is_assigned(snapshot, &pin_field) {
                out.push(ConflictDef {
                    id: format!("conflict_injection_single_point_{i}"),
                    group: "fuel",
                    label: format!("Single Point + выход {i}"),
                    message: format!(
                        "В режиме «{mode_label}» используется только выход форсунки 1"
                    ),
                    value_display: mode_label.clone(),
                    fields: vec!["injectionMode".to_string(), pin_field.clone()],
                    editor_fields: vec![pin_field],
                });
            }
        }
    }

    if mode == 1 {
        let cylinders = cylinder_count(snapshot);
        for i in 1..=cylinders.min(MAX_INJECTION_PINS) {
            let pin_field = format!("injectionPins{i}");
            if !pin_is_assigned(snapshot, &pin_field) {
                out.push(ConflictDef {
                    id: format!("conflict_injection_sequential_missing_{i}"),
                    group: "fuel",
                    label: format!("Sequential: выход {i}"),
                    message: format!(
                        "В режиме «{mode_label}» нужен отдельный выход на каждый цилиндр"
                    ),
                    value_display: format!("цил. {i} без выхода"),
                    fields: vec!["injectionMode".to_string(), pin_field.clone()],
                    editor_fields: vec![pin_field],
                });
            }
        }
    }

    out
}

fn trigger_input_conflicts(
    snapshot: &ConfigSnapshot,
    field_info: &HashMap<String, ConfigFieldInfo>,
) -> Vec<ConflictDef> {
    let mut out = Vec::new();
    let trigger_type = scalar_u32(snapshot, "trigger_type");
    let type_label = scalar_option_label(field_info, "trigger_type", trigger_type)
        .unwrap_or_else(|| format!("тип {trigger_type}"));

    if TRIGGER_NEEDS_SECONDARY.contains(&trigger_type)
        && !pin_is_assigned(snapshot, "triggerInputPins2")
    {
        out.push(ConflictDef {
            id: format!("conflict_trigger_secondary_missing_{trigger_type}"),
            group: "trigger",
            label: "Вторичный вход триггера".to_string(),
            message: format!(
                "Для «{type_label}» нужен вторичный вход (Secondary channel)"
            ),
            value_display: type_label.clone(),
            fields: vec![
                "trigger_type".to_string(),
                "triggerInputPins2".to_string(),
            ],
            editor_fields: vec!["triggerInputPins2".to_string()],
        });
    }

    if !TRIGGER_NEEDS_SECONDARY.contains(&trigger_type)
        && pin_is_assigned(snapshot, "triggerInputPins2")
    {
        out.push(ConflictDef {
            id: "conflict_trigger_secondary_unused".to_string(),
            group: "trigger",
            label: "Лишний вторичный вход".to_string(),
            message: format!(
                "Для «{type_label}» вторичный вход не используется — сбросьте Secondary channel"
            ),
            value_display: type_label,
            fields: vec![
                "trigger_type".to_string(),
                "triggerInputPins2".to_string(),
            ],
            editor_fields: vec!["triggerInputPins2".to_string()],
        });
    }

    out
}

fn scalar_option_label(
    field_info: &HashMap<String, ConfigFieldInfo>,
    field: &str,
    value: u32,
) -> Option<String> {
    let info = field_info.get(field)?;
    let options = info.options.as_ref()?;
    options
        .iter()
        .find(|o| o.value == value)
        .map(|o| o.label.clone())
}

fn subsystem_enabled(snapshot: &ConfigSnapshot, field: &str) -> bool {
    scalar_value(snapshot, field)
        .map(|v| v >= 1.0)
        .unwrap_or(false)
}

fn cylinder_count(snapshot: &ConfigSnapshot) -> usize {
    scalar_value(snapshot, "cylindersCount")
        .map(|v| v.round().max(0.0) as usize)
        .unwrap_or(0)
}

fn ignition_mode(snapshot: &ConfigSnapshot) -> u32 {
    scalar_u32(snapshot, "ignitionMode")
}

fn injection_mode(snapshot: &ConfigSnapshot) -> u32 {
    scalar_u32(snapshot, "injectionMode")
}

fn scalar_u32(snapshot: &ConfigSnapshot, field: &str) -> u32 {
    scalar_value(snapshot, field).map(|v| v as u32).unwrap_or(0)
}

fn pin_is_assigned(snapshot: &ConfigSnapshot, field: &str) -> bool {
    scalar_value(snapshot, field)
        .map(|v| v as u32 > PIN_NONE_VALUE)
        .unwrap_or(false)
}

fn scalar_value(snapshot: &ConfigSnapshot, field: &str) -> Option<f64> {
    snapshot.values.get(field).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_checklist::{FieldMapping, GroupDefinition};
    use std::collections::HashMap;

    fn rules_with_fields(fields: HashMap<String, FieldMapping>) -> ChecklistRules {
        ChecklistRules {
            fields,
            groups: HashMap::from([(
                "engine".to_string(),
                GroupDefinition {
                    title: "Двигатель".to_string(),
                    order: 10,
                },
            )]),
            levels: HashMap::from([(
                "conflicts".to_string(),
                LevelDefinition {
                    title: "Конфликты".to_string(),
                    description: None,
                    severity: "critical".to_string(),
                },
            )]),
            checks: vec![],
        }
    }

    fn snap(
        values: HashMap<String, f64>,
        pin_usage: HashMap<String, HashMap<u32, Vec<String>>>,
    ) -> ConfigSnapshot {
        ConfigSnapshot {
            connected: true,
            loaded: true,
            read_only: false,
            loading: false,
            progress: 1.0,
            bytes_loaded: 100,
            bytes_total: 100,
            raw_len: 100,
            values,
            string_values: HashMap::new(),
            field_count: 1,
            last_error: None,
            pin_usage,
            checklist: Default::default(),
        }
    }

    #[test]
    fn extra_ignition_pin_beyond_cylinder_count() {
        let rules = rules_with_fields(HashMap::from([(
            "cylindersCount".to_string(),
            FieldMapping {
                label: "Цилиндры".to_string(),
                hint: None,
                panel: Some("engineChars".to_string()),
                component: Some("cylinderscount".to_string()),
            },
        )]));
        let snapshot = snap(
            HashMap::from([
                ("cylindersCount".to_string(), 4.0),
                ("isIgnitionEnabled".to_string(), 1.0),
                ("ignitionPins5".to_string(), 41.0),
            ]),
            HashMap::new(),
        );
        let (items, issues) = collect_conflict_items(&snapshot, &rules, &HashMap::new());
        let item = items
            .iter()
            .find(|i| i.id == "conflict_cyl_ignition_pin_5")
            .expect("conflict item");
        assert_eq!(item.fields, vec!["cylindersCount", "ignitionPins5"]);
        assert_eq!(item.editors.len(), 1);
        assert_eq!(item.editors[0].field, "ignitionPins5");
        assert_eq!(issues[0].severity, "critical");
    }

    #[test]
    fn pin_duplicate_opens_both_assignments_only() {
        let rules = rules_with_fields(HashMap::new());
        let mut pin_usage = HashMap::new();
        pin_usage.insert(
            "output_pin_e_list".to_string(),
            HashMap::from([(41, vec!["fanPin".to_string(), "vvtPins1".to_string()])]),
        );
        let snapshot = snap(
            HashMap::from([
                ("fanPin".to_string(), 41.0),
                ("vvtPins1".to_string(), 41.0),
            ]),
            pin_usage,
        );
        let (items, _) = collect_conflict_items(&snapshot, &rules, &HashMap::new());
        let item = items
            .iter()
            .find(|i| i.id == "pin_conflict_output_pin_e_list_41")
            .expect("pin conflict");
        assert_eq!(item.editors.len(), 2);
        assert!(item.editors.iter().any(|e| e.field == "fanPin"));
        assert!(item.editors.iter().any(|e| e.field == "vvtPins1"));
    }
}
