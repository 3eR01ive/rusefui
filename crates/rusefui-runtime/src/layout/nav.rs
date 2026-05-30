use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::instance::ComponentInstance;
use super::type_catalog::{is_container, resolve_nav_activatable, resolve_nav_selectable};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NavMode {
    Select,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NavRegion {
    Sidebar,
    Main,
    Default,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavPathEntry {
    pub path: String,
    pub selectable: bool,
    pub activatable: bool,
    pub region: NavRegion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavSnapshot {
    pub paths: Vec<String>,
    pub entries: Vec<NavPathEntry>,
    pub mode: NavMode,
    pub selected_path: String,
    pub active_path: String,
    pub sidebar_anchor: String,
}

impl Default for NavSnapshot {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            entries: Vec::new(),
            mode: NavMode::Select,
            selected_path: String::new(),
            active_path: String::new(),
            sidebar_anchor: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NavExtension {
    pub base_path: String,
    pub instance: ComponentInstance,
}

/// FSM навигации по компонентам вкладки.
#[derive(Debug, Default)]
pub struct WorkspaceNav {
    tab_path: String,
    tab_root: Option<ComponentInstance>,
    extensions: Vec<NavExtension>,
    menu_paths: HashMap<String, Vec<String>>,
    snapshot: NavSnapshot,
}

impl WorkspaceNav {    pub fn snapshot(&self) -> &NavSnapshot {
        &self.snapshot
    }

    pub fn init_tab(&mut self, tab_path: String, root: ComponentInstance) {
        self.tab_path = tab_path;
        self.tab_root = Some(root);
        self.extensions.clear();
        self.menu_paths.clear();
        self.snapshot.mode = NavMode::Select;
        self.snapshot.selected_path.clear();
        self.snapshot.active_path.clear();
        self.snapshot.sidebar_anchor.clear();
        self.rebuild();
    }

    pub fn reset(&mut self) {
        self.snapshot.mode = NavMode::Select;
        self.snapshot.selected_path.clear();
        self.snapshot.active_path.clear();
        self.snapshot.sidebar_anchor.clear();
        self.rebuild();
    }

    pub fn set_menu_paths(&mut self, host_path: String, paths: Vec<String>) {
        if paths.is_empty() {
            self.menu_paths.remove(&host_path);
        } else {
            self.menu_paths.insert(host_path, paths);
        }
        self.rebuild();
    }

    pub fn set_extension(&mut self, base_path: String, instance: Option<ComponentInstance>) {
        self.extensions.retain(|e| e.base_path != base_path);
        if let Some(inst) = instance {
            self.extensions.push(NavExtension {
                base_path,
                instance: inst,
            });
        }
        self.rebuild();
    }

    pub fn select(&mut self, path: &str) {
        if self.snapshot.mode == NavMode::Active && self.snapshot.active_path != path {
            self.deactivate();
        }
        if path.contains("/menu/") {
            self.snapshot.sidebar_anchor = path.to_string();
        }
        self.snapshot.selected_path = path.to_string();
    }

    pub fn activate(&mut self, path: &str) {
        if !is_nav_activatable(path, &self.snapshot.entries) {
            return;
        }
        self.snapshot.selected_path = path.to_string();
        self.snapshot.active_path = path.to_string();
        self.snapshot.mode = NavMode::Active;
    }

    pub fn deactivate(&mut self) {
        self.snapshot.mode = NavMode::Select;
        self.snapshot.active_path.clear();
    }

    pub fn move_selection(&mut self, key: NavArrowKey) {
        let paths = &self.snapshot.paths;
        if paths.is_empty() {
            return;
        }
        let cur = &self.snapshot.selected_path;
        let region = nav_region(cur);

        match region {
            NavRegion::Default => {
                let delta = match key {
                    NavArrowKey::Down | NavArrowKey::Right => 1,
                    NavArrowKey::Up | NavArrowKey::Left => -1,
                };
                if let Some(path) = move_linear(paths, cur, delta) {
                    self.select(&path);
                }
            }
            NavRegion::Sidebar => match key {
                NavArrowKey::Down => {
                    if let Some(path) = move_within_region(paths, cur, NavRegion::Sidebar, 1) {
                        self.select(&path);
                    }
                }
                NavArrowKey::Up => {
                    if let Some(path) = move_within_region(paths, cur, NavRegion::Sidebar, -1) {
                        self.select(&path);
                    }
                }
                NavArrowKey::Right => {
                    if cur.contains("/menu/") {
                        self.snapshot.sidebar_anchor = cur.clone();
                    }
                    if let Some(path) = first_in_region(paths, NavRegion::Main) {
                        self.select(&path);
                    }
                }
                NavArrowKey::Left => {
                    if let Some(path) = move_within_region(paths, cur, NavRegion::Sidebar, -1) {
                        self.select(&path);
                    }
                }
            },
            NavRegion::Main => match key {
                NavArrowKey::Down => {
                    if let Some(path) = move_within_region(paths, cur, NavRegion::Main, 1) {
                        self.select(&path);
                    }
                }
                NavArrowKey::Up => {
                    if let Some(path) = move_within_region(paths, cur, NavRegion::Main, -1) {
                        self.select(&path);
                    }
                }
                NavArrowKey::Left => {
                    if let Some(path) = self.sidebar_fallback(paths) {
                        self.select(&path);
                    }
                }
                NavArrowKey::Right => {
                    if let Some(path) = move_within_region(paths, cur, NavRegion::Main, 1) {
                        self.select(&path);
                    }
                }
            },
        }
    }

    pub fn ensure_selected(&mut self) {
        if self.snapshot.paths.is_empty() {
            self.snapshot.selected_path.clear();
            return;
        }
        if !self.snapshot.paths.contains(&self.snapshot.selected_path) {
            self.snapshot.selected_path = self.snapshot.paths[0].clone();
        }
    }

    fn sidebar_fallback(&self, paths: &[String]) -> Option<String> {
        let anchor = &self.snapshot.sidebar_anchor;
        if !anchor.is_empty() && paths.contains(anchor) {
            return Some(anchor.clone());
        }
        paths
            .iter()
            .find(|p| nav_region(p) == NavRegion::Sidebar && p.contains("/menu/"))
            .cloned()
    }

    fn rebuild(&mut self) {
        let Some(root) = self.tab_root.clone() else {
            self.snapshot.paths.clear();
            self.snapshot.entries.clear();
            return;
        };

        let mut paths: Vec<String> = Vec::new();
        let mut entries: Vec<NavPathEntry> = Vec::new();
        collect_nav_paths_from_tree(&root, &self.tab_path, &mut paths, &mut entries);

        let mut menu_hosts_used = std::collections::HashSet::new();
        let menu_map = &self.menu_paths;
        let mut i = 0;
        while i < paths.len() {
            let p = paths[i].clone();
            if let Some(menu) = menu_map.get(&p) {
                if !menu.is_empty() {
                    paths.remove(i);
                    if let Some(idx) = entries.iter().position(|e| e.path == p) {
                        entries.remove(idx);
                    }
                    for mp in menu {
                        paths.push(mp.clone());
                        entries.push(menu_entry(mp));
                    }
                    menu_hosts_used.insert(p);
                    continue;
                }
            }
            i += 1;
        }

        for (host_path, menu) in menu_map {
            if !host_path.starts_with(&format!("{}/", self.tab_path))
                || menu_hosts_used.contains(host_path)
                || menu.is_empty()
            {
                continue;
            }
            for mp in menu {
                if !paths.contains(mp) {
                    paths.push(mp.clone());
                    entries.push(menu_entry(mp));
                }
            }
        }

        let mut sorted_ext = self.extensions.clone();
        sorted_ext.sort_by(|a, b| a.base_path.cmp(&b.base_path));
        for ext in &sorted_ext {
            if ext.base_path.starts_with(&format!("{}/", self.tab_path)) {
                collect_nav_paths_from_tree(
                    &ext.instance,
                    &ext.base_path,
                    &mut paths,
                    &mut entries,
                );
            }
        }

        self.snapshot.paths = paths;
        self.snapshot.entries = entries;
        self.ensure_selected();
    }
}

#[derive(Debug, Clone, Copy)]
pub enum NavArrowKey {
    Up,
    Down,
    Left,
    Right,
}

impl NavArrowKey {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ArrowUp" => Some(Self::Up),
            "ArrowDown" => Some(Self::Down),
            "ArrowLeft" => Some(Self::Left),
            "ArrowRight" => Some(Self::Right),
            _ => None,
        }
    }
}

pub fn nav_region(path: &str) -> NavRegion {
    if path.contains("/menu/") || path.ends_with("/filter") {
        return NavRegion::Sidebar;
    }
    if path.contains("/editor/")
        || path.ends_with("/editor")
        || path.contains("/preview/")
        || path.ends_with("/preview")
    {
        return NavRegion::Main;
    }
    NavRegion::Default
}

pub fn is_filter_nav_path(path: &str) -> bool {
    path.ends_with("/filter")
}

fn is_nav_activatable(path: &str, entries: &[NavPathEntry]) -> bool {
    entries
        .iter()
        .find(|e| e.path == path)
        .map(|e| e.activatable)
        .unwrap_or(true)
}

fn menu_entry(path: &str) -> NavPathEntry {
    NavPathEntry {
        path: path.to_string(),
        selectable: true,
        activatable: false,
        region: nav_region(path),
    }
}

pub fn collect_nav_paths_from_tree(
    instance: &ComponentInstance,
    path: &str,
    paths: &mut Vec<String>,
    entries: &mut Vec<NavPathEntry>,
) {
    if instance.type_.is_empty() {
        return;
    }
    if is_container(instance) {
        for (index, child) in instance.children.iter().enumerate() {
            let child_path = child.child_path(path, index);
            collect_nav_paths_from_tree(child, &child_path, paths, entries);
        }
        return;
    }
    if !resolve_nav_selectable(instance) {
        return;
    }
    paths.push(path.to_string());
    entries.push(NavPathEntry {
        path: path.to_string(),
        selectable: true,
        activatable: resolve_nav_activatable(instance),
        region: nav_region(path),
    });
}

pub fn build_nav_paths(
    root: &ComponentInstance,
    tab_path: &str,
    extensions: &[NavExtension],
    menu_paths: &HashMap<String, Vec<String>>,
) -> (Vec<String>, Vec<NavPathEntry>) {
    let mut nav = WorkspaceNav::default();
    nav.tab_path = tab_path.to_string();
    nav.tab_root = Some(root.clone());
    nav.extensions = extensions.to_vec();
    nav.menu_paths = menu_paths.clone();
    nav.rebuild();
    (nav.snapshot.paths.clone(), nav.snapshot.entries.clone())
}

fn paths_in_region(paths: &[String], region: NavRegion) -> Vec<&String> {
    paths
        .iter()
        .filter(|p| nav_region(p) == region)
        .collect()
}

fn move_linear(paths: &[String], cur: &str, delta: i32) -> Option<String> {
    let cur_idx = paths.iter().position(|p| p == cur);
    let next = match cur_idx {
        None => {
            if delta > 0 {
                0
            } else {
                paths.len().saturating_sub(1)
            }
        }
        Some(i) => {
            let n = i as i32 + delta;
            n.clamp(0, paths.len() as i32 - 1) as usize
        }
    };
    paths.get(next).cloned()
}

fn move_within_region(
    paths: &[String],
    cur: &str,
    region: NavRegion,
    delta: i32,
) -> Option<String> {
    let region_paths: Vec<String> = paths_in_region(paths, region)
        .into_iter()
        .cloned()
        .collect();
    if region_paths.is_empty() {
        return None;
    }
    let cur_idx = region_paths.iter().position(|p| p == cur);
    let next = match cur_idx {
        None => {
            if delta > 0 {
                0
            } else {
                region_paths.len().saturating_sub(1)
            }
        }
        Some(i) => {
            let n = i as i32 + delta;
            n.clamp(0, region_paths.len() as i32 - 1) as usize
        }
    };
    region_paths.get(next).cloned()
}

fn first_in_region(paths: &[String], region: NavRegion) -> Option<String> {
    paths
        .iter()
        .find(|p| nav_region(p) == region)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn leaf(type_id: &str, id: &str) -> ComponentInstance {
        ComponentInstance {
            id: Some(id.into()),
            type_: type_id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn skips_non_selectable_host_and_uses_menu_override() {
        let root = ComponentInstance {
            type_: "composite".into(),
            children: vec![ComponentInstance {
                id: Some("checklist".into()),
                type_: "config-checklist".into(),
                nav_selectable: Some(false),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut menu = HashMap::new();
        menu.insert(
            "tab/checklist/checklist".into(),
            vec![
                "tab/checklist/checklist/menu/a".into(),
                "tab/checklist/checklist/menu/b".into(),
            ],
        );
        let (paths, entries) = build_nav_paths(&root, "tab/checklist", &[], &menu);
        assert!(!paths.contains(&"tab/checklist/checklist".to_string()));
        assert_eq!(paths.len(), 2);
        assert!(entries.iter().all(|e| !e.activatable));
    }

    fn text_hint(id: &str) -> ComponentInstance {
        ComponentInstance {
            id: Some(id.into()),
            type_: "text".into(),
            props: Some(json!({ "variant": "hint" })),
            ..Default::default()
        }
    }

    #[test]
    fn text_hints_not_in_paths() {
        let root = ComponentInstance {
            type_: "composite".into(),
            children: vec![text_hint("hint"), leaf("scalar-field", "rpm")],
            ..Default::default()
        };
        let (paths, _) = build_nav_paths(&root, "tab/run", &[], &HashMap::new());
        assert!(!paths.iter().any(|p| p.contains("hint")));
        assert!(paths.iter().any(|p| p.contains("rpm")));
    }
}
