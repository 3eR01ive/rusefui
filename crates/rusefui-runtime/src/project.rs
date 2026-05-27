//! Файл проекта rusefui (JSON): снимок config ECU, ссылки на логи, настройки UI.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rusefi_ini::decode_config_fields;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config_diff::encode_scalar_into_page;
use crate::ini::resolve_ini_for_signature;
use crate::session::EcuSession;
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
    /// Сырой page 0 (как после чтения с ECU), base64.
    pub raw_page0_base64: String,
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
    pub has_ecu_config: bool,
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
            has_ecu_config: doc.ecu_config.is_some(),
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

    pub fn set_name(&self, name: String) {
        let mut doc = self.doc.lock().unwrap();
        doc.name = name;
        doc.touch();
        *self.dirty.lock().unwrap() = true;
    }

    pub fn load_from_path(&self, path: &Path) -> Result<(), String> {
        let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let doc: RusefuiProject = serde_json::from_str(&text)
            .map_err(|e| format!("Некорректный JSON проекта: {e}"))?;
        if doc.format_version != FORMAT_VERSION {
            return Err(format!(
                "Версия формата {} не поддерживается (ожидается {FORMAT_VERSION})",
                doc.format_version
            ));
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
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let mut doc = self.doc.lock().unwrap();
        doc.touch();
        let text =
            serde_json::to_string_pretty(&*doc).map_err(|e| format!("Сериализация: {e}"))?;
        fs::write(path, text).map_err(|e| e.to_string())?;
        drop(doc);
        *self.path.lock().unwrap() = Some(path.to_path_buf());
        *self.dirty.lock().unwrap() = false;
        Ok(())
    }

    pub fn saved_path(&self) -> Option<PathBuf> {
        self.path.lock().unwrap().clone()
    }

    pub fn capture_ecu_config(&self, session: &EcuSession) -> Result<(), String> {
        let snap = session.config().snapshot();
        let mut raw = session.config().page_raw();
        if raw.is_empty() || !snap.loaded {
            return Err(
                "Сначала загрузите конфигурацию с ECU (страница настроек)".into(),
            );
        }
        let ini = session.ini_context();
        let page_len = ini.page_size as usize;
        if raw.len() < page_len {
            raw.resize(page_len, 0);
        }
        let mut doc = self.doc.lock().unwrap();
        doc.ecu_config = Some(ProjectEcuConfig {
            captured_at_ms: now_ms(),
            page_size: ini.page_size,
            raw_page0_base64: B64.encode(&raw),
            values: snap.values.clone(),
        });
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
        doc.touch();
        *self.dirty.lock().unwrap() = true;
    }

    /// Скопировать текущий page 0 из сессии в `ecuConfig` проекта (offline-редактирование).
    pub fn sync_ecu_config_from_session(&self, session: &EcuSession) -> Result<(), String> {
        let snap = session.config().snapshot();
        if !snap.loaded {
            return Err("Config не загружен в сессии".into());
        }
        let mut raw = session.config().page_raw();
        if raw.is_empty() {
            return Err("Пустой образ page 0".into());
        }
        let ini = session.ini_context();
        let page_len = ini.page_size as usize;
        if raw.len() < page_len {
            raw.resize(page_len, 0);
        }
        let values = decode_config_fields(&ini.config_fields, &raw);
        let mut doc = self.doc.lock().unwrap();
        doc.ecu_config = Some(ProjectEcuConfig {
            captured_at_ms: now_ms(),
            page_size: ini.page_size,
            raw_page0_base64: B64.encode(&raw),
            values,
        });
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
        let mut raw = B64
            .decode(&ecu.raw_page0_base64)
            .map_err(|e| format!("page0 base64: {e}"))?;
        encode_scalar_into_page(&ini, &mut raw, field, value)?;
        ecu.raw_page0_base64 = B64.encode(&raw);
        ecu.values = decode_config_fields(&ini.config_fields, &raw);
        ecu.values.insert(field.to_string(), value);
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
            return Ok(());
        }

        Err(
            "В проекте нет рабочего ini.path и ini.signature — нельзя декодировать ecuConfig"
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
