//! Взаимоисключающие настройки: дубли пинов, цилиндры vs выходы, режимы vs пины, триггер vs входы.

use std::collections::HashMap;

use crate::config_checklist::{
    field_labels_for, pin_label_for_value, resolve_editors, resolve_group, ChecklistEditor,
    ChecklistIssue, ChecklistItem, ChecklistRules, LevelDefinition,
};
use crate::config_vars::{logic as var, ConfigVarResolver};
use crate::sources::config::{ConfigFieldInfo, ConfigSnapshot};

const CONFLICT_LEVEL: &str = "conflicts";

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
    let vars = ConfigVarResolver::new(&rules.vars, &rules.conflict_constants, field_info);

    let mut defs = Vec::new();
    defs.extend(pin_pool_conflicts(snapshot, rules, field_info));
    defs.extend(cylinder_pin_conflicts(snapshot, &vars));
    defs.extend(firing_order_cylinder_conflicts(snapshot, &vars));
    defs.extend(ignition_mode_pin_conflicts(snapshot, &vars));
    defs.extend(injection_mode_pin_conflicts(snapshot, &vars));
    defs.extend(trigger_input_conflicts(snapshot, &vars));

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

fn cylinder_pin_conflicts(snapshot: &ConfigSnapshot, vars: &ConfigVarResolver<'_>) -> Vec<ConflictDef> {
    let mut out = Vec::new();
    let cylinders = vars.scalar_usize(snapshot, var::ENGINE_CYLINDER_COUNT);
    if cylinders == 0 {
        return out;
    }

    let cylinder_field = vars
        .field_name(var::ENGINE_CYLINDER_COUNT)
        .unwrap_or_default()
        .to_string();

    if vars.bool_enabled(snapshot, var::IGNITION_ENABLED) {
        let max = vars.max_index(var::IGNITION_OUTPUT_PINS);
        for i in (cylinders + 1)..=max {
            let Some(pin_field) = vars.indexed_field_name(var::IGNITION_OUTPUT_PINS, i) else {
                continue;
            };
            if vars.indexed_pin_assigned(snapshot, var::IGNITION_OUTPUT_PINS, i) {
                out.push(ConflictDef {
                    id: format!("conflict_cyl_ignition_pin_{i}"),
                    group: "engine",
                    label: format!("Зажигание: выход {i}"),
                    message: format!(
                        "Выход зажигания №{i} назначен, но цилиндров только {cylinders}"
                    ),
                    value_display: format!("{cylinders} цил., выход {i} занят"),
                    fields: vec![cylinder_field.clone(), pin_field.clone()],
                    editor_fields: vec![pin_field],
                });
            }
        }
    }

    let skip_modes = &vars.constants().injection.skip_cylinder_pin_check;
    let injection_mode = vars.scalar_u32(snapshot, var::FUEL_INJECTION_MODE);
    if vars.bool_enabled(snapshot, var::FUEL_INJECTION_ENABLED)
        && !skip_modes.contains(&injection_mode)
    {
        let max = vars.max_index(var::FUEL_INJECTOR_PINS);
        for i in (cylinders + 1)..=max {
            let Some(pin_field) = vars.indexed_field_name(var::FUEL_INJECTOR_PINS, i) else {
                continue;
            };
            if vars.indexed_pin_assigned(snapshot, var::FUEL_INJECTOR_PINS, i) {
                out.push(ConflictDef {
                    id: format!("conflict_cyl_injection_pin_{i}"),
                    group: "engine",
                    label: format!("Впрыск: выход {i}"),
                    message: format!(
                        "Выход форсунки №{i} назначен, но цилиндров только {cylinders}"
                    ),
                    value_display: format!("{cylinders} цил., выход {i} занят"),
                    fields: vec![cylinder_field.clone(), pin_field.clone()],
                    editor_fields: vec![pin_field],
                });
            }
        }
    }

    out
}

fn firing_order_cylinder_conflicts(
    snapshot: &ConfigSnapshot,
    vars: &ConfigVarResolver<'_>,
) -> Vec<ConflictDef> {
    let cylinders = vars.scalar_usize(snapshot, var::ENGINE_CYLINDER_COUNT);
    if cylinders == 0 {
        return Vec::new();
    }

    let firing_order = vars.scalar_u32(snapshot, var::ENGINE_FIRING_ORDER);
    let Some(order_label) = vars.option_label(var::ENGINE_FIRING_ORDER, firing_order) else {
        return Vec::new();
    };

    let Some(order_cylinders) = cylinder_count_from_firing_order_label(&order_label) else {
        return Vec::new();
    };

    if order_cylinders == cylinders {
        return Vec::new();
    }

    let cylinder_field = vars
        .field_name(var::ENGINE_CYLINDER_COUNT)
        .unwrap_or_default()
        .to_string();
    let firing_field = vars
        .field_name(var::ENGINE_FIRING_ORDER)
        .unwrap_or_default()
        .to_string();

    vec![ConflictDef {
        id: "conflict_cylinders_firing_order".to_string(),
        group: "engine",
        label: "Цилиндры и порядок зажигания".to_string(),
        message: format!(
            "Порядок зажигания «{order_label}» рассчитан на {order_cylinders} цил., указано {cylinders}"
        ),
        value_display: format!("{cylinders} цил. · {order_label}"),
        fields: vec![cylinder_field.clone(), firing_field.clone()],
        editor_fields: vec![cylinder_field, firing_field],
    }]
}

/// Число цилиндров по подписи enum firing order (максимальный номер в последовательности).
fn cylinder_count_from_firing_order_label(label: &str) -> Option<usize> {
    let trimmed = label.trim();
    if trimmed.eq_ignore_ascii_case("One Cylinder") {
        return Some(1);
    }
    if trimmed.eq_ignore_ascii_case("INVALID") || trimmed.starts_with("fo") {
        return None;
    }

    let mut max = 0usize;
    for part in trimmed.split(|c: char| !c.is_ascii_digit()) {
        if part.is_empty() {
            continue;
        }
        let n: usize = part.parse().ok()?;
        max = max.max(n);
    }

    if max > 0 { Some(max) } else { None }
}

fn ignition_mode_pin_conflicts(
    snapshot: &ConfigSnapshot,
    vars: &ConfigVarResolver<'_>,
) -> Vec<ConflictDef> {
    let mut out = Vec::new();
    if !vars.bool_enabled(snapshot, var::IGNITION_ENABLED) {
        return out;
    }

    let mode = vars.scalar_u32(snapshot, var::IGNITION_MODE);
    let mode_label = vars
        .option_label(var::IGNITION_MODE, mode)
        .unwrap_or_else(|| format!("режим {mode}"));
    let mode_field = vars
        .field_name(var::IGNITION_MODE)
        .unwrap_or_default()
        .to_string();
    let constants = &vars.constants().ignition;

    if mode == constants.single_coil {
        let max = vars.max_index(var::IGNITION_OUTPUT_PINS);
        for i in 2..=max {
            let Some(pin_field) = vars.indexed_field_name(var::IGNITION_OUTPUT_PINS, i) else {
                continue;
            };
            if vars.indexed_pin_assigned(snapshot, var::IGNITION_OUTPUT_PINS, i) {
                out.push(ConflictDef {
                    id: format!("conflict_ignition_single_coil_{i}"),
                    group: "ignition",
                    label: format!("Single Coil + выход {i}"),
                    message: format!(
                        "В режиме «{mode_label}» используется только выход катушки 1"
                    ),
                    value_display: mode_label.clone(),
                    fields: vec![mode_field.clone(), pin_field.clone()],
                    editor_fields: vec![pin_field],
                });
            }
        }
    }

    if mode == constants.individual {
        let cylinders = vars.scalar_usize(snapshot, var::ENGINE_CYLINDER_COUNT);
        let max = vars.max_index(var::IGNITION_OUTPUT_PINS);
        for i in 1..=cylinders.min(max) {
            let Some(pin_field) = vars.indexed_field_name(var::IGNITION_OUTPUT_PINS, i) else {
                continue;
            };
            if !vars.indexed_pin_assigned(snapshot, var::IGNITION_OUTPUT_PINS, i) {
                out.push(ConflictDef {
                    id: format!("conflict_ignition_individual_missing_{i}"),
                    group: "ignition",
                    label: format!("Individual: выход {i}"),
                    message: format!(
                        "В режиме «{mode_label}» нужен отдельный выход на каждый цилиндр"
                    ),
                    value_display: format!("цил. {i} без выхода"),
                    fields: vec![mode_field.clone(), pin_field.clone()],
                    editor_fields: vec![pin_field],
                });
            }
        }
    }

    out
}

fn injection_mode_pin_conflicts(
    snapshot: &ConfigSnapshot,
    vars: &ConfigVarResolver<'_>,
) -> Vec<ConflictDef> {
    let mut out = Vec::new();
    if !vars.bool_enabled(snapshot, var::FUEL_INJECTION_ENABLED) {
        return out;
    }

    let mode = vars.scalar_u32(snapshot, var::FUEL_INJECTION_MODE);
    let mode_label = vars
        .option_label(var::FUEL_INJECTION_MODE, mode)
        .unwrap_or_else(|| format!("режим {mode}"));
    let mode_field = vars
        .field_name(var::FUEL_INJECTION_MODE)
        .unwrap_or_default()
        .to_string();
    let constants = &vars.constants().injection;

    if mode == constants.single_point {
        let max = vars.max_index(var::FUEL_INJECTOR_PINS);
        for i in 2..=max {
            let Some(pin_field) = vars.indexed_field_name(var::FUEL_INJECTOR_PINS, i) else {
                continue;
            };
            if vars.indexed_pin_assigned(snapshot, var::FUEL_INJECTOR_PINS, i) {
                out.push(ConflictDef {
                    id: format!("conflict_injection_single_point_{i}"),
                    group: "fuel",
                    label: format!("Single Point + выход {i}"),
                    message: format!(
                        "В режиме «{mode_label}» используется только выход форсунки 1"
                    ),
                    value_display: mode_label.clone(),
                    fields: vec![mode_field.clone(), pin_field.clone()],
                    editor_fields: vec![pin_field],
                });
            }
        }
    }

    if mode == constants.sequential {
        let cylinders = vars.scalar_usize(snapshot, var::ENGINE_CYLINDER_COUNT);
        let max = vars.max_index(var::FUEL_INJECTOR_PINS);
        for i in 1..=cylinders.min(max) {
            let Some(pin_field) = vars.indexed_field_name(var::FUEL_INJECTOR_PINS, i) else {
                continue;
            };
            if !vars.indexed_pin_assigned(snapshot, var::FUEL_INJECTOR_PINS, i) {
                out.push(ConflictDef {
                    id: format!("conflict_injection_sequential_missing_{i}"),
                    group: "fuel",
                    label: format!("Sequential: выход {i}"),
                    message: format!(
                        "В режиме «{mode_label}» нужен отдельный выход на каждый цилиндр"
                    ),
                    value_display: format!("цил. {i} без выхода"),
                    fields: vec![mode_field.clone(), pin_field.clone()],
                    editor_fields: vec![pin_field],
                });
            }
        }
    }

    out
}

fn trigger_input_conflicts(
    snapshot: &ConfigSnapshot,
    vars: &ConfigVarResolver<'_>,
) -> Vec<ConflictDef> {
    let mut out = Vec::new();
    let trigger_type = vars.scalar_u32(snapshot, var::TRIGGER_TYPE);
    let type_label = vars
        .option_label(var::TRIGGER_TYPE, trigger_type)
        .unwrap_or_else(|| format!("тип {trigger_type}"));
    let type_field = vars
        .field_name(var::TRIGGER_TYPE)
        .unwrap_or_default()
        .to_string();
    let secondary_field = vars
        .field_name(var::TRIGGER_SECONDARY_INPUT)
        .unwrap_or_default()
        .to_string();
    let needs_secondary = &vars.constants().trigger.types_needing_secondary;

    if needs_secondary.contains(&trigger_type) && !vars.pin_assigned(snapshot, var::TRIGGER_SECONDARY_INPUT)
    {
        out.push(ConflictDef {
            id: format!("conflict_trigger_secondary_missing_{trigger_type}"),
            group: "trigger",
            label: "Вторичный вход триггера".to_string(),
            message: format!("Для «{type_label}» нужен вторичный вход (Secondary channel)"),
            value_display: type_label.clone(),
            fields: vec![type_field.clone(), secondary_field.clone()],
            editor_fields: vec![secondary_field.clone()],
        });
    }

    if !needs_secondary.contains(&trigger_type)
        && vars.pin_assigned(snapshot, var::TRIGGER_SECONDARY_INPUT)
    {
        out.push(ConflictDef {
            id: "conflict_trigger_secondary_unused".to_string(),
            group: "trigger",
            label: "Лишний вторичный вход".to_string(),
            message: format!(
                "Для «{type_label}» вторичный вход не используется — сбросьте Secondary channel"
            ),
            value_display: type_label,
            fields: vec![type_field, secondary_field.clone()],
            editor_fields: vec![secondary_field],
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_checklist::{FieldMapping, GroupDefinition};
    use crate::config_vars::{ConflictConstants, VarBinding};
    use std::collections::HashMap;

    fn test_vars() -> HashMap<String, VarBinding> {
        HashMap::from([
            (
                var::ENGINE_CYLINDER_COUNT.to_string(),
                VarBinding {
                    source: "config".to_string(),
                    parameter: Some("cylindersCount".to_string()),
                    prefix: None,
                },
            ),
            (
                var::ENGINE_FIRING_ORDER.to_string(),
                VarBinding {
                    source: "config".to_string(),
                    parameter: Some("firingOrder".to_string()),
                    prefix: None,
                },
            ),
            (
                var::IGNITION_ENABLED.to_string(),
                VarBinding {
                    source: "config".to_string(),
                    parameter: Some("isIgnitionEnabled".to_string()),
                    prefix: None,
                },
            ),
            (
                var::IGNITION_MODE.to_string(),
                VarBinding {
                    source: "config".to_string(),
                    parameter: Some("ignitionMode".to_string()),
                    prefix: None,
                },
            ),
            (
                var::IGNITION_OUTPUT_PINS.to_string(),
                VarBinding {
                    source: "config".to_string(),
                    parameter: None,
                    prefix: Some("ignitionPins".to_string()),
                },
            ),
            (
                var::FUEL_INJECTION_ENABLED.to_string(),
                VarBinding {
                    source: "config".to_string(),
                    parameter: Some("isInjectionEnabled".to_string()),
                    prefix: None,
                },
            ),
            (
                var::FUEL_INJECTION_MODE.to_string(),
                VarBinding {
                    source: "config".to_string(),
                    parameter: Some("injectionMode".to_string()),
                    prefix: None,
                },
            ),
            (
                var::FUEL_INJECTOR_PINS.to_string(),
                VarBinding {
                    source: "config".to_string(),
                    parameter: None,
                    prefix: Some("injectionPins".to_string()),
                },
            ),
            (
                var::TRIGGER_TYPE.to_string(),
                VarBinding {
                    source: "config".to_string(),
                    parameter: Some("trigger_type".to_string()),
                    prefix: None,
                },
            ),
            (
                var::TRIGGER_SECONDARY_INPUT.to_string(),
                VarBinding {
                    source: "config".to_string(),
                    parameter: Some("triggerInputPins2".to_string()),
                    prefix: None,
                },
            ),
        ])
    }

    fn pin_field_info(prefix: &str, count: usize) -> HashMap<String, ConfigFieldInfo> {
        let mut field_info = HashMap::new();
        for i in 1..=count {
            field_info.insert(
                format!("{prefix}{i}"),
                ConfigFieldInfo {
                    name: format!("{prefix}{i}"),
                    ty: "enum".to_string(),
                    units: None,
                    options: None,
                    array_cols: None,
                    array_rows: None,
                    array_length: None,
                    pin_pool: None,
                },
            );
        }
        field_info
    }

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
            vars: test_vars(),
            conflict_constants: ConflictConstants::default(),
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
        let mut field_info = pin_field_info("ignitionPins", 12);
        field_info.insert(
            "cylindersCount".to_string(),
            ConfigFieldInfo {
                name: "cylindersCount".to_string(),
                ty: "scalar".to_string(),
                units: None,
                options: None,
                array_cols: None,
                array_rows: None,
                array_length: None,
                pin_pool: None,
            },
        );
        let snapshot = snap(
            HashMap::from([
                ("cylindersCount".to_string(), 4.0),
                ("isIgnitionEnabled".to_string(), 1.0),
                ("ignitionPins5".to_string(), 41.0),
            ]),
            HashMap::new(),
        );
        let (items, issues) = collect_conflict_items(&snapshot, &rules, &field_info);
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

    #[test]
    fn cylinder_count_from_firing_order_label_parses_sequence() {
        assert_eq!(
            cylinder_count_from_firing_order_label("1-2-3-4-5-6"),
            Some(6)
        );
        assert_eq!(
            cylinder_count_from_firing_order_label("1-8-4-3-6-5-7-2"),
            Some(8)
        );
        assert_eq!(cylinder_count_from_firing_order_label("One Cylinder"), Some(1));
        assert!(cylinder_count_from_firing_order_label("INVALID").is_none());
    }

    #[test]
    fn detects_cylinders_vs_firing_order_mismatch() {
        use crate::sources::config::{ConfigEnumOption, ConfigFieldInfo};

        let rules = rules_with_fields(HashMap::new());
        let mut field_info = HashMap::new();
        field_info.insert(
            "firingOrder".to_string(),
            ConfigFieldInfo {
                name: "firingOrder".to_string(),
                ty: "enum".to_string(),
                units: None,
                options: Some(vec![
                    ConfigEnumOption {
                        value: 9,
                        label: "1-2-3-4-5-6".to_string(),
                    },
                    ConfigEnumOption {
                        value: 1,
                        label: "1-3-4-2".to_string(),
                    },
                ]),
                array_cols: None,
                array_rows: None,
                array_length: None,
                pin_pool: None,
            },
        );

        let snapshot = snap(
            HashMap::from([
                ("cylindersCount".to_string(), 4.0),
                ("firingOrder".to_string(), 9.0),
            ]),
            HashMap::new(),
        );
        let (items, _) = collect_conflict_items(&snapshot, &rules, &field_info);
        let item = items
            .iter()
            .find(|i| i.id == "conflict_cylinders_firing_order")
            .expect("firing order conflict");
        assert_eq!(item.editors.len(), 2);
        assert_eq!(item.editors[0].field, "cylindersCount");
        assert_eq!(item.editors[1].field, "firingOrder");
    }
}
