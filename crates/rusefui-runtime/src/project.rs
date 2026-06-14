//! Файл проекта rusefui (`project.json`): снимок config ECU, логи, UI.
//! Хранение — git-репозиторий в `~/.rusefui/projects/{name}/`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusefi_ini::{config_field_ini_page, decode_config_fields_pages, DEFAULT_INI_PAGE};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config_diff::encode_scalar_into_page;
use crate::ini::resolve_ini_for_signature;
use crate::project_repo::ProjectGitRepo;
use crate::project_timeline::{
    channel, clip_with_default_end, validate_channel, ProjectTimeline, ProjectTimelineClip,
    ProjectTimelineRecordRef,
};
use crate::session::EcuSession;
use crate::sources::config::{build_project_ecu_config, pages_from_project_ecu};
use crate::ui_persist::{self, ProjectUi};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RusefuiProject {
    #[serde(default)]
    pub format_version: u32,
    pub name: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ini: Option<ProjectIniRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ecu_config: Option<ProjectEcuConfig>,
    #[serde(default)]
    pub logs: Vec<ProjectLogRef>,
    #[serde(default)]
    pub timeline: ProjectTimeline,
    #[serde(default)]
    pub scripts: Vec<ProjectScript>,
    #[serde(default)]
    pub ui: ProjectUi,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectIniRef {
    pub path: Option<String>,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEcuConfig {
    pub captured_at_ms: u64,
    pub page_size: u32,
    pub raw_page0_base64: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub config_pages_base64: HashMap<String, String>,
    pub values: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectScript {
    pub id: String,
    pub name: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLogRef {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub added_at_ms: u64,
    pub kind: String,
}

impl RusefuiProject {
    pub fn new_named(name: impl Into<String>) -> Self {
        let t = now_ms();
        let mut ui = ProjectUi::default();
        ui_persist::init_document_ui(&mut ui);
        Self {
            format_version: 2,
            name: name.into(),
            created_at_ms: t,
            updated_at_ms: t,
            ini: None,
            ecu_config: None,
            logs: Vec::new(),
            timeline: ProjectTimeline::default(),
            scripts: Vec::new(),
            ui,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at_ms = now_ms();
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    /// Project directory path (None when no project is open).
    pub path: Option<String>,
    pub name: String,
    pub dirty: bool,
    pub log_count: usize,
    pub timeline_clip_count: usize,
    pub has_ecu_config: bool,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct ProjectInner {
    repo: Option<ProjectGitRepo>,
    doc: RusefuiProject,
    dirty: bool,
}

impl ProjectInner {
    fn scratch() -> Self {
        Self {
            repo: None,
            doc: RusefuiProject::new_named("Новый проект"),
            dirty: false,
        }
    }
}

// ---------------------------------------------------------------------------
// ProjectStore
// ---------------------------------------------------------------------------

pub struct ProjectStore {
    inner: Mutex<ProjectInner>,
}

impl Default for ProjectStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectStore {
    pub fn new() -> Self {
        Self { inner: Mutex::new(ProjectInner::scratch()) }
    }

    pub fn info(&self) -> ProjectInfo {
        let inner = self.inner.lock().unwrap();
        ProjectInfo {
            path: inner.repo.as_ref().map(|r| r.dir().display().to_string()),
            name: inner.doc.name.clone(),
            dirty: inner.dirty,
            log_count: inner.doc.logs.len(),
            timeline_clip_count: inner.doc.timeline.clips.len(),
            has_ecu_config: inner.doc.ecu_config.is_some(),
        }
    }

    pub fn document(&self) -> RusefuiProject {
        self.inner.lock().unwrap().doc.clone()
    }

    pub fn project_dir(&self) -> Option<PathBuf> {
        self.inner.lock().unwrap().repo.as_ref().map(|r| r.dir().to_path_buf())
    }

    pub fn ui_get(&self, key: &str) -> Result<Value, String> {
        let inner = self.inner.lock().unwrap();
        ui_persist::get(&inner.doc.ui, key)
    }

    pub fn ui_set(&self, key: &str, value: Value) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        let existing = inner.doc.ui.sections.get(key).cloned();
        ui_persist::set(&mut inner.doc.ui, key, value)?;
        let changed = existing.as_ref() != inner.doc.ui.sections.get(key);
        if changed {
            inner.doc.touch();
            inner.dirty = true;
        }
        Ok(())
    }

    pub fn ui_persist_keys(&self) -> Vec<&'static str> {
        ui_persist::persist_keys()
    }

    /// Reset to scratch (no project open — Gate screen).
    pub fn new_document(&self, name: String) {
        let mut inner = self.inner.lock().unwrap();
        *inner = ProjectInner::scratch();
        inner.doc.name = name;
    }

    pub fn set_name(&self, name: String) {
        let mut inner = self.inner.lock().unwrap();
        inner.doc.name = name;
        inner.doc.touch();
        inner.dirty = true;
    }

    // -----------------------------------------------------------------------
    // Git operations
    // -----------------------------------------------------------------------

    /// Create a new project in `~/.rusefui/projects/`. Returns the project directory path.
    pub fn create_project(&self, name: &str) -> Result<PathBuf, String> {
        let (repo, doc) = ProjectGitRepo::create(name)?;
        let dir = repo.dir().to_path_buf();
        let mut inner = self.inner.lock().unwrap();
        *inner = ProjectInner { repo: Some(repo), doc, dirty: false };
        Ok(dir)
    }

    /// Open a project directory or migrate a legacy `.rusefui` file. Returns dir path.
    pub fn open_project_path(&self, path: &Path) -> Result<PathBuf, String> {
        let is_legacy = path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("rusefui") || e.eq_ignore_ascii_case("json"))
            .unwrap_or(false);

        let (repo, doc) = if is_legacy {
            // Legacy file → migrate to git project
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("Не удалось прочитать файл: {e}"))?;
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("project")
                .to_string();
            println!("[project] migrate legacy: {}", path.display());
            ProjectGitRepo::import_legacy(&text, &name)?
        } else {
            // Git project directory
            ProjectGitRepo::open(path)?
        };

        let dir = repo.dir().to_path_buf();
        let mut inner = self.inner.lock().unwrap();
        *inner = ProjectInner { repo: Some(repo), doc, dirty: false };
        Ok(dir)
    }

    /// Commit current in-memory doc. Returns commit id hex string.
    pub fn commit(&self, message: Option<&str>) -> Result<String, String> {
        let (doc_clone, repo_dir) = {
            let inner = self.inner.lock().unwrap();
            let repo = inner.repo.as_ref().ok_or("Проект не открыт")?;
            let mut doc = inner.doc.clone();
            doc.touch();
            (doc, repo.dir().to_path_buf())
        };

        let repo = ProjectGitRepo { dir: repo_dir };
        let commit_id = repo.write_doc_and_commit(&doc_clone, message.unwrap_or("Сохранение"))?;

        let mut inner = self.inner.lock().unwrap();
        inner.doc.updated_at_ms = doc_clone.updated_at_ms;
        inner.dirty = false;
        Ok(commit_id)
    }

    pub fn history(&self) -> Result<Vec<crate::project_repo::CommitSummary>, String> {
        let inner = self.inner.lock().unwrap();
        let repo = inner.repo.as_ref().ok_or("Проект не открыт")?;
        repo.history()
    }

    pub fn diff(&self, from_id: &str, to_id: Option<&str>) -> Result<String, String> {
        let inner = self.inner.lock().unwrap();
        let repo = inner.repo.as_ref().ok_or("Проект не открыт")?;
        match to_id {
            Some(to) => repo.diff_commits(from_id, to),
            None => repo.diff_working(from_id, &inner.doc),
        }
    }

    pub fn checkout_commit(&self, commit_id: &str) -> Result<(), String> {
        let repo_dir = {
            let inner = self.inner.lock().unwrap();
            let repo = inner.repo.as_ref().ok_or("Проект не открыт")?;
            repo.dir().to_path_buf()
        };

        let repo = ProjectGitRepo { dir: repo_dir };
        let doc = repo.checkout(commit_id)?;

        let mut inner = self.inner.lock().unwrap();
        inner.doc = doc;
        inner.dirty = false;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Script management
    // -----------------------------------------------------------------------

    pub fn list_scripts(&self) -> Vec<ProjectScript> {
        self.inner.lock().unwrap().doc.scripts.clone()
    }

    pub fn create_script(&self, name: &str) -> Result<ProjectScript, String> {
        let (repo_dir, id) = {
            let inner = self.inner.lock().unwrap();
            let repo = inner.repo.as_ref().ok_or("Проект не открыт")?;
            let id = format!("{:x}", now_ms());
            std::fs::create_dir_all(repo.scripts_dir()).map_err(|e| e.to_string())?;
            std::fs::write(repo.script_path(&id), "-- Lua script\n")
                .map_err(|e| e.to_string())?;
            (repo.dir().to_path_buf(), id)
        };
        let script = ProjectScript {
            id: id.clone(),
            name: name.to_string(),
            created_at_ms: now_ms(),
        };
        let mut inner = self.inner.lock().unwrap();
        inner.doc.scripts.push(script.clone());
        inner.dirty = true;
        let _ = repo_dir; // ensure not dropped early
        Ok(script)
    }

    pub fn delete_script(&self, id: &str) -> Result<(), String> {
        let repo_dir = {
            let inner = self.inner.lock().unwrap();
            let repo = inner.repo.as_ref().ok_or("Проект не открыт")?;
            repo.delete_script_file(id)?;
            repo.dir().to_path_buf()
        };
        let mut inner = self.inner.lock().unwrap();
        inner.doc.scripts.retain(|s| s.id != id);
        inner.dirty = true;
        let _ = repo_dir;
        Ok(())
    }

    pub fn get_script_content(&self, id: &str) -> Result<String, String> {
        let inner = self.inner.lock().unwrap();
        let repo = inner.repo.as_ref().ok_or("Проект не открыт")?;
        repo.read_script_content(id)
    }

    pub fn set_script_content(&self, id: &str, content: &str) -> Result<(), String> {
        let repo_dir = {
            let inner = self.inner.lock().unwrap();
            let repo = inner.repo.as_ref().ok_or("Проект не открыт")?;
            if !inner.doc.scripts.iter().any(|s| s.id == id) {
                return Err(format!("Скрипт {id} не найден"));
            }
            repo.dir().to_path_buf()
        };
        let repo = ProjectGitRepo { dir: repo_dir };
        repo.write_script_content(id, content)?;
        self.inner.lock().unwrap().dirty = true;
        Ok(())
    }

    /// Import a .lua file from disk into the project's scripts dir.
    pub fn import_script_file(&self, path: &Path) -> Result<ProjectScript, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Не удалось прочитать файл: {e}"))?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("script")
            .to_string();
        let id = format!("{:x}", now_ms());
        {
            let inner = self.inner.lock().unwrap();
            let repo = inner.repo.as_ref().ok_or("Проект не открыт")?;
            std::fs::create_dir_all(repo.scripts_dir()).map_err(|e| e.to_string())?;
            std::fs::write(repo.script_path(&id), &content).map_err(|e| e.to_string())?;
        }
        let script = ProjectScript { id, name, created_at_ms: now_ms() };
        let mut inner = self.inner.lock().unwrap();
        inner.doc.scripts.push(script.clone());
        inner.dirty = true;
        Ok(script)
    }

    pub fn script_history(&self, id: &str) -> Result<Vec<crate::project_repo::CommitSummary>, String> {
        let inner = self.inner.lock().unwrap();
        let repo = inner.repo.as_ref().ok_or("Проект не открыт")?;
        repo.script_history(id)
    }

    pub fn script_diff(&self, id: &str, from_id: &str, to_id: Option<&str>) -> Result<String, String> {
        let inner = self.inner.lock().unwrap();
        let repo = inner.repo.as_ref().ok_or("Проект не открыт")?;
        repo.script_diff(id, from_id, to_id)
    }

    /// Restore a script to the version from `commit_id`. Returns the restored content.
    pub fn checkout_script(&self, id: &str, commit_id: &str) -> Result<String, String> {
        let repo_dir = {
            let inner = self.inner.lock().unwrap();
            inner.repo.as_ref().ok_or("Проект не открыт")?.dir().to_path_buf()
        };
        let repo = ProjectGitRepo { dir: repo_dir };
        let content = repo.script_content_at_commit(id, commit_id)?;
        repo.write_script_content(id, &content)?;
        self.inner.lock().unwrap().dirty = true;
        Ok(content)
    }

    // -----------------------------------------------------------------------
    // Legacy-compatible session operations (internal impl now git-aware)
    // -----------------------------------------------------------------------

    pub fn prepare_for_save(&self, session: &EcuSession) -> Result<(), String> {
        if session.config().snapshot().loaded {
            self.sync_ecu_config_from_session(session)?;
        }
        Ok(())
    }

    pub fn capture_ecu_config(&self, session: &EcuSession) -> Result<(), String> {
        let snap = session.config().snapshot();
        let pages = session.config().config_pages();
        if pages.is_empty() || !snap.loaded {
            return Err(
                "Сначала загрузите конфигурацию с ECU (страница настроек)".into(),
            );
        }
        let ini = session.ini_context();
        let mut ecu = build_project_ecu_config(&pages, &ini, snap.values.clone());
        ecu.captured_at_ms = now_ms();

        let mut inner = self.inner.lock().unwrap();
        let project_dir = inner.repo.as_ref().map(|r| r.dir().to_path_buf());
        inner.doc.ecu_config = Some(ecu);
        inner.doc.ini = Some(ProjectIniRef {
            path: session.loaded_ini_path().map(|p| {
                ini_path_for_project(p.as_path(), project_dir.as_deref())
            }),
            signature: ini.signature.clone(),
        });
        inner.doc.touch();
        inner.dirty = true;
        Ok(())
    }

    pub fn set_ini_ref(&self, path: Option<String>, signature: Option<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner.doc.ini = Some(ProjectIniRef { path, signature });
        inner.doc.touch();
        inner.dirty = true;
    }

    pub fn add_log(&self, path: impl AsRef<Path>, label: Option<String>, kind: Option<&str>) {
        let path = path.as_ref();
        let path_str = path.display().to_string();
        let mut inner = self.inner.lock().unwrap();
        if inner.doc.logs.iter().any(|l| l.path == path_str) {
            return;
        }
        inner.doc.logs.push(ProjectLogRef {
            path: path_str,
            label,
            added_at_ms: now_ms(),
            kind: kind.unwrap_or("output_csv").into(),
        });
        inner.doc.touch();
        inner.dirty = true;
    }

    pub fn remove_log(&self, path: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.doc.logs.retain(|l| l.path != path);
        inner.doc.timeline.clips.retain(|c| c.record.path != path);
        inner.doc.touch();
        inner.dirty = true;
    }

    pub fn list_timeline_clips(&self) -> Vec<ProjectTimelineClip> {
        let inner = self.inner.lock().unwrap();
        let mut clips = inner.doc.timeline.clips.clone();
        for (idx, log) in inner.doc.logs.iter().enumerate() {
            if clips.iter().any(|c| c.record.path == log.path) {
                continue;
            }
            if let Some(ch) = channel::from_log_kind(&log.kind) {
                clips.push(ProjectTimelineClip {
                    id: format!("log-{idx}"),
                    channel: ch.to_string(),
                    start_ms: log.added_at_ms,
                    end_ms: None,
                    record: ProjectTimelineRecordRef::new(
                        log.path.clone(),
                        Some(log.kind.clone()),
                    ),
                    label: log.label.clone(),
                });
            }
        }
        clips.sort_by_key(|c| c.start_ms);
        clips.into_iter().map(clip_with_default_end).collect()
    }

    pub fn upsert_timeline_clip(&self, clip: ProjectTimelineClip) -> Result<(), String> {
        validate_channel(&clip.channel)?;
        if clip.record.path.trim().is_empty() {
            return Err("Путь записи не может быть пустым".into());
        }
        if clip.end_ms.is_some_and(|end| end < clip.start_ms) {
            return Err("Конец записи раньше начала".into());
        }
        let mut inner = self.inner.lock().unwrap();
        if let Some(existing) = inner.doc.timeline.clips.iter_mut().find(|c| c.id == clip.id) {
            *existing = clip;
        } else {
            inner.doc.timeline.clips.push(clip);
        }
        inner.doc.touch();
        inner.dirty = true;
        Ok(())
    }

    pub fn clear_timeline(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if inner.doc.timeline.clips.is_empty() {
            return false;
        }
        inner.doc.timeline.clips.clear();
        inner.doc.touch();
        inner.dirty = true;
        true
    }

    pub fn sync_ecu_config_from_session(&self, session: &EcuSession) -> Result<(), String> {
        let snap = session.config().snapshot();
        if !snap.loaded {
            return Err("Config не загружен в сессии".into());
        }
        let pages = session.config().config_pages();
        if pages.is_empty() {
            return Err("Пустой образ config".into());
        }
        let ini = session.ini_context();
        let owned: Vec<(u8, Vec<u8>)> =
            pages.iter().map(|(p, v)| (*p, v.clone())).collect();
        let slices: Vec<(u8, &[u8])> =
            owned.iter().map(|(p, v)| (*p, v.as_slice())).collect();
        let values = decode_config_fields_pages(&ini.config_fields, &slices);
        let mut ecu = build_project_ecu_config(&pages, &ini, values);
        ecu.captured_at_ms = now_ms();
        let mut inner = self.inner.lock().unwrap();
        inner.doc.ecu_config = Some(ecu);
        inner.doc.touch();
        inner.dirty = true;
        Ok(())
    }

    pub fn patch_ecu_config_field(
        &self,
        session: &EcuSession,
        field: &str,
        value: f64,
    ) -> Result<(), String> {
        let ini = session.ini_context();
        let mut inner = self.inner.lock().unwrap();
        let ecu = inner
            .doc
            .ecu_config
            .as_mut()
            .ok_or_else(|| "В проекте нет снимка ecuConfig".to_string())?;
        let mut pages = pages_from_project_ecu(ecu, &ini)?;
        let ini_page = ini
            .config_fields
            .get(field)
            .map(config_field_ini_page)
            .unwrap_or(DEFAULT_INI_PAGE);
        let page_len = ini_page_size(ini_page, &ini) as usize;
        let raw = pages.entry(ini_page).or_insert_with(|| vec![0u8; page_len]);
        encode_scalar_into_page(&ini, raw, field, value)?;
        let owned: Vec<(u8, Vec<u8>)> =
            pages.iter().map(|(p, v)| (*p, v.clone())).collect();
        let slices: Vec<(u8, &[u8])> =
            owned.iter().map(|(p, v)| (*p, v.as_slice())).collect();
        let mut values = decode_config_fields_pages(&ini.config_fields, &slices);
        values.insert(field.to_string(), value);
        *ecu = build_project_ecu_config(&pages, &ini, values);
        inner.doc.touch();
        inner.dirty = true;
        Ok(())
    }

    pub fn apply_to_session(&self, session: &EcuSession) -> Result<(), String> {
        let (ini_ref, ecu_config, project_dir) = {
            let inner = self.inner.lock().unwrap();
            (
                inner.doc.ini.clone(),
                inner.doc.ecu_config.clone(),
                inner.repo.as_ref().map(|r| r.dir().to_path_buf()),
            )
        };

        let project_ini_sig = ini_ref.as_ref().and_then(|r| r.signature.clone());
        session.set_project_panels_root(project_dir.as_deref());

        if !ini_ref_actionable(ini_ref.as_ref()) {
            println!("[workspace-fsm] apply_to_session: нет ini — pending выбор INI");
            session.set_pending_project_ini_required(
                "В проекте нет секции ini — выберите или загрузите файл INI",
                project_ini_sig.clone(),
            );
            session.set_project_ini_signature(project_ini_sig);
            return Ok(());
        }

        let ini = ini_ref.as_ref().expect("actionable => Some");
        if let Err(e) = load_project_ini(session, Some(ini), project_dir.as_deref()) {
            println!("[workspace-fsm] apply_to_session: ini не загружен — pending: {e}");
            session.set_pending_project_ini_required(e, project_ini_sig.clone());
            session.set_project_ini_signature(project_ini_sig);
            return Ok(());
        }

        if !session.is_connected() {
            session.clear_pending_ini_resolution();
        }

        if let Some(ecu) = ecu_config {
            session
                .config()
                .apply_from_project(&ecu, project_ini_sig.as_deref())?;
        }
        session.set_project_ini_signature(project_ini_sig);
        Ok(())
    }

    pub fn apply_ecu_config_if_present(&self, session: &EcuSession) -> Result<(), String> {
        let (ecu_config, project_ini_sig) = {
            let inner = self.inner.lock().unwrap();
            (
                inner.doc.ecu_config.clone(),
                inner.doc.ini.as_ref().and_then(|r| r.signature.clone()),
            )
        };
        if let Some(ecu) = ecu_config {
            session
                .config()
                .apply_from_project(&ecu, project_ini_sig.as_deref())?;
        }
        Ok(())
    }

    /// Fork current project without timeline. Returns the new project's directory.
    pub fn fork_without_timeline(
        &self,
        new_name: &str,
        session: &EcuSession,
    ) -> Result<PathBuf, String> {
        self.prepare_for_save(session)?;
        let doc = self.inner.lock().unwrap().doc.clone();
        let (repo, new_doc) = ProjectGitRepo::fork_without_timeline(&doc, new_name)?;
        let dir = repo.dir().to_path_buf();
        let mut inner = self.inner.lock().unwrap();
        *inner = ProjectInner { repo: Some(repo), doc: new_doc, dirty: false };
        Ok(dir)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ini_ref_actionable(ini_ref: Option<&ProjectIniRef>) -> bool {
    let Some(ini) = ini_ref else { return false; };
    ini.path.as_deref().is_some_and(|p| !p.is_empty())
        || ini.signature.as_deref().is_some_and(|s| !s.is_empty())
}

/// INI path stored in project.json: relative to the project directory if possible.
fn ini_path_for_project(ini: &Path, project_dir: Option<&Path>) -> String {
    if let Some(dir) = project_dir {
        if let Ok(rel) = ini.strip_prefix(dir) {
            return rel.display().to_string();
        }
    }
    ini.display().to_string()
}

fn resolve_ini_path(path: &str, project_dir: Option<&Path>) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    if let Some(dir) = project_dir {
        return dir.join(p);
    }
    p.to_path_buf()
}

fn load_project_ini(
    session: &EcuSession,
    ini_ref: Option<&ProjectIniRef>,
    project_dir: Option<&Path>,
) -> Result<(), String> {
    let Some(ini_ref) = ini_ref else {
        return Err(
            "В проекте нет секции ini — сохраните проект после загрузки config с ECU".into(),
        );
    };

    if let Some(path) = ini_ref.path.as_deref().filter(|p| !p.is_empty()) {
        let resolved = resolve_ini_path(path, project_dir);
        if resolved.is_file() {
            session.load_ini_from_path(&resolved)?;
            return Ok(());
        }
    }

    if let Some(sig) = ini_ref.signature.as_deref().filter(|s| !s.is_empty()) {
        let resolved = resolve_ini_for_signature(sig).map_err(|e| {
            format!(
                "INI для signature проекта не найден ({sig}): {e}. \
                 Поместите ini.path рядом с папкой проекта."
            )
        })?;
        session.apply_ini(resolved);
        return Ok(());
    }

    Err(
        "В проекте нет рабочего ini.path и ini.signature — нельзя декодировать ecuConfig".into(),
    )
}

fn ini_page_size(ini_page: u8, ini: &crate::sources::output_channels::IniContext) -> u32 {
    let idx = ini_page.saturating_sub(1) as usize;
    ini.page_sizes.get(idx).copied().unwrap_or(ini.page_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_timeline::{channel, ProjectTimelineRecordRef, DEFAULT_CLIP_DURATION_MS};
    use crate::ui_persist::{LogUiSettings, PERSIST_KEY_OUTPUT_CHART};

    #[test]
    fn project_json_roundtrip() {
        let mut p = RusefuiProject::new_named("test");
        p.logs.push(ProjectLogRef {
            path: "/tmp/log.csv".into(),
            label: Some("run1".into()),
            added_at_ms: 1,
            kind: "output_csv".into(),
        });
        let mut settings: LogUiSettings =
            serde_json::from_value(ui_persist::get(&p.ui, PERSIST_KEY_OUTPUT_CHART).unwrap())
                .unwrap();
        settings.zoom_step_pct = 12;
        ui_persist::set(
            &mut p.ui,
            PERSIST_KEY_OUTPUT_CHART,
            serde_json::to_value(&settings).unwrap(),
        )
        .unwrap();
        let text = serde_json::to_string_pretty(&p).unwrap();
        let back: RusefuiProject = serde_json::from_str(&text).unwrap();
        assert_eq!(back.name, "test");
        assert_eq!(back.logs.len(), 1);
        let loaded: LogUiSettings = serde_json::from_value(
            ui_persist::get(&back.ui, PERSIST_KEY_OUTPUT_CHART).unwrap(),
        )
        .unwrap();
        assert_eq!(loaded.zoom_step_pct, 12);
    }

    #[test]
    fn timeline_clip_without_end_gets_default_duration() {
        let store = ProjectStore::new();
        store
            .upsert_timeline_clip(ProjectTimelineClip {
                id: "c1".into(),
                channel: channel::LOGS.into(),
                start_ms: 1000,
                end_ms: None,
                record: ProjectTimelineRecordRef::new("/tmp/a.csv", Some("output_csv".into())),
                label: None,
            })
            .unwrap();
        let list = store.list_timeline_clips();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].end_ms, Some(1000 + DEFAULT_CLIP_DURATION_MS));
    }

    #[test]
    fn timeline_clip_roundtrip_in_project() {
        let store = ProjectStore::new();
        store
            .upsert_timeline_clip(ProjectTimelineClip {
                id: "c1".into(),
                channel: channel::LOGS.into(),
                start_ms: 1000,
                end_ms: Some(8000),
                record: ProjectTimelineRecordRef::new("/tmp/a.csv", Some("output_csv".into())),
                label: Some("run".into()),
            })
            .unwrap();
        let list = store.list_timeline_clips();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].end_ms, Some(8000));
    }

    #[test]
    fn clear_timeline_removes_persisted_clips() {
        let store = ProjectStore::new();
        store
            .upsert_timeline_clip(ProjectTimelineClip {
                id: "c1".into(),
                channel: channel::LOGS.into(),
                start_ms: 1,
                end_ms: None,
                record: ProjectTimelineRecordRef::new("/tmp/a.csv", Some("output_csv".into())),
                label: None,
            })
            .unwrap();
        assert!(store.clear_timeline());
        assert_eq!(store.info().timeline_clip_count, 0);
        assert!(!store.clear_timeline());
    }
}
