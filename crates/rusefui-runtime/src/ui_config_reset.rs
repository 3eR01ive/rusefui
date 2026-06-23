//! Сброс локальной копии UI-config вкладки к версии из бандла софта.
//!
//! Старые проекты несут в `config/` собственный снимок layout (наследие версии,
//! где config засевался в проект). Этот снимок перекрывает бандл, поэтому
//! обновления панелей в новой версии софта не доходят до таких проектов.
//!
//! Reset активной вкладки удаляет её локальные копии (сам `tab` + достижимые из
//! него компоненты), чтобы `project_read_ui_config` читал их из бандла текущей
//! версии. Компоненты, используемые другими вкладками проекта, не трогаются.

use std::collections::BTreeSet;
use std::path::Path;

/// Рекурсивно собрать все значения ключа `$component` в YAML-дереве.
fn collect_component_refs(value: &serde_yaml::Value, out: &mut Vec<String>) {
    match value {
        serde_yaml::Value::Mapping(map) => {
            for (k, v) in map {
                if k.as_str() == Some("$component") {
                    if let Some(name) = v.as_str() {
                        out.push(name.to_string());
                    }
                } else {
                    collect_component_refs(v, out);
                }
            }
        }
        serde_yaml::Value::Sequence(seq) => {
            for v in seq {
                collect_component_refs(v, out);
            }
        }
        _ => {}
    }
}

/// Достижимые компоненты из файла (рекурсивно по `$component`). Читаются только
/// файлы, существующие в каталоге проекта; отсутствующие — это бандл, их не
/// удалить, поэтому в множество не попадают.
fn collect_reachable(config_dir: &Path, rel_path: &str, acc: &mut BTreeSet<String>) {
    let Ok(text) = std::fs::read_to_string(config_dir.join(rel_path)) else {
        return;
    };
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&text) else {
        return;
    };
    let mut refs = Vec::new();
    collect_component_refs(&value, &mut refs);
    for name in refs {
        if acc.insert(name.clone()) {
            collect_reachable(config_dir, &format!("components/{name}.yaml"), acc);
        }
    }
}

fn remove_if_file(config_dir: &Path, rel: &str, deleted: &mut Vec<String>) -> Result<(), String> {
    let path = config_dir.join(rel);
    if path.is_file() {
        std::fs::remove_file(&path).map_err(|e| format!("{rel}: {e}"))?;
        deleted.push(rel.to_string());
    }
    Ok(())
}

/// Удалить локальные копии файлов вкладки `tab_id` из `config_dir`, чтобы вкладка
/// читалась из бандла. Не трогает компоненты, используемые другими вкладками.
/// Возвращает список удалённых путей (config-relative).
pub fn reset_tab_ui_config(config_dir: &Path, tab_id: &str) -> Result<Vec<String>, String> {
    if !config_dir.is_dir() {
        return Ok(Vec::new());
    }
    let tab_rel = format!("tabs/{tab_id}.tab.yaml");

    // Компоненты активной вкладки.
    let mut active = BTreeSet::new();
    collect_reachable(config_dir, &tab_rel, &mut active);

    // Компоненты всех остальных вкладок проекта — защищаем от удаления.
    let mut other = BTreeSet::new();
    let self_file = format!("{tab_id}.tab.yaml");
    if let Ok(entries) = std::fs::read_dir(config_dir.join("tabs")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".tab.yaml") || name == self_file {
                continue;
            }
            collect_reachable(config_dir, &format!("tabs/{name}"), &mut other);
        }
    }

    let mut deleted = Vec::new();
    remove_if_file(config_dir, &tab_rel, &mut deleted)?;
    for comp in &active {
        if other.contains(comp) {
            continue;
        }
        remove_if_file(config_dir, &format!("components/{comp}.yaml"), &mut deleted)?;
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, content: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    #[test]
    fn deletes_active_tab_keeps_shared() {
        let tmp = std::env::temp_dir().join(format!("rusefui-test-cfg-{}", std::process::id()));
        let cfg = tmp.join("config");
        let _ = std::fs::remove_dir_all(&tmp);

        write(&cfg, "tabs/monitor.tab.yaml", "tab:\n  id: monitor\nroot:\n  $component: monitor.panel\n");
        write(&cfg, "components/monitor.panel.yaml", "id: monitor.panel\nchildren:\n  - { $component: shared.widget }\n");
        write(&cfg, "tabs/run.tab.yaml", "tab:\n  id: run\nroot:\n  $component: shared.widget\n");
        write(&cfg, "components/shared.widget.yaml", "id: shared.widget\nchildren: []\n");

        let deleted = reset_tab_ui_config(&cfg, "monitor").unwrap();

        assert!(deleted.contains(&"tabs/monitor.tab.yaml".to_string()));
        assert!(deleted.contains(&"components/monitor.panel.yaml".to_string()));
        // shared.widget используется вкладкой run → НЕ удаляется
        assert!(!deleted.iter().any(|d| d.contains("shared.widget")));
        assert!(cfg.join("components/shared.widget.yaml").is_file());
        assert!(cfg.join("tabs/run.tab.yaml").is_file());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn no_config_dir_is_ok() {
        let missing = std::env::temp_dir().join("rusefui-test-nonexistent-xyz");
        assert!(reset_tab_ui_config(&missing, "monitor").unwrap().is_empty());
    }
}
