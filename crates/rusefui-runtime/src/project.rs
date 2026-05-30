//! Файл проекта rusefui (JSON): снимок config ECU, ссылки на логи, настройки UI.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusefi_ini::{config_field_ini_page, decode_config_fields_pages, DEFAULT_INI_PAGE};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config_diff::encode_scalar_into_page;
use crate::ini::resolve_ini_for_signature;
use crate::session::EcuSession;
use crate::sources::config::{
    build_default_ecu_config, build_project_ecu_config, pages_from_project_ecu,
};
use crate::project_timeline::{
    channel, clip_with_default_end, validate_channel, ProjectTimeline, ProjectTimelineClip,
    ProjectTimelineRecordRef,
};
use crate::ui_persist::{self, ProjectUi};

pub const FORMAT_VERSION: u32 = 1;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RusefuiProject {
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
    /// Сырой INI page 1 (основная калибровка), base64.
    pub raw_page0_base64: String,
    /// Доп. страницы INI (`"2"`…`"4"` → base64). Second ignition/VE — page 4.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub config_pages_base64: HashMap<String, String>,
    /// Декодированные скаляры/enum для просмотра и diff.
    pub values: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLogRef {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub added_at_ms: u64,
    /// Например `output_csv`.
    pub kind: String,
}

impl RusefuiProject {
    pub fn new_named(name: impl Into<String>) -> Self {
        let t = now_ms();
        let mut ui = ProjectUi::default();
        ui_persist::init_document_ui(&mut ui);
        Self {
            format_version: FORMAT_VERSION,
            name: name.into(),
            created_at_ms: t,
            updated_at_ms: t,
            ini: None,
            ecu_config: None,
            logs: Vec::new(),
            timeline: ProjectTimeline::default(),
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
    pub path: Option<String>,
    pub name: String,
    pub dirty: bool,
    pub log_count: usize,
    pub timeline_clip_count: usize,
    pub has_ecu_config: bool,
    pub ini_signature: Option<String>,
    pub ini_path: Option<String>,
}

pub struct ProjectStore {
    path: Mutex<Option<PathBuf>>,
    dirty: Mutex<bool>,
    doc: Mutex<RusefuiProject>,
}

impl Default for ProjectStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectStore {
    pub fn new() -> Self {
        Self {
            path: Mutex::new(None),
            dirty: Mutex::new(false),
            doc: Mutex::new(RusefuiProject::new_named("Новый проект")),
        }
    }

    pub fn info(&self) -> ProjectInfo {
        let path = self.path.lock().unwrap().clone();
        let dirty = *self.dirty.lock().unwrap();
        let doc = self.doc.lock().unwrap();
        ProjectInfo {
            path: path.map(|p| p.display().to_string()),
            name: doc.name.clone(),
            dirty,
            log_count: doc.logs.len(),
            timeline_clip_count: doc.timeline.clips.len(),
            has_ecu_config: doc.ecu_config.is_some(),
            ini_signature: doc.ini.as_ref().and_then(|i| i.signature.clone()),
            ini_path: doc.ini.as_ref().and_then(|i| i.path.clone()),
        }
    }

    pub fn document(&self) -> RusefuiProject {
        self.doc.lock().unwrap().clone()
    }

    pub fn ui_get(&self, key: &str) -> Result<Value, String> {
        let doc = self.doc.lock().unwrap();
        ui_persist::get(&doc.ui, key)
    }

    pub fn ui_set(&self, key: &str, value: Value) -> Result<(), String> {
        let mut doc = self.doc.lock().unwrap();
        // Не помечать грязным если значение не изменилось (например, восстановление UI после загрузки).
        let existing = doc.ui.sections.get(key).cloned();
        ui_persist::set(&mut doc.ui, key, value)?;
        let changed = existing.as_ref() != doc.ui.sections.get(key);
        if changed {
            doc.touch();
            *self.dirty.lock().unwrap() = true;
        }
        Ok(())
    }

    pub fn ui_persist_keys(&self) -> Vec<&'static str> {
        ui_persist::persist_keys()
    }

    pub fn new_document(&self, name: String) {
        *self.doc.lock().unwrap() = RusefuiProject::new_named(name);
        *self.path.lock().unwrap() = None;
        *self.dirty.lock().unwrap() = false;
    }

    fn ini_ref_is_valid(ini_ref: Option<&ProjectIniRef>) -> bool {
        ini_ref.is_some_and(|r| {
            r.path.as_deref().is_some_and(|p| !p.is_empty())
                || r.signature.as_deref().is_some_and(|s| !s.is_empty())
        })
    }

    /// Новый проект на диске с обязательной привязкой INI.
    pub fn create_with_ini(
        &self,
        name: String,
        project_path: &Path,
        ini_path: &Path,
        session: &EcuSession,
        force: bool,
    ) -> Result<(), String> {
        session.apply_ini_with_options(ini_path, force)?;
        let ini = session.ini_context();
        let signature = ini.signature.clone();
        let rel_ini = Self::ini_path_for_project_store(ini_path, Some(project_path));

        let mut doc = RusefuiProject::new_named(name);
        doc.ini = Some(ProjectIniRef {
            path: Some(rel_ini),
            signature: signature.clone(),
        });
        doc.ecu_config = Some(build_default_ecu_config(&ini));
        Self::write_document_to_path(&doc, project_path)?;
        *self.doc.lock().unwrap() = doc;
        *self.path.lock().unwrap() = Some(project_path.to_path_buf());
        *self.dirty.lock().unwrap() = false;
        session.set_project_ini_signature(signature);
        Ok(())
    }

    /// Сменить INI открытого проекта; при смене signature сбрасывает `ecuConfig`.
    pub fn change_ini(
        &self,
        session: &EcuSession,
        ini_path: &Path,
        force: bool,
    ) -> Result<(), String> {
        let project_path = self
            .saved_path()
            .ok_or_else(|| "Нет открытого проекта".to_string())?;
        let old_signature = self
            .doc
            .lock()
            .unwrap()
            .ini
            .as_ref()
            .and_then(|i| i.signature.clone());

        session.apply_ini_with_options(ini_path, force)?;
        let ini = session.ini_context();
        let loaded = session
            .loaded_ini_path()
            .unwrap_or_else(|| ini_path.to_path_buf());
        let rel = Self::ini_path_for_project_store(&loaded, Some(&project_path));
        let signature_changed = old_signature.as_deref() != ini.signature.as_deref();

        {
            let mut doc = self.doc.lock().unwrap();
            doc.ini = Some(ProjectIniRef {
                path: Some(rel),
                signature: ini.signature.clone(),
            });
            if signature_changed {
                doc.ecu_config = None;
            }
            doc.touch();
        }
        *self.dirty.lock().unwrap() = true;
        session.set_project_ini_signature(ini.signature.clone());
        if signature_changed {
            session.config().stop();
        }
        Ok(())
    }

    pub fn set_name(&self, name: String) {
        let mut doc = self.doc.lock().unwrap();
        doc.name = name;
        doc.touch();
        *self.dirty.lock().unwrap() = true;
    }

    pub fn load_from_path(&self, path: &Path) -> Result<(), String> {
        let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut doc: RusefuiProject = serde_json::from_str(&text)
            .map_err(|e| format!("Некорректный JSON проекта: {e}"))?;
        if doc.format_version != FORMAT_VERSION {
            return Err(format!(
                "Версия формата {} не поддерживается (ожидается {FORMAT_VERSION})",
                doc.format_version
            ));
        }
        doc.timeline.migrate_legacy();
        if !Self::ini_ref_is_valid(doc.ini.as_ref()) {
            return Err(
                "В проекте нет секции ini — создайте проект заново с выбором INI".into(),
            );
        }
        *self.doc.lock().unwrap() = doc;
        *self.path.lock().unwrap() = Some(path.to_path_buf());
        *self.dirty.lock().unwrap() = false;
        Ok(())
    }

    /// Перед `save_to_path`: скопировать в JSON актуальный page 0 из сессии.
    ///
    /// Иначе в файле остаётся старый `ecuConfig` (первый «Снимок config» или прошлое
    /// открытие проекта), хотя на экране уже данные после чтения с ECU.
    pub fn prepare_for_save(&self, session: &EcuSession) -> Result<(), String> {
        if session.config().snapshot().loaded {
            self.sync_ecu_config_from_session(session)?;
        }
        Ok(())
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), String> {
        let mut doc = self.doc.lock().unwrap();
        doc.touch();
        Self::write_document_to_path(&doc, path)?;
        drop(doc);
        *self.path.lock().unwrap() = Some(path.to_path_buf());
        *self.dirty.lock().unwrap() = false;
        Ok(())
    }

    fn write_document_to_path(doc: &RusefuiProject, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let text =
            serde_json::to_string_pretty(doc).map_err(|e| format!("Сериализация: {e}"))?;
        fs::write(path, text).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Копия проекта на диск без секции `timeline` (клипы не переносятся).
    pub fn write_copy_without_timeline(
        &self,
        path: &Path,
        session: &EcuSession,
    ) -> Result<(), String> {
        self.prepare_for_save(session)?;
        let mut doc = self.doc.lock().unwrap().clone();
        doc.timeline = ProjectTimeline::default();
        let base = doc.name.trim();
        doc.name = if base.ends_with("(копия)") {
            base.to_string()
        } else {
            format!("{base} (копия)")
        };
        let t = now_ms();
        doc.created_at_ms = t;
        doc.updated_at_ms = t;
        Self::write_document_to_path(&doc, path)
    }

    pub fn clear_timeline(&self) -> bool {
        let mut doc = self.doc.lock().unwrap();
        if doc.timeline.clips.is_empty() {
            return false;
        }
        doc.timeline.clips.clear();
        doc.touch();
        drop(doc);
        *self.dirty.lock().unwrap() = true;
        true
    }

    pub fn saved_path(&self) -> Option<PathBuf> {
        self.path.lock().unwrap().clone()
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
        let mut doc = self.doc.lock().unwrap();
        doc.ecu_config = Some(ecu);
        doc.ini = Some(ProjectIniRef {
            path: session.loaded_ini_path().map(|p| {
                Self::ini_path_for_project_store(
                    p.as_path(),
                    self.path.lock().unwrap().as_deref(),
                )
            }),
            signature: ini.signature.clone(),
        });
        doc.touch();
        *self.dirty.lock().unwrap() = true;
        Ok(())
    }

    /// Перезаписать ссылку на INI в проекте (вызывается после ручного применения
    /// несовпадавшего INI, чтобы при следующем `project_load` подгрузить нужный файл).
    pub fn set_ini_ref(&self, path: Option<String>, signature: Option<String>) {
        let mut doc = self.doc.lock().unwrap();
        doc.ini = Some(ProjectIniRef { path, signature });
        doc.touch();
        *self.dirty.lock().unwrap() = true;
    }

    pub fn add_log(
        &self,
        path: impl AsRef<Path>,
        label: Option<String>,
        kind: Option<&str>,
    ) {
        let path = path.as_ref();
        let path_str = path.display().to_string();
        let mut doc = self.doc.lock().unwrap();
        if doc.logs.iter().any(|l| l.path == path_str) {
            return;
        }
        doc.logs.push(ProjectLogRef {
            path: path_str,
            label,
            added_at_ms: now_ms(),
            kind: kind.unwrap_or("output_csv").into(),
        });
        doc.touch();
        *self.dirty.lock().unwrap() = true;
    }

    pub fn remove_log(&self, path: &str) {
        let mut doc = self.doc.lock().unwrap();
        doc.logs.retain(|l| l.path != path);
        doc.timeline.clips.retain(|c| c.record.path != path);
        doc.touch();
        *self.dirty.lock().unwrap() = true;
    }

    pub fn list_timeline_clips(&self) -> Vec<ProjectTimelineClip> {
        let doc = self.doc.lock().unwrap();
        let mut clips = doc.timeline.clips.clone();
        for (idx, log) in doc.logs.iter().enumerate() {
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
        let mut doc = self.doc.lock().unwrap();
        if let Some(existing) = doc.timeline.clips.iter_mut().find(|c| c.id == clip.id) {
            *existing = clip;
        } else {
            doc.timeline.clips.push(clip);
        }
        doc.touch();
        *self.dirty.lock().unwrap() = true;
        Ok(())
    }

    /// Скопировать текущий page 0 из сессии в `ecuConfig` проекта (offline-редактирование).
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
        let owned: Vec<(u8, Vec<u8>)> = pages.iter().map(|(p, v)| (*p, v.clone())).collect();
        let slices: Vec<(u8, &[u8])> = owned.iter().map(|(p, v)| (*p, v.as_slice())).collect();
        let values = decode_config_fields_pages(&ini.config_fields, &slices);
        let mut ecu = build_project_ecu_config(&pages, &ini, values);
        ecu.captured_at_ms = now_ms();
        let mut doc = self.doc.lock().unwrap();
        doc.ecu_config = Some(ecu);
        doc.touch();
        *self.dirty.lock().unwrap() = true;
        Ok(())
    }

    /// Обновить одно поле в `ecuConfig` проекта (после выбора значения с ECU в diff).
    pub fn patch_ecu_config_field(
        &self,
        session: &EcuSession,
        field: &str,
        value: f64,
    ) -> Result<(), String> {
        let ini = session.ini_context();
        let mut doc = self.doc.lock().unwrap();
        let ecu = doc
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
        let owned: Vec<(u8, Vec<u8>)> = pages.iter().map(|(p, v)| (*p, v.clone())).collect();
        let slices: Vec<(u8, &[u8])> = owned.iter().map(|(p, v)| (*p, v.as_slice())).collect();
        let mut values = decode_config_fields_pages(&ini.config_fields, &slices);
        values.insert(field.to_string(), value);
        *ecu = build_project_ecu_config(&pages, &ini, values);
        doc.touch();
        *self.dirty.lock().unwrap() = true;
        Ok(())
    }

    /// После `load_from_path`: INI из проекта + снимок config в сессию.
    pub fn apply_to_session(&self, session: &EcuSession) -> Result<(), String> {
        let (ini_ref, ecu_config, project_path) = {
            let doc = self.doc.lock().unwrap();
            (
                doc.ini.clone(),
                doc.ecu_config.clone(),
                self.path.lock().unwrap().clone(),
            )
        };

        let project_ini_sig = ini_ref.as_ref().and_then(|r| r.signature.clone());

        if ecu_config.is_some() {
            Self::load_project_ini(
                session,
                ini_ref.as_ref(),
                project_path.as_deref(),
            )?;
        } else if let Some(ini_ref) = ini_ref.as_ref() {
            if let Some(path) = ini_ref.path.as_deref().filter(|p| !p.is_empty()) {
                let resolved = Self::resolve_ini_path(path, project_path.as_deref());
                if resolved.is_file() {
                    session.load_ini_from_path(&resolved)?;
                }
            }
            session.bootstrap_offline_ini_if_needed();
        } else {
            session.bootstrap_offline_ini_if_needed();
        }

        let ecu_config = {
            let mut doc = self.doc.lock().unwrap();
            if doc.ecu_config.is_none() {
                let ini = session.ini_context();
                if !ini.config_fields.is_empty() {
                    doc.ecu_config = Some(build_default_ecu_config(&ini));
                    doc.touch();
                    *self.dirty.lock().unwrap() = true;
                }
            }
            doc.ecu_config.clone()
        };

        if let Some(ecu) = ecu_config {
            session
                .config()
                .apply_from_project(&ecu, project_ini_sig.as_deref())?;
        }
        session.set_project_ini_signature(project_ini_sig);
        Ok(())
    }

    /// Путь INI в JSON проекта: относительно файла `.rusefui`, если возможно.
    fn ini_path_for_project_store(ini: &Path, project_file: Option<&Path>) -> String {
        if let Some(proj) = project_file {
            if let Some(parent) = proj.parent() {
                if let Ok(rel) = ini.strip_prefix(parent) {
                    return rel.display().to_string();
                }
            }
        }
        ini.display().to_string()
    }

    fn resolve_ini_path(path: &str, project_file: Option<&Path>) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            return p.to_path_buf();
        }
        if let Some(dir) = project_file.and_then(|f| f.parent()) {
            return dir.join(p);
        }
        p.to_path_buf()
    }

    /// INI с тем же layout, что при `capture_ecu_config` (path или signature из проекта).
    fn load_project_ini(
        session: &EcuSession,
        ini_ref: Option<&ProjectIniRef>,
        project_file: Option<&Path>,
    ) -> Result<(), String> {
        let Some(ini_ref) = ini_ref else {
            return Err(
                "В проекте нет секции ini — сохраните проект после загрузки config с ECU".into(),
            );
        };

        if let Some(path) = ini_ref.path.as_deref().filter(|p| !p.is_empty()) {
            let resolved = Self::resolve_ini_path(path, project_file);
            if resolved.is_file() {
                session.load_ini_from_path(&resolved)?;
                return Ok(());
            }
        }

        if let Some(sig) = ini_ref.signature.as_deref().filter(|s| !s.is_empty()) {
            let resolved = resolve_ini_for_signature(sig).map_err(|e| {
                format!(
                    "INI для signature проекта не найден ({sig}): {e}. \
                     Укажите корректный ini.path рядом с файлом проекта."
                )
            })?;
            session.apply_ini(resolved);
            if let Err(e) = session.ensure_ui_panels() {
                session.log_panel_cache_error("project_load", e);
            }
            return Ok(());
        }

        Err(
            "В проекте нет рабочего ini.path и ini.signature — нельзя декодировать ecuConfig"
                .into(),
        )
    }
}

fn ini_page_size(ini_page: u8, ini: &crate::sources::output_channels::IniContext) -> u32 {
    let idx = ini_page.saturating_sub(1) as usize;
    ini.page_sizes
        .get(idx)
        .copied()
        .unwrap_or(ini.page_size)
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

    #[test]
    fn copy_without_timeline_writes_empty_timeline_section() {
        use crate::protocol_log::ProtocolLogStore;
        use crate::session::EcuSession;

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
        let session = EcuSession::new_arc(ProtocolLogStore::new(std::env::temp_dir().join(
            format!("rusefui-proto-test-{}", now_ms()),
        )));
        let dir = std::env::temp_dir().join(format!("rusefui-copy-test-{}", now_ms()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("copy.json");
        store.write_copy_without_timeline(&path, &session).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let back: RusefuiProject = serde_json::from_str(&text).unwrap();
        assert!(back.timeline.clips.is_empty());
        assert!(back.name.contains("(копия)"));
        let _ = fs::remove_dir_all(dir);
    }
}
