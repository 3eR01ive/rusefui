//! Парсинг секции `[Menu]`: меню, subMenu и определения dialog.

use std::collections::HashMap;

use crate::error::IniError;
use crate::parse::split_ini_args;

#[derive(Debug, Clone)]
pub struct IniMenu {
    pub entries: Vec<MenuEntry>,
    pub dialogs: HashMap<String, IniDialog>,
}

#[derive(Debug, Clone)]
pub struct MenuEntry {
    pub dialog_id: String,
    pub title: String,
    pub menu_path: String,
}

#[derive(Debug, Clone)]
pub struct IniDialog {
    pub id: String,
    pub title: String,
    pub items: Vec<DialogItem>,
}

#[derive(Debug, Clone)]
pub enum DialogItem {
    Field(DialogField),
    Panel(DialogPanel),
    CommandButton { label: String, command: String },
    Text(String),
}

#[derive(Debug, Clone)]
pub struct DialogField {
    pub label: String,
    pub field_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DialogPanel {
    pub panel_id: String,
}

pub fn parse_menu_section(text: &str) -> Result<IniMenu, IniError> {
    let menu_lines = extract_section_lines(text, "Menu");
    let entries = parse_menu_entries(&menu_lines);
    let dialogs = parse_all_dialogs(text)?;
    Ok(IniMenu { entries, dialogs })
}

fn extract_section_lines(text: &str, section: &str) -> Vec<(usize, String)> {
    let mut in_section = false;
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let trimmed = raw.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let name = &trimmed[1..trimmed.len() - 1];
            in_section = name == section;
            continue;
        }
        if in_section {
            if raw.trim().is_empty() || raw.trim_start().starts_with(';') {
                continue;
            }
            out.push((i + 1, raw.to_string()));
        }
    }
    out
}

fn parse_all_dialogs(text: &str) -> Result<HashMap<String, IniDialog>, IniError> {
    let mut dialogs = HashMap::new();
    let mut current: Option<(IniDialog, usize)> = None;

    for (line_no, raw) in text.lines().enumerate() {
        let trimmed = raw.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if let Some((d, _)) = current.take() {
                dialogs.insert(d.id.clone(), d);
            }
            continue;
        }

        if trimmed.starts_with("dialog ") || trimmed.starts_with("dialog=") {
            if let Some((d, _)) = current.take() {
                dialogs.insert(d.id.clone(), d);
            }
            let header_indent = leading_indent(raw);
            let dialog = parse_dialog_header(trimmed).map_err(|e| {
                IniError::Parse(format!("line {}: {e}", line_no + 1))
            })?;
            current = Some((dialog, header_indent));
            continue;
        }

        let Some((dialog, header_indent)) = current.as_mut() else {
            continue;
        };

        if trimmed.is_empty() || trimmed.starts_with(';') {
            continue;
        }

        let indent = leading_indent(raw);
        if indent <= *header_indent {
            continue;
        }

        if let Some(item) = parse_dialog_item(trimmed) {
            dialog.items.push(item);
        }
    }

    if let Some((d, _)) = current {
        dialogs.insert(d.id.clone(), d);
    }

    Ok(dialogs)
}

fn leading_indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

pub(crate) fn ini_rhs<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let trimmed = line.trim();
    let prefix = format!("{key}=");
    let prefix_sp = format!("{key} =");
    if let Some(rest) = trimmed.strip_prefix(&prefix_sp) {
        return Some(rest.trim());
    }
    if let Some(rest) = trimmed.strip_prefix(&prefix) {
        return Some(rest.trim());
    }
    if trimmed.starts_with(key) {
        let rest = trimmed[key.len()..].trim().trim_start_matches('=').trim();
        if !rest.is_empty() {
            return Some(rest);
        }
    }
    None
}

fn parse_dialog_header(line: &str) -> Result<IniDialog, String> {
    let rest = ini_rhs(line, "dialog").ok_or("not dialog")?;
    let parts = split_ini_args(rest)?;
    let id = parts.first().ok_or("dialog without id")?.trim().to_string();
    let title = parts.get(1).cloned().unwrap_or_default();
    Ok(IniDialog {
        id,
        title,
        items: Vec::new(),
    })
}

fn parse_dialog_item(line: &str) -> Option<DialogItem> {
    if let Some(rest) = ini_rhs(line, "field") {
        return Some(parse_field_item(rest));
    }
    if let Some(rest) = ini_rhs(line, "panel") {
        let parts = split_ini_args(rest).ok()?;
        let panel_id = parts
            .first()?
            .trim()
            .trim_start_matches('=')
            .trim()
            .to_string();
        return Some(DialogItem::Panel(DialogPanel { panel_id }));
    }
    if let Some(rest) = ini_rhs(line, "commandButton") {
        let parts = split_ini_args(rest).ok()?;
        let label = parts.first()?.clone();
        let command = parts.get(1).cloned().unwrap_or_default();
        return Some(DialogItem::CommandButton { label, command });
    }
    None
}

fn parse_field_item(rest: &str) -> DialogItem {
    let parts = split_ini_args(rest).unwrap_or_default();
    if parts.is_empty() || parts[0].is_empty() {
        return DialogItem::Text(String::new());
    }

    let first = &parts[0];

    if !first.starts_with('"')
        && !first.starts_with('#')
        && !first.starts_with('!')
        && parts.len() == 1
    {
        return DialogItem::Field(DialogField {
            label: first.clone(),
            field_name: Some(first.clone()),
        });
    }

    if !first.starts_with('"')
        && !first.starts_with('#')
        && !first.starts_with('!')
        && parts.len() >= 2
        && is_field_token(&parts[1])
    {
        return DialogItem::Field(DialogField {
            label: first.clone(),
            field_name: Some(parts[1].clone()),
        });
    }

    if parts.len() >= 2 && is_field_token(&parts[1]) {
        return DialogItem::Field(DialogField {
            label: first.clone(),
            field_name: Some(parts[1].clone()),
        });
    }

    text_or_field_label(first)
}

fn is_field_token(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn text_or_field_label(label: &str) -> DialogItem {
    let text = label
        .trim_start_matches('#')
        .trim_start_matches('!')
        .trim()
        .to_string();
    DialogItem::Text(text)
}

fn parse_menu_entries(lines: &[(usize, String)]) -> Vec<MenuEntry> {
    let mut entries = Vec::new();
    let mut path: Vec<String> = Vec::new();

    for (_, raw) in lines {
        let trimmed = raw.trim();
        if trimmed.starts_with("menu ") || trimmed.starts_with("menu=") {
            let title = parse_menu_title(trimmed, "menu");
            path.clear();
            if !title.is_empty() {
                path.push(title);
            }
            continue;
        }
        if trimmed.starts_with("groupMenu ") || trimmed.starts_with("groupMenu=") {
            let title = parse_menu_title(trimmed, "groupMenu");
            if path.is_empty() {
                path.push("Menu".into());
            }
            if path.len() > 1 {
                path.truncate(1);
            }
            if !title.is_empty() {
                if path.len() == 1 {
                    path.push(title);
                } else {
                    path[1] = title;
                }
            }
            continue;
        }

        let is_submenu = ini_rhs(trimmed, "subMenu").is_some();
        let rest = if let Some(r) = ini_rhs(trimmed, "subMenu") {
            r
        } else if let Some(r) = ini_rhs(trimmed, "groupChildMenu") {
            r
        } else {
            continue;
        };

        let parts = split_ini_args(rest).unwrap_or_default();
        let dialog_id = parts.first().cloned().unwrap_or_default();
        if dialog_id == "std_separator" || dialog_id.is_empty() {
            continue;
        }
        let title = parts.get(1).cloned().unwrap_or_else(|| dialog_id.clone());

        if is_submenu && path.len() > 2 {
            path.truncate(1);
        }

        let menu_path = if path.is_empty() {
            title.clone()
        } else {
            format!("{} › {}", path.join(" › "), title)
        };

        entries.push(MenuEntry {
            dialog_id,
            title,
            menu_path,
        });
    }

    entries
}

fn parse_menu_title(line: &str, prefix: &str) -> String {
    let rest = ini_rhs(line, prefix).unwrap_or("");
    let parts = split_ini_args(rest).unwrap_or_default();
    let title = parts.first().cloned().unwrap_or_default();
    title.strip_prefix('&').unwrap_or(&title).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_panel_with_tabs_before_equals() {
        let text = r#"
[Menu]
menu = "Fuel", 0

[Menu]
	subMenu = veTableDialog, "VE"

	dialog = veTableDialog, "", border
		panel		= veTableTbl, Center
"#;
        let menu = parse_menu_section(text).unwrap();
        let dialog = menu.dialogs.get("veTableDialog").unwrap();
        let panel = dialog.items.iter().find_map(|item| match item {
            DialogItem::Panel(p) => Some(p.panel_id.clone()),
            _ => None,
        });
        assert_eq!(panel.as_deref(), Some("veTableTbl"));
    }
}
