//! Кэш YAML-панелей, сгенерированных из INI (`~/.rusEFI/ui_panels/{hash}/`).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusefi_ini::{convert_ini_path, ConvertResult, PanelManifest};
use serde::Serialize;

use super::signature::parse_rusefi_signature;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelCacheStatus {
    pub hash: String,
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

/// Каталог user-level cache для UI-панелей (не в проекте, не в bundle).
pub fn ui_panels_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RUSEFI_UI_PANELS_DIR") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .map(|home| home.join(".rusEFI").join("ui_panels"))
        .unwrap_or_else(|| PathBuf::from(".rusEFI/ui_panels"))
}

pub fn cache_dir_for_hash(hash: &str) -> PathBuf {
    ui_panels_cache_dir().join(hash)
}

pub fn manifest_path_for_hash(hash: &str) -> PathBuf {
    cache_dir_for_hash(hash).join("manifest.json")
}

/// Создать cache при miss; при hit — только вернуть путь.
pub fn ensure_panels_for_ini(ini_path: &Path, signature: &str) -> Result<PanelCacheStatus, String> {
    let parsed = parse_rusefi_signature(signature)
        .ok_or_else(|| format!("signature не парсится для panel cache: {signature}"))?;
    let hash = parsed.hash;
    let cache_dir = cache_dir_for_hash(&hash);
    let manifest_path = cache_dir.join("manifest.json");

    if manifest_path.is_file() {
        return Ok(PanelCacheStatus {
            hash,
            dir: cache_dir.display().to_string(),
            manifest_path: manifest_path.display().to_string(),
            generated: false,
        });
    }

    let result = convert_ini_path(ini_path).map_err(|e| e.to_string())?;
    write_panel_cache(&cache_dir, &result, signature, &hash, ini_path)?;

    Ok(PanelCacheStatus {
        hash,
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
    };

    let yaml = serde_yaml::to_string(&manifest).map_err(|e| e.to_string())?;
    fs::write(cache_dir.join("manifest.yaml"), yaml).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(cache_dir.join("manifest.json"), json).map_err(|e| e.to_string())?;
    Ok(())
}

/// Прочитать manifest.json из cache hash (если есть).
pub fn read_cached_manifest(hash: &str) -> Result<PanelManifest, String> {
    let path = manifest_path_for_hash(hash);
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// Прочитать YAML-панель из cache hash.
pub fn read_cached_panel_yaml(hash: &str, file: &str) -> Result<String, String> {
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return Err("недопустимое имя файла панели".into());
    }
    let path = cache_dir_for_hash(hash).join(file);
    fs::read_to_string(&path).map_err(|e| e.to_string())
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

        let status = ensure_panels_for_ini(&ini_path, &sig).expect("ensure");
        assert!(status.generated);
        assert!(PathBuf::from(&status.manifest_path).is_file());

        let again = ensure_panels_for_ini(&ini_path, &sig).expect("hit");
        assert!(!again.generated);

        std::env::remove_var("RUSEFI_UI_PANELS_DIR");
        let _ = std::fs::remove_dir_all(temp);
    }
}
