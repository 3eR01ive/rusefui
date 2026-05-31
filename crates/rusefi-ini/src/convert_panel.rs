//! Конвертация INI dialog → YAML компонент rusefui.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::menu::{DialogItem, IniMenu, MenuEntry};
use crate::model::{ConfigFieldKind, EnumOption, IniCurveDef, IniTableDef};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelManifest {
    pub ini_source: String,
    pub panel_count: usize,
    pub panels: Vec<PanelManifestEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ini_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ini_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_at_ms: Option<u64>,
    /// Версия генератора panel cache; при смене — пересоздание user cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator_version: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelManifestEntry {
    pub id: String,
    pub file: String,
    pub title: String,
    pub menu_path: String,
}

#[derive(Debug, Serialize)]
pub struct PanelYamlFile {
    pub id: String,
    pub description: String,
    pub source: PanelSource,
    pub children: Vec<YamlNode>,
}

#[derive(Debug, Serialize)]
pub struct PanelSource {
    pub ini_dialog: String,
    pub menu_path: String,
}

#[derive(Debug, Serialize)]
pub struct YamlNode {
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub props: Option<HashMap<String, serde_yaml::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind: Option<YamlBind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<YamlNode>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "navSelectable")]
    pub nav_selectable: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct YamlBind {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, serde_yaml::Value>>,
}

fn config_table_bind(
    x_bins: Option<&str>,
    y_bins: Option<&str>,
    z_bins: &str,
) -> YamlBind {
    let mut params = HashMap::new();
    params.insert(
        "zBins".into(),
        serde_yaml::Value::String(z_bins.to_string()),
    );
    if let Some(x) = x_bins {
        params.insert("xBins".into(), serde_yaml::Value::String(x.to_string()));
    }
    if let Some(y) = y_bins {
        params.insert("yBins".into(), serde_yaml::Value::String(y.to_string()));
    }
    YamlBind {
        source: "config".into(),
        field: None,
        fields: None,
        params: Some(params),
    }
}

fn config_curve_bind(x_bins: &str, y_bins: &str) -> YamlBind {
    let mut params = HashMap::new();
    params.insert(
        "xBins".into(),
        serde_yaml::Value::String(x_bins.to_string()),
    );
    params.insert(
        "yBins".into(),
        serde_yaml::Value::String(y_bins.to_string()),
    );
    YamlBind {
        source: "config".into(),
        field: None,
        fields: None,
        params: Some(params),
    }
}

pub struct ConvertResult {
    pub manifest: PanelManifest,
    pub files: HashMap<String, String>,
}

/// Прочитать INI с диска и сконвертировать menu → YAML панелей.
pub fn convert_ini_path(path: &std::path::Path) -> Result<ConvertResult, crate::IniError> {
    let text = std::fs::read_to_string(path).map_err(|e| crate::IniError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    let menu = crate::parse_menu_section(&text)?;
    let ini = crate::parse_ini(&text)?;
    Ok(convert_menu_panels(
        &menu,
        &ini.config_fields,
        &ini.tables,
        &ini.curves,
        &path.display().to_string(),
    ))
}

pub fn convert_menu_panels(
    menu: &IniMenu,
    config_fields: &HashMap<String, ConfigFieldKind>,
    tables: &HashMap<String, IniTableDef>,
    curves: &HashMap<String, IniCurveDef>,
    ini_source: &str,
) -> ConvertResult {
    let mut seen = HashSet::new();
    let mut manifest_entries = Vec::new();
    let mut files = HashMap::new();

    for entry in &menu.entries {
        if !seen.insert(entry.dialog_id.clone()) {
            continue;
        }
        if !menu.dialogs.contains_key(&entry.dialog_id) {
            continue;
        }

        let file_name = format!("{}.panel.yaml", slugify(&entry.dialog_id));
        let panel = build_panel_yaml(menu, entry, config_fields, tables, curves);
        let yaml = serde_yaml::to_string(&panel).expect("panel yaml");
        files.insert(file_name.clone(), yaml);
        manifest_entries.push(PanelManifestEntry {
            id: entry.dialog_id.clone(),
            file: file_name,
            title: entry.title.clone(),
            menu_path: entry.menu_path.clone(),
        });
    }

    let manifest = PanelManifest {
        ini_source: ini_source.to_string(),
        panel_count: manifest_entries.len(),
        panels: manifest_entries,
        ini_signature: None,
        ini_hash: None,
        generated_at_ms: None,
        generator_version: None,
    };

    ConvertResult { manifest, files }
}

fn build_panel_yaml(
    menu: &IniMenu,
    entry: &MenuEntry,
    config_fields: &HashMap<String, ConfigFieldKind>,
    tables: &HashMap<String, IniTableDef>,
    curves: &HashMap<String, IniCurveDef>,
) -> PanelYamlFile {
    let mut visited = HashSet::new();
    let children = convert_dialog_items(
        menu,
        &entry.dialog_id,
        config_fields,
        tables,
        curves,
        &mut visited,
        0,
    );

    let dialog = menu.dialogs.get(&entry.dialog_id);
    let description = dialog
        .map(|d| {
            if d.title.is_empty() {
                entry.title.clone()
            } else {
                d.title.clone()
            }
        })
        .unwrap_or_else(|| entry.title.clone());

    PanelYamlFile {
        id: format!("{}.panel", entry.dialog_id),
        description,
        source: PanelSource {
            ini_dialog: entry.dialog_id.clone(),
            menu_path: entry.menu_path.clone(),
        },
        children,
    }
}

fn convert_dialog_items(
    menu: &IniMenu,
    dialog_id: &str,
    config_fields: &HashMap<String, ConfigFieldKind>,
    tables: &HashMap<String, IniTableDef>,
    curves: &HashMap<String, IniCurveDef>,
    visited: &mut HashSet<String>,
    depth: usize,
) -> Vec<YamlNode> {
    if depth > 12 {
        return vec![hint_node("… слишком глубокая вложенность панелей")];
    }

    let Some(dialog) = menu.dialogs.get(dialog_id) else {
        return vec![hint_node(&format!("Неизвестная панель: {dialog_id}"))];
    };

    let mut out = Vec::new();
    for item in &dialog.items {
        match item {
            DialogItem::Field(f) => {
                if let Some(name) = &f.field_name {
                    out.push(field_node(config_fields, name, &f.label));
                } else if !f.label.is_empty() {
                    out.push(hint_node(&f.label));
                }
            }
            DialogItem::Text(t) => {
                if !t.is_empty() {
                    out.push(hint_node(t));
                }
            }
            DialogItem::Panel(p) => {
                out.extend(convert_panel_ref(
                    menu,
                    &p.panel_id,
                    config_fields,
                    tables,
                    curves,
                    visited,
                    depth + 1,
                ));
            }
            DialogItem::CommandButton { label, command } => {
                out.push(hint_node(&format!("Кнопка: {label} ({command})")));
            }
        }
    }
    inject_rusefui_extra_fields(dialog_id, config_fields, &mut out);
    out
}

/// Поля есть в INI/config, но не добавлены в TunerStudio dialog (softwareKnockCfg).
fn inject_rusefui_extra_fields(
    dialog_id: &str,
    config_fields: &HashMap<String, ConfigFieldKind>,
    out: &mut Vec<YamlNode>,
) {
    if dialog_id != "softwareKnockCfg" || !config_fields.contains_key("enableKnockScope") {
        return;
    }
    if out.iter().any(|n| n.bind.as_ref().and_then(|b| b.field.as_deref()) == Some("enableKnockScope")) {
        return;
    }
    let node = field_node(
        config_fields,
        "enableKnockScope",
        "Knock scope (FFT on host, rusefui)",
    );
    if let Some(pos) = out
        .iter()
        .position(|n| n.bind.as_ref().and_then(|b| b.field.as_deref()) == Some("enableKnockSpectrogram"))
    {
        out.insert(pos + 1, node);
    } else {
        out.push(node);
    }
}

fn convert_panel_ref(
    menu: &IniMenu,
    panel_id: &str,
    config_fields: &HashMap<String, ConfigFieldKind>,
    tables: &HashMap<String, IniTableDef>,
    curves: &HashMap<String, IniCurveDef>,
    visited: &mut HashSet<String>,
    depth: usize,
) -> Vec<YamlNode> {
    if let Some(table) = tables.get(panel_id) {
        return vec![config_table_node(table)];
    }
    if let Some(curve) = curves.get(panel_id) {
        return vec![config_curve_node(curve)];
    }

    if is_table_panel(panel_id) {
        return vec![hint_node(&format!(
            "Таблица/кривая «{panel_id}» — нет определения в INI"
        ))];
    }

    if menu.dialogs.contains_key(panel_id) {
        if !visited.insert(panel_id.to_string()) {
            return vec![hint_node(&format!("↩ {panel_id} (уже показано выше)"))];
        }
        let nested = menu.dialogs.get(panel_id).unwrap();
        let title = if nested.title.is_empty() {
            panel_id.to_string()
        } else {
            nested.title.clone()
        };
        let children = convert_dialog_items(
            menu, panel_id, config_fields, tables, curves, visited, depth,
        );
        visited.remove(panel_id);
        return vec![section_node(&title, children)];
    }

    vec![hint_node(&format!("Панель «{panel_id}» — нет определения в INI"))]
}

fn is_table_panel(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    lower.ends_with("tbl")
        || lower.ends_with("table")
        || lower.ends_with("curve")
        || lower.ends_with("map")
        || lower.contains("table")
        || lower.contains("curve")
}

fn config_table_node(table: &IniTableDef) -> YamlNode {
    let mut props = HashMap::new();
    props.insert(
        "title".into(),
        serde_yaml::Value::String(table.title.clone()),
    );
    props.insert("variant".into(), serde_yaml::Value::String("table".into()));
    if let Some(x) = &table.x_label {
        props.insert("xLabel".into(), serde_yaml::Value::String(x.clone()));
    }
    if let Some(y) = &table.y_label {
        props.insert("yLabel".into(), serde_yaml::Value::String(y.clone()));
    }
    YamlNode {
        node_type: if table.z_bins == "ignitionTable" {
            "ignition-table".into()
        } else {
            "config-table".into()
        },
        id: Some(slugify(&table.id)),
        props: Some(props),
        bind: Some(config_table_bind(
            table.x_bins.as_deref(),
            table.y_bins.as_deref(),
            &table.z_bins,
        )),
        children: None,
        nav_selectable: None,
    }
}

fn config_curve_node(curve: &IniCurveDef) -> YamlNode {
    let mut props = HashMap::new();
    props.insert(
        "title".into(),
        serde_yaml::Value::String(curve.title.clone()),
    );
    if let Some(x) = &curve.x_label {
        props.insert("xLabel".into(), serde_yaml::Value::String(x.clone()));
    }
    if let Some(y) = &curve.y_label {
        props.insert("yLabel".into(), serde_yaml::Value::String(y.clone()));
    }
    YamlNode {
        node_type: "curve".into(),
        id: Some(slugify(&curve.id)),
        props: Some(props),
        bind: Some(config_curve_bind(&curve.x_bins, &curve.y_bins)),
        children: None,
        nav_selectable: None,
    }
}

fn field_node(
    config_fields: &HashMap<String, ConfigFieldKind>,
    field: &str,
    label: &str,
) -> YamlNode {
    if let Some(ConfigFieldKind::Enum(e)) = config_fields.get(field) {
        if !e.options.is_empty() {
            return enum_node(field, label, &e.options);
        }
    }
    if let Some(ConfigFieldKind::String(s)) = config_fields.get(field) {
        return string_node(field, label, s.length);
    }
    scalar_node(field, label)
}

fn enum_node(field: &str, label: &str, options: &[EnumOption]) -> YamlNode {
    let option_values: Vec<serde_yaml::Value> = options
        .iter()
        .map(|o| {
            serde_yaml::Mapping::from_iter([
                (
                    serde_yaml::Value::String("value".into()),
                    serde_yaml::Value::Number(o.value.into()),
                ),
                (
                    serde_yaml::Value::String("label".into()),
                    serde_yaml::Value::String(o.label.clone()),
                ),
            ])
            .into()
        })
        .collect();

    YamlNode {
        node_type: "enum-field".into(),
        id: Some(slugify(field)),
        props: Some({
            let mut m = HashMap::new();
            m.insert(
                "label".into(),
                serde_yaml::Value::String(label.to_string()),
            );
            m.insert("options".into(), serde_yaml::Value::Sequence(option_values));
            m
        }),
        bind: Some(YamlBind {
            source: "config".into(),
            field: Some(field.to_string()),
            fields: None,
            params: None,
        }),
        children: None,
        nav_selectable: None,
    }
}

fn string_node(field: &str, label: &str, max_length: u32) -> YamlNode {
    YamlNode {
        node_type: "string-field".into(),
        id: Some(slugify(field)),
        props: Some({
            let mut m = HashMap::new();
            m.insert(
                "label".into(),
                serde_yaml::Value::String(label.to_string()),
            );
            m.insert(
                "maxLength".into(),
                serde_yaml::Value::Number(max_length.into()),
            );
            m
        }),
        bind: Some(YamlBind {
            source: "config".into(),
            field: Some(field.to_string()),
            fields: None,
            params: None,
        }),
        children: None,
        nav_selectable: None,
    }
}

fn scalar_node(field: &str, label: &str) -> YamlNode {
    YamlNode {
        node_type: "scalar-field".into(),
        id: Some(slugify(field)),
        props: Some({
            let mut m = HashMap::new();
            m.insert(
                "label".into(),
                serde_yaml::Value::String(label.to_string()),
            );
            m
        }),
        bind: Some(YamlBind {
            source: "config".into(),
            field: Some(field.to_string()),
            fields: None,
            params: None,
        }),
        children: None,
        nav_selectable: None,
    }
}

fn hint_node(text: &str) -> YamlNode {
    YamlNode {
        node_type: "text".into(),
        id: None,
        props: Some({
            let mut m = HashMap::new();
            m.insert("text".into(), serde_yaml::Value::String(text.to_string()));
            m.insert("variant".into(), serde_yaml::Value::String("hint".into()));
            m
        }),
        bind: None,
        children: None,
        nav_selectable: Some(false),
    }
}

fn section_node(title: &str, children: Vec<YamlNode>) -> YamlNode {
    YamlNode {
        node_type: "section".into(),
        id: None,
        props: Some({
            let mut m = HashMap::new();
            m.insert("title".into(), serde_yaml::Value::String(title.to_string()));
            m
        }),
        bind: None,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
        nav_selectable: None,
    }
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::parse_menu_section;
    use crate::parse_ini;

    #[test]
    fn convert_proteus_panels() {
        let text = std::fs::read_to_string(crate::default_test_ini_path()).unwrap();
        let menu = parse_menu_section(&text).unwrap();
        let ini = parse_ini(&text).unwrap();
        let result = convert_menu_panels(
            &menu,
            &ini.config_fields,
            &ini.tables,
            &ini.curves,
            "test.ini",
        );
        assert!(result.manifest.panel_count > 50);
        assert!(result.files.contains_key("enginechars.panel.yaml"));
        let has_table = result.files.values().any(|yaml| yaml.contains("config-table"));
        assert!(has_table, "expected config-table in converted panels");
        let has_curve = result
            .files
            .values()
            .any(|yaml| yaml.contains("\n  type: curve\n") || yaml.contains("- type: curve"));
        assert!(has_curve, "expected curve in converted panels");
        let enginechars = result.files.get("enginechars.panel.yaml").unwrap();
        assert!(
            enginechars.contains("- type: string-field\n    id: vehiclename"),
            "vehicleName should be string-field"
        );
    }
}
