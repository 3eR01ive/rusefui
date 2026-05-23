use std::path::{Path, PathBuf};

use rusefi_ini::IniFile;

use super::signature::{ini_download_target, parse_rusefi_signature};

const MIN_INI_BYTES: u64 = 10_000;

#[derive(Debug, thiserror::Error)]
pub enum IniResolveError {
    #[error("некорректная signature ECU: {0}")]
    InvalidSignature(String),

    #[error("INI не найден для signature ECU: {0}")]
    NotFound(String),

    #[error("signature ECU не совпадает с INI: ECU={ecu}, INI={ini}")]
    SignatureMismatch { ecu: String, ini: String },

    #[error("INI без поля signature: {path}")]
    MissingIniSignature { path: PathBuf },

    #[error("{path}: {message}")]
    LoadFailed { path: PathBuf, message: String },
}

pub struct ResolvedIni {
    pub path: PathBuf,
    pub file: IniFile,
}

/// Строгое совпадение signature ECU и INI.
pub fn signatures_match(ecu_signature: &str, ini_signature: Option<&str>) -> bool {
    ini_signature == Some(ecu_signature)
}

/// Найти и загрузить INI для signature ECU; signature должны совпасть точно.
pub fn resolve_ini_for_signature(ecu_signature: &str) -> Result<ResolvedIni, IniResolveError> {
    if parse_rusefi_signature(ecu_signature).is_none() {
        return Err(IniResolveError::InvalidSignature(ecu_signature.to_string()));
    }

    if let Ok(path) = std::env::var("RUSEFI_INI_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return load_and_verify(&path, ecu_signature);
        }
    }

    if let Some(parsed) = parse_rusefi_signature(ecu_signature) {
        let cache_path = ini_cache_dir().join(format!("{}.ini", parsed.hash));
        if cache_path.is_file() && cache_path.metadata().is_ok_and(|m| m.len() > MIN_INI_BYTES) {
            if let Ok(resolved) = load_and_verify(&cache_path, ecu_signature) {
                return Ok(resolved);
            }
        }
    }

    if let Some(path) = try_download(ecu_signature) {
        if let Ok(resolved) = load_and_verify(&path, ecu_signature) {
            return Ok(resolved);
        }
    }

    for dir in search_directories() {
        if let Some(path) = scan_directory_for_signature(&dir, ecu_signature) {
            return load_and_verify(&path, ecu_signature);
        }
    }

    Err(IniResolveError::NotFound(ecu_signature.to_string()))
}

fn load_and_verify(path: &Path, ecu_signature: &str) -> Result<ResolvedIni, IniResolveError> {
    let file = IniFile::load_file(path).map_err(|e| IniResolveError::LoadFailed {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    match file.signature.as_deref() {
        Some(ini_sig) if ini_sig == ecu_signature => Ok(ResolvedIni {
            path: path.to_path_buf(),
            file,
        }),
        Some(ini_sig) => Err(IniResolveError::SignatureMismatch {
            ecu: ecu_signature.to_string(),
            ini: ini_sig.to_string(),
        }),
        None => Err(IniResolveError::MissingIniSignature {
            path: path.to_path_buf(),
        }),
    }
}

fn try_download(ecu_signature: &str) -> Option<PathBuf> {
    if std::env::var("RUSEFI_INI_NO_DOWNLOAD").is_ok() {
        return None;
    }
    if let Ok(extra) = std::env::var("RUSEFI_EXTRA_INI_PATH") {
        let path = PathBuf::from(extra);
        if path.is_file() {
            return Some(path);
        }
    }

    let (url, file_name) = ini_download_target(ecu_signature)?;
    let dest = ini_cache_dir().join(&file_name);
    std::fs::create_dir_all(ini_cache_dir()).ok()?;

    let response = ureq::get(&url).call().ok()?;
    if response.status() >= 300 {
        return None;
    }
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut bytes).ok()?;
    if bytes.len() as u64 <= MIN_INI_BYTES {
        return None;
    }
    std::fs::write(&dest, bytes).ok()?;
    Some(dest)
}

fn scan_directory_for_signature(dir: &Path, ecu_signature: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut matches = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name()?.to_str()?;
        if !name.starts_with("rusefi_") || !name.ends_with(".ini") {
            continue;
        }
        let ini = IniFile::load_file(&path).ok()?;
        if signatures_match(ecu_signature, ini.signature.as_deref()) {
            matches.push(path);
        }
    }
    matches.sort();
    matches.into_iter().next()
}

pub fn ini_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RUSEFI_INI_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .map(|home| home.join(".rusEFI").join("ini_database"))
        .unwrap_or_else(|| PathBuf::from(".rusEFI/ini_database"))
}

pub fn search_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(dir) = std::env::var("RUSEFI_INI_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Ok(dir) = std::env::var("RUSEFI_GENERATED_INI_DIR") {
        dirs.push(PathBuf::from(dir));
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dirs.push(manifest.join("../../test_data"));
    dirs.push(
        manifest.join("../../../rusefi/firmware/tunerstudio/generated"),
    );
    dirs.push(ini_cache_dir());

    dirs.into_iter()
        .filter_map(|dir| dir.canonicalize().ok())
        .filter(|dir| dir.is_dir())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn scan_test_data_directly() {
        let sig = "rusEFI master.2025.09.02.proteus_f7.4139280449";
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_data")
            .canonicalize()
            .expect("test_data dir");
        let ini_path = dir.join("rusefi_proteus_f7.ini");
        let ini = IniFile::load_file(&ini_path).expect("load proteus ini");
        assert_eq!(ini.signature.as_deref(), Some(sig));
        assert!(scan_directory_for_signature(&dir, sig).is_some());
    }

    #[test]
    fn resolve_proteus_from_test_data_or_generated() {
        let sig = "rusEFI master.2025.09.02.proteus_f7.4139280449";
        std::env::set_var("RUSEFI_INI_NO_DOWNLOAD", "1");
        std::env::remove_var("RUSEFI_INI_PATH");
        let dirs = search_directories();
        assert!(!dirs.is_empty(), "search dirs empty: {dirs:?}");
        let resolved = resolve_ini_for_signature(sig).expect("proteus ini should be found locally");
        assert_eq!(resolved.file.signature.as_deref(), Some(sig));
    }
}
