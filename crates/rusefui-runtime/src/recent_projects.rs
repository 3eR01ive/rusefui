//! Список недавно открытых файлов проекта (настройки приложения, не в JSON проекта).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_ENTRIES: usize = 12;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProjectEntry {
    pub path: String,
    /// Имя файла без расширения для отображения в списке.
    pub label: String,
    pub exists: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RecentProjectsFile {
    #[serde(default)]
    paths: Vec<String>,
}

pub struct RecentProjectsStore {
    file_path: PathBuf,
}

impl RecentProjectsStore {
    pub fn new() -> Self {
        Self {
            file_path: Self::default_file_path(),
        }
    }

    #[cfg(test)]
    pub fn with_file_path(file_path: PathBuf) -> Self {
        Self { file_path }
    }

    pub fn default_file_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rusefui")
            .join("recent-projects.json")
    }

    pub fn list_entries(&self) -> Vec<RecentProjectEntry> {
        self.load_paths()
            .into_iter()
            .map(|p| entry_from_path(&p))
            .collect()
    }

    pub fn record(&self, path: &Path) -> Result<(), String> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if !path.exists() {
            return Ok(());
        }
        let key = path.to_string_lossy().into_owned();
        let mut paths = self.load_paths();
        paths.retain(|p| p.to_string_lossy() != key);
        paths.insert(0, path);
        paths.truncate(MAX_ENTRIES);
        self.save_paths(&paths)
    }

    fn load_paths(&self) -> Vec<PathBuf> {
        let Ok(raw) = fs::read_to_string(&self.file_path) else {
            return Vec::new();
        };
        let file: RecentProjectsFile = serde_json::from_str(&raw).unwrap_or_default();
        file.paths.into_iter().map(PathBuf::from).collect()
    }

    fn save_paths(&self, paths: &[PathBuf]) -> Result<(), String> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let file = RecentProjectsFile {
            paths: paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        };
        let json = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
        let tmp = self.file_path.with_extension("json.tmp");
        fs::write(&tmp, json).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.file_path).map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn entry_from_path(path: &Path) -> RecentProjectEntry {
    let exists = path.exists();
    // For directories (new git projects): use file_name. For legacy files: strip extension.
    let label = if path.is_dir() {
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string()
    } else {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_else(|| path.file_name().and_then(|s| s.to_str()).unwrap_or(""))
            .to_string()
    };
    RecentProjectEntry {
        path: path.to_string_lossy().into_owned(),
        label,
        exists,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store() -> (RecentProjectsStore, PathBuf) {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let dir = std::env::temp_dir().join(format!("rusefui-recent-test-{ms}"));
        let file = dir.join("recent-projects.json");
        (RecentProjectsStore::with_file_path(file), dir)
    }

    #[test]
    fn record_moves_to_front_and_dedupes() {
        let (store, dir) = temp_store();
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.rusefui");
        let b = dir.join("b.rusefui");
        fs::write(&a, "{}").unwrap();
        fs::write(&b, "{}").unwrap();
        store.record(&a).unwrap();
        store.record(&b).unwrap();
        store.record(&a).unwrap();
        let paths = store.load_paths();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], a);
    }
}
