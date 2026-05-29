//! Checklist конфигурации (авиационный стиль): правила из YAML, проверка на каждом снимке.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

use crate::sources::config::{ConfigFieldInfo, ConfigSnapshot, ConfigSource};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldMapping {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupDefinition {
    pub title: String,
    #[serde(default = "default_group_order")]
    pub order: u32,
}

fn default_group_order() -> u32 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelDefinition {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_severity")]
    pub severity: String,
}

fn default_severity() -> String {
    "warning".to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckDef {
    id: String,
    level: String,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    fields: Vec<String>,
    message: String,
    #[serde(flatten)]
    check: CheckSpec,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CheckSpec {
    FieldPresent { field: String },
    ScalarMin { field: String, min: f64 },
    ScalarMinExclusive { field: String, min: f64 },
    ScalarMax { field: String, max: f64 },
    ScalarMaxExclusive { field: String, max: f64 },
    ScalarNotIn { field: String, values: Vec<f64> },
    ScalarIn { field: String, values: Vec<f64> },
    StringNonEmpty { field: String },
    ArrayAllMinExclusive { field: String, min: f64 },
    ArrayNotAllZero { field: String },
    PinsAssigned {
        prefix: String,
        count_field: String,
        min: f64,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChecklistRules {
    #[serde(default)]
    pub fields: HashMap<String, FieldMapping>,
    #[serde(default)]
    pub groups: HashMap<String, GroupDefinition>,
    pub levels: HashMap<String, LevelDefinition>,
    pub checks: Vec<CheckDef>,
}

impl ChecklistRules {
    pub fn parse_yaml(yaml: &str) -> Result<Self, String> {
        serde_yaml::from_str(yaml).map_err(|e| format!("checklist.yaml: {e}"))
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChecklistEditor {
    pub panel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    pub field: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChecklistItem {
    pub id: String,
    pub level: String,
    pub group: String,
    pub group_title: String,
    pub group_order: u32,
    pub label: String,
    pub ok: bool,
    pub message: String,
    pub value_display: String,
    pub fields: Vec<String>,
    pub field_labels: Vec<String>,
    pub editor: ChecklistEditor,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChecklistIssue {
    pub id: String,
    pub level: String,
    pub level_title: String,
    pub severity: String,
    pub message: String,
    pub fields: Vec<String>,
    pub field_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChecklistLevelStatus {
    pub id: String,
    pub title: String,
    pub severity: String,
    pub ok: bool,
    pub issue_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChecklistSnapshot {
    pub rules_loaded: bool,
    pub evaluated: bool,
    pub ok: bool,
    pub items: Vec<ChecklistItem>,
    pub issues: Vec<ChecklistIssue>,
    pub levels: Vec<ChecklistLevelStatus>,
}

pub fn evaluate_checklist(
    snapshot: &ConfigSnapshot,
    rules: &ChecklistRules,
    config: &ConfigSource,
) -> ChecklistSnapshot {
    if !snapshot.loaded {
        return ChecklistSnapshot {
            rules_loaded: true,
            evaluated: false,
            ok: true,
            items: Vec::new(),
            issues: Vec::new(),
            levels: level_statuses_from_issues(rules, &[]),
        };
    }

    let field_info: HashMap<String, ConfigFieldInfo> = config
        .list_fields()
        .into_iter()
        .map(|f| (f.name.clone(), f))
        .collect();

    let mut items = Vec::new();
    let mut issues = Vec::new();

    for check in &rules.checks {
        let ok = check_passes(snapshot, config, &check.check);
        let fields = related_fields(check);
        let field_labels = field_labels_for(rules, &fields);
        let label = check
            .label
            .clone()
            .unwrap_or_else(|| field_labels.first().cloned().unwrap_or_else(|| check.id.clone()));
        let (group, group_title, group_order) = resolve_group(rules, check.group.as_deref());
        let primary = primary_field(check);
        let value_display = format_value_display(
            rules,
            snapshot,
            config,
            &field_info,
            &check.check,
            &primary,
        );
        let editor = resolve_editor(rules, &primary);

        items.push(ChecklistItem {
            id: check.id.clone(),
            level: check.level.clone(),
            group: group.clone(),
            group_title,
            group_order,
            label: label.clone(),
            ok,
            message: check.message.clone(),
            value_display,
            fields: fields.clone(),
            field_labels: field_labels.clone(),
            editor,
        });

        if !ok {
            let level_def = rules
                .levels
                .get(&check.level)
                .cloned()
                .unwrap_or_else(|| LevelDefinition {
                    title: check.level.clone(),
                    description: None,
                    severity: "warning".to_string(),
                });
            issues.push(ChecklistIssue {
                id: check.id.clone(),
                level: check.level.clone(),
                level_title: level_def.title.clone(),
                severity: level_def.severity.clone(),
                message: check.message.clone(),
                fields,
                field_labels,
            });
        }
    }

    ChecklistSnapshot {
        rules_loaded: true,
        evaluated: true,
        ok: issues.is_empty(),
        items,
        issues: issues.clone(),
        levels: level_statuses_from_issues(rules, &issues),
    }
}

fn resolve_group(rules: &ChecklistRules, group: Option<&str>) -> (String, String, u32) {
    let id = group.unwrap_or("other").to_string();
    if let Some(def) = rules.groups.get(&id) {
        return (id, def.title.clone(), def.order);
    }
    if id == "other" {
        return (id, "Прочее".to_string(), 999);
    }
    (id.clone(), id, 100)
}

fn resolve_editor(rules: &ChecklistRules, field: &str) -> ChecklistEditor {
    let mapping = rules.fields.get(field);
    ChecklistEditor {
        panel: mapping.and_then(|m| m.panel.clone()).unwrap_or_default(),
        component: mapping.and_then(|m| m.component.clone()),
        field: field.to_string(),
    }
}

fn level_statuses_from_issues(
    rules: &ChecklistRules,
    issues: &[ChecklistIssue],
) -> Vec<ChecklistLevelStatus> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for issue in issues {
        *counts.entry(issue.level.as_str()).or_default() += 1;
    }

    let mut levels: Vec<ChecklistLevelStatus> = rules
        .levels
        .iter()
        .map(|(id, def)| {
            let issue_count = counts.get(id.as_str()).copied().unwrap_or(0);
            ChecklistLevelStatus {
                id: id.clone(),
                title: def.title.clone(),
                severity: def.severity.clone(),
                ok: issue_count == 0,
                issue_count,
                description: def.description.clone(),
            }
        })
        .collect();
    levels.sort_by(|a, b| a.id.cmp(&b.id));
    levels
}

fn field_labels_for(rules: &ChecklistRules, fields: &[String]) -> Vec<String> {
    fields
        .iter()
        .map(|name| {
            rules
                .fields
                .get(name)
                .map(|m| m.label.clone())
                .unwrap_or_else(|| name.clone())
        })
        .collect()
}

fn primary_field(check: &CheckDef) -> String {
    if !check.fields.is_empty() {
        return check.fields[0].clone();
    }
    match &check.check {
        CheckSpec::FieldPresent { field }
        | CheckSpec::ScalarMin { field, .. }
        | CheckSpec::ScalarMinExclusive { field, .. }
        | CheckSpec::ScalarMax { field, .. }
        | CheckSpec::ScalarMaxExclusive { field, .. }
        | CheckSpec::ScalarNotIn { field, .. }
        | CheckSpec::ScalarIn { field, .. }
        | CheckSpec::StringNonEmpty { field }
        | CheckSpec::ArrayAllMinExclusive { field, .. }
        | CheckSpec::ArrayNotAllZero { field } => field.clone(),
        CheckSpec::PinsAssigned { prefix, .. } => format!("{prefix}1"),
    }
}

fn related_fields(check: &CheckDef) -> Vec<String> {
    if !check.fields.is_empty() {
        return check.fields.clone();
    }
    match &check.check {
        CheckSpec::FieldPresent { field }
        | CheckSpec::ScalarMin { field, .. }
        | CheckSpec::ScalarMinExclusive { field, .. }
        | CheckSpec::ScalarMax { field, .. }
        | CheckSpec::ScalarMaxExclusive { field, .. }
        | CheckSpec::ScalarNotIn { field, .. }
        | CheckSpec::ScalarIn { field, .. }
        | CheckSpec::StringNonEmpty { field }
        | CheckSpec::ArrayAllMinExclusive { field, .. }
        | CheckSpec::ArrayNotAllZero { field } => vec![field.clone()],
        CheckSpec::PinsAssigned {
            prefix,
            count_field,
            ..
        } => vec![count_field.clone(), format!("{prefix}1")],
    }
}

fn format_value_display(
    rules: &ChecklistRules,
    snapshot: &ConfigSnapshot,
    _config: &ConfigSource,
    field_info: &HashMap<String, ConfigFieldInfo>,
    check: &CheckSpec,
    primary: &str,
) -> String {
    match check {
        CheckSpec::ArrayAllMinExclusive { field, .. } | CheckSpec::ArrayNotAllZero { field } => {
            rules
                .fields
                .get(field)
                .map(|m| m.label.clone())
                .unwrap_or_else(|| field.clone())
        }
        CheckSpec::PinsAssigned {
            prefix,
            count_field,
            min,
        } => {
            let count = scalar_value(snapshot, count_field)
                .map(|v| v.round().max(0.0) as usize)
                .unwrap_or(0);
            let assigned = (1..=count)
                .filter(|i| {
                    scalar_value(snapshot, &format!("{prefix}{i}"))
                        .map(|v| v > *min)
                        .unwrap_or(false)
                })
                .count();
            format!("{assigned}/{count}")
        }
        _ => format_scalar_display(rules, snapshot, field_info, primary),
    }
}

fn format_scalar_display(
    rules: &ChecklistRules,
    snapshot: &ConfigSnapshot,
    field_info: &HashMap<String, ConfigFieldInfo>,
    field: &str,
) -> String {
    let Some(v) = scalar_value(snapshot, field) else {
        return "—".to_string();
    };

    if let Some(info) = field_info.get(field) {
        if info.ty == "array" {
            return rules
                .fields
                .get(field)
                .map(|m| m.label.clone())
                .unwrap_or_else(|| field.to_string());
        }
        if let Some(options) = &info.options {
            for opt in options {
                if approx_eq(opt.value as f64, v) {
                    return opt.label.clone();
                }
            }
        }
        if let Some(units) = &info.units {
            if !units.is_empty() {
                return format!("{} {}", format_number(v), units);
            }
        }
    }

    format_number(v)
}

fn format_number(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        return format!("{}", v.round() as i64);
    }
    let s = format!("{:.4}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn check_passes(snapshot: &ConfigSnapshot, config: &ConfigSource, check: &CheckSpec) -> bool {
    match check {
        CheckSpec::FieldPresent { field } => snapshot.values.contains_key(field),
        CheckSpec::ScalarMin { field, min } => scalar_value(snapshot, field)
            .map(|v| v >= *min)
            .unwrap_or(false),
        CheckSpec::ScalarMinExclusive { field, min } => scalar_value(snapshot, field)
            .map(|v| v > *min)
            .unwrap_or(false),
        CheckSpec::ScalarMax { field, max } => scalar_value(snapshot, field)
            .map(|v| v <= *max)
            .unwrap_or(false),
        CheckSpec::ScalarMaxExclusive { field, max } => scalar_value(snapshot, field)
            .map(|v| v < *max)
            .unwrap_or(false),
        CheckSpec::ScalarNotIn { field, values } => scalar_value(snapshot, field)
            .map(|v| !values.iter().any(|x| approx_eq(*x, v)))
            .unwrap_or(false),
        CheckSpec::ScalarIn { field, values } => scalar_value(snapshot, field)
            .map(|v| values.iter().any(|x| approx_eq(*x, v)))
            .unwrap_or(false),
        CheckSpec::StringNonEmpty { field } => snapshot
            .string_values
            .get(field)
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        CheckSpec::ArrayAllMinExclusive { field, min } => {
            array_all_min_exclusive(config, field, *min)
        }
        CheckSpec::ArrayNotAllZero { field } => array_not_all_zero(config, field),
        CheckSpec::PinsAssigned {
            prefix,
            count_field,
            min,
        } => pins_assigned(snapshot, prefix, count_field, *min),
    }
}

fn array_not_all_zero(config: &ConfigSource, field: &str) -> bool {
    match config.get_array(field) {
        Ok(values) if !values.is_empty() => values.iter().any(|v| v.abs() > 1e-9),
        _ => false,
    }
}

fn array_all_min_exclusive(config: &ConfigSource, field: &str, min: f64) -> bool {
    match config.get_array(field) {
        Ok(values) if !values.is_empty() => values.iter().all(|v| *v > min),
        _ => false,
    }
}

fn pins_assigned(snapshot: &ConfigSnapshot, prefix: &str, count_field: &str, min: f64) -> bool {
    let count = scalar_value(snapshot, count_field)
        .map(|v| v.round().max(0.0) as usize)
        .unwrap_or(0);
    if count == 0 {
        return false;
    }
    (1..=count).all(|i| {
        let name = format!("{prefix}{i}");
        scalar_value(snapshot, &name)
            .map(|v| v > min)
            .unwrap_or(false)
    })
}

fn scalar_value(snapshot: &ConfigSnapshot, field: &str) -> Option<f64> {
    snapshot.values.get(field).copied()
}

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::output_channels::IniContext;

    fn sample_rules() -> ChecklistRules {
        ChecklistRules {
            fields: HashMap::from([(
                "cylindersCount".to_string(),
                FieldMapping {
                    label: "Цилиндры".to_string(),
                    hint: None,
                    panel: Some("engineChars".to_string()),
                    component: Some("cylinderscount".to_string()),
                },
            )]),
            groups: HashMap::from([(
                "engine".to_string(),
                GroupDefinition {
                    title: "Двигатель".to_string(),
                    order: 10,
                },
            )]),
            levels: HashMap::from([(
                "startup_minimum".to_string(),
                LevelDefinition {
                    title: "Минимум".to_string(),
                    description: None,
                    severity: "error".to_string(),
                },
            )]),
            checks: vec![CheckDef {
                id: "cyl".to_string(),
                level: "startup_minimum".to_string(),
                group: Some("engine".to_string()),
                label: None,
                fields: vec!["cylindersCount".to_string()],
                message: "need cylinders".to_string(),
                check: CheckSpec::ScalarMin {
                    field: "cylindersCount".to_string(),
                    min: 1.0,
                },
            }],
        }
    }

    fn sample_snapshot(values: HashMap<String, f64>) -> ConfigSnapshot {
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
            pin_usage: HashMap::new(),
            checklist: ChecklistSnapshot::default(),
        }
    }

    #[test]
    fn fails_when_scalar_below_min() {
        let rules = sample_rules();
        let snap = sample_snapshot(HashMap::from([("cylindersCount".to_string(), 0.0)]));
        let config = ConfigSource::new(IniContext::disconnected());
        let result = evaluate_checklist(&snap, &rules, &config);
        assert!(!result.ok);
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.items.len(), 1);
        assert!(!result.items[0].ok);
        assert_eq!(result.items[0].group, "engine");
        assert_eq!(result.items[0].editor.panel, "engineChars");
    }
}
