//! Кэш YAML-панелей из INI: `~/.rusEFI/projects/{project_key}/ui_panels/{ini_hash}/`.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusefi_ini::{convert_ini_path, ConvertResult, PanelManifest};
use serde::Serialize;

use super::signature::parse_rusefi_signature;

/// Увеличивать при изменении конвертера INI → YAML (устаревший cache пересоздаётся).
pub const PANEL_CACHE_GENERATOR_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelCacheStatus {
    pub hash: String,
    pub project_key: String,
    pub dir: String,
    pub manifest_path: String,
    pub generated: bool,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Стабильный ключ каталога для файла проекта (или `scratch` без пути).
pub fn project_cache_key(project_path: Option<&Path>) -> String {
    match project_path {
        None => "scratch".into(),
        Some(p) => {
            let canonical = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
            let mut hasher = DefaultHasher::new();
            canonical.hash(&mut hasher);
            format!("{:016x}", hasher.finish())
        }
    }
}

/// Корень user-level cache (`~/.rusEFI/projects/`).
pub fn projects_cache_root() -> PathBuf {
    if let Ok(dir) = std::env::var("RUSEFI_UI_PANELS_DIR") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .map(|home| home.join(".rusEFI").join("projects"))
        .unwrap_or_else(|| PathBuf::from(".rusEFI/projects"))
}

pub fn project_panels_root(project_key: &str) -> PathBuf {
    projects_cache_root().join(project_key).join("ui_panels")
}

pub fn cache_dir_for_project_ini(project_key: &str, ini_hash: &str) -> PathBuf {
    project_panels_root(project_key).join(ini_hash)
}

fn remove_cache_dir(cache_dir: &Path) -> Result<(), String> {
    if cache_dir.is_dir() {
        fs::remove_dir_all(cache_dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn is_cache_valid(manifest: &PanelManifest, cache_dir: &Path, ini_path: &Path) -> bool {
    if manifest.generator_version != Some(PANEL_CACHE_GENERATOR_VERSION) {
        return false;
    }

    for entry in &manifest.panels {
        if !cache_dir.join(&entry.file).is_file() {
            return false;
        }
    }

    if let (Ok(meta), Some(gen_ms)) = (fs::metadata(ini_path), manifest.generated_at_ms) {
        if let Ok(modified) = meta.modified() {
            let generated = UNIX_EPOCH + Duration::from_millis(gen_ms);
            if modified > generated {
                return false;
            }
        }
    }

    true
}

/// Создать cache при miss или устаревшем hit; валидный hit — только вернуть путь.
pub fn ensure_panels_for_ini(
    ini_path: &Path,
    signature: &str,
    project_key: &str,
) -> Result<PanelCacheStatus, String> {
    let parsed = parse_rusefi_signature(signature)
        .ok_or_else(|| format!("signature не парсится для panel cache: {signature}"))?;
    let hash = parsed.hash;
    let cache_dir = cache_dir_for_project_ini(project_key, &hash);
    let manifest_path = cache_dir.join("manifest.json");

    if manifest_path.is_file() {
        if let Ok(manifest) = read_manifest_from_dir(&cache_dir) {
            if is_cache_valid(&manifest, &cache_dir, ini_path) {
                return Ok(PanelCacheStatus {
                    hash,
                    project_key: project_key.to_string(),
                    dir: cache_dir.display().to_string(),
                    manifest_path: manifest_path.display().to_string(),
                    generated: false,
                });
            }
        }
        remove_cache_dir(&cache_dir)?;
    }

    let result = convert_ini_path(ini_path).map_err(|e| e.to_string())?;
    write_panel_cache(&cache_dir, &result, signature, &hash, ini_path)?;

    Ok(PanelCacheStatus {
        hash,
        project_key: project_key.to_string(),
        dir: cache_dir.display().to_string(),
        manifest_path: manifest_path.display().to_string(),
        generated: true,
    })
}

fn write_panel_cache(
    cache_dir: &Path,
    result: &ConvertResult,
    signature: &str,
    hash: &str,
    ini_path: &Path,
) -> Result<(), String> {
    fs::create_dir_all(cache_dir).map_err(|e| e.to_string())?;
    for (name, content) in &result.files {
        fs::write(cache_dir.join(name), content).map_err(|e| e.to_string())?;
    }

    let manifest = PanelManifest {
        ini_source: ini_path.display().to_string(),
        panel_count: result.manifest.panel_count,
        panels: result.manifest.panels.clone(),
        ini_signature: Some(signature.to_string()),
        ini_hash: Some(hash.to_string()),
        generated_at_ms: Some(now_ms()),
        generator_version: Some(PANEL_CACHE_GENERATOR_VERSION),
    };

    let yaml = serde_yaml::to_string(&manifest).map_err(|e| e.to_string())?;
    fs::write(cache_dir.join("manifest.yaml"), yaml).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(cache_dir.join("manifest.json"), json).map_err(|e| e.to_string())?;
    Ok(())
}

/// Прочитать manifest.json из каталога cache.
pub fn read_manifest_from_dir(cache_dir: &Path) -> Result<PanelManifest, String> {
    let path = cache_dir.join("manifest.json");
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

pub fn read_panel_yaml(cache_dir: &Path, file: &str) -> Result<String, String> {
    let path = cache_dir.join(file);
    fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn ensure_panels_writes_cache_on_miss() {
        let _guard = test_lock();
        let temp = std::env::temp_dir().join(format!("rusefui_panel_cache_{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap();
        std::env::set_var("RUSEFI_UI_PANELS_DIR", &temp);

        let ini_path = rusefi_ini::default_test_ini_path();
        let ini = rusefi_ini::IniFile::load_file(&ini_path).unwrap();
        let sig = ini.signature.expect("test ini signature");
        let project_key = "test-project";

        let status = ensure_panels_for_ini(&ini_path, &sig, project_key).expect("ensure");
        assert!(status.generated);
        assert_eq!(status.project_key, project_key);
        assert!(PathBuf::from(&status.manifest_path).is_file());

        let manifest = read_manifest_from_dir(&cache_dir_for_project_ini(project_key, &status.hash))
            .expect("manifest");
        assert_eq!(
            manifest.generator_version,
            Some(PANEL_CACHE_GENERATOR_VERSION)
        );
        assert!(manifest.panel_count >= 200);

        let again = ensure_panels_for_ini(&ini_path, &sig, project_key).expect("hit");
        assert!(!again.generated);

        std::env::remove_var("RUSEFI_UI_PANELS_DIR");
        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn ensure_panels_regenerates_stale_cache_without_generator_version() {
        let _guard = test_lock();
        let temp = std::env::temp_dir().join(format!(
            "rusefui_panel_cache_stale_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        std::env::set_var("RUSEFI_UI_PANELS_DIR", &temp);

        let ini_path = rusefi_ini::default_test_ini_path();
        let ini = rusefi_ini::IniFile::load_file(&ini_path).unwrap();
        let sig = ini.signature.expect("test ini signature");
        let parsed = parse_rusefi_signature(&sig).expect("signature");
        let project_key = "stale-project";
        let cache_dir = cache_dir_for_project_ini(project_key, &parsed.hash);

        std::fs::create_dir_all(&cache_dir).unwrap();
        let stale = PanelManifest {
            ini_source: ini_path.display().to_string(),
            panel_count: 1,
            panels: vec![rusefi_ini::PanelManifestEntry {
                id: "staleOnly".into(),
                file: "stale.panel.yaml".into(),
                title: "stale".into(),
                menu_path: "stale".into(),
            }],
            ini_signature: Some(sig.clone()),
            ini_hash: Some(parsed.hash.clone()),
            generated_at_ms: Some(1),
            generator_version: None,
        };
        fs::write(
            cache_dir.join("manifest.json"),
            serde_json::to_string(&stale).unwrap(),
        )
        .unwrap();
        fs::write(cache_dir.join("stale.panel.yaml"), "children: []\n").unwrap();

        let status = ensure_panels_for_ini(&ini_path, &sig, project_key).expect("regenerate");
        assert!(status.generated);

        let manifest = read_manifest_from_dir(&cache_dir).expect("manifest");
        assert_eq!(
            manifest.generator_version,
            Some(PANEL_CACHE_GENERATOR_VERSION)
        );
        assert!(manifest.panel_count >= 200);
        assert!(
            manifest
                .panels
                .iter()
                .any(|p| p.id == "IgnitionTableDialog")
        );

        std::env::remove_var("RUSEFI_UI_PANELS_DIR");
        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn different_projects_get_separate_cache_dirs() {
        let _guard = test_lock();
        let temp = std::env::temp_dir().join(format!("rusefui_panel_cache_sep_{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap();
        std::env::set_var("RUSEFI_UI_PANELS_DIR", &temp);

        let ini_path = rusefi_ini::default_test_ini_path();
        let ini = rusefi_ini::IniFile::load_file(&ini_path).unwrap();
        let sig = ini.signature.expect("test ini signature");

        let a = ensure_panels_for_ini(&ini_path, &sig, "project-a").expect("a");
        let b = ensure_panels_for_ini(&ini_path, &sig, "project-b").expect("b");
        assert_ne!(a.dir, b.dir);

        std::env::remove_var("RUSEFI_UI_PANELS_DIR");
        let _ = std::fs::remove_dir_all(temp);
    }
}
