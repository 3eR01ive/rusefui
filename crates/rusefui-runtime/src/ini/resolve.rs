use std::path::{Path, PathBuf};

use rusefi_ini::IniFile;
use serde::Serialize;

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

/// Откуда найден INI-кандидат — для подсказки в UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IniCandidateSource {
    /// `RUSEFI_INI_PATH` / `RUSEFI_EXTRA_INI_PATH`.
    EnvOverride,
    /// `~/.rusEFI/ini_database/` (предыдущие загрузки c rusefi.com).
    Cache,
    /// Каталог, явно переданный через env (`RUSEFI_INI_DIR` / `RUSEFI_GENERATED_INI_DIR`)
    /// либо встроенный `test_data/` / соседний `rusefi/firmware/.../generated/`.
    LocalDir,
}

/// Описание найденного INI-кандидата для UI выбора.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IniCandidate {
    pub path: String,
    pub file_name: String,
    pub source: IniCandidateSource,
    pub signature: Option<String>,
    pub matches_ecu: bool,
    /// `bundle_target` из signature (e.g. `proteus_f7`) — для подсветки несовпадения железа.
    pub bundle_target: Option<String>,
    pub size_bytes: u64,
}

/// Статус online-загрузки c rusefi.com (для последнего отображения в UI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OnlineDownloadStatus {
    /// Signature ECU не парсится в `rusEFI {branch}.{...}` — URL построить нельзя.
    NotApplicable,
    /// Загрузка не пробовалась (например, `RUSEFI_INI_NO_DOWNLOAD=1`).
    NotAttempted { reason: String },
    /// Успешно скачано в указанный кэш-файл.
    Succeeded { path: String, url: String },
    /// HTTP/IO ошибка или signature файла не совпала с ECU.
    Failed { url: String, error: String },
}

impl OnlineDownloadStatus {
    pub fn is_success(&self) -> bool {
        matches!(self, OnlineDownloadStatus::Succeeded { .. })
    }
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

/// Загрузить INI с диска без проверки signature ECU (offline / настройка UI).
pub fn load_ini_path(path: &Path) -> Result<ResolvedIni, IniResolveError> {
    let file = IniFile::load_file(path).map_err(|e| IniResolveError::LoadFailed {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    Ok(ResolvedIni {
        path: path.to_path_buf(),
        file,
    })
}

/// Локальный INI для offline: `RUSEFI_INI_PATH`, кэш, `test_data`, generated.
pub fn find_any_local_ini() -> Option<ResolvedIni> {
    if let Some(path) = super::explicit_ini_path() {
        if let Ok(resolved) = load_ini_path(&path) {
            if !resolved.file.output_channels.fields.is_empty() {
                return Some(resolved);
            }
        }
    }

    let mut best: Option<(usize, ResolvedIni)> = None;
    for dir in search_directories() {
        let entries = std::fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name()?.to_str()?;
            if !name.starts_with("rusefi_") || !name.ends_with(".ini") {
                continue;
            }
            if path.metadata().ok().map(|m| m.len()).unwrap_or(0) < MIN_INI_BYTES {
                continue;
            }
            let Ok(resolved) = load_ini_path(&path) else {
                continue;
            };
            let n = resolved.file.output_channels.fields.len();
            if n == 0 {
                continue;
            }
            if best.as_ref().is_none_or(|(best_n, _)| n > *best_n) {
                best = Some((n, resolved));
            }
        }
    }
    best.map(|(_, r)| r)
}

fn load_and_verify(path: &Path, ecu_signature: &str) -> Result<ResolvedIni, IniResolveError> {
    let resolved = load_ini_path(path)?;
    let file = &resolved.file;
    match file.signature.as_deref() {
        Some(ini_sig) if ini_sig == ecu_signature => Ok(resolved),
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
    match download_ini_for_signature(ecu_signature) {
        OnlineDownloadStatus::Succeeded { path, .. } => Some(PathBuf::from(path)),
        _ => None,
    }
}

/// Принудительная попытка скачать INI с rusefi.com и сохранить в `ini_cache_dir`.
/// Возвращает детальный статус для отображения в UI; перед записью проверяет,
/// что signature файла совпадает с `ecu_signature`.
pub fn download_ini_for_signature(ecu_signature: &str) -> OnlineDownloadStatus {
    if let Ok(reason) = std::env::var("RUSEFI_INI_NO_DOWNLOAD") {
        let reason = if reason.is_empty() {
            "RUSEFI_INI_NO_DOWNLOAD=1".to_string()
        } else {
            format!("RUSEFI_INI_NO_DOWNLOAD={reason}")
        };
        return OnlineDownloadStatus::NotAttempted { reason };
    }

    let Some((url, file_name)) = ini_download_target(ecu_signature) else {
        return OnlineDownloadStatus::NotApplicable;
    };

    let dest = ini_cache_dir().join(&file_name);
    if let Err(e) = std::fs::create_dir_all(ini_cache_dir()) {
        return OnlineDownloadStatus::Failed {
            url,
            error: format!("не создать каталог кэша: {e}"),
        };
    }

    let response = match ureq::get(&url).call() {
        Ok(r) => r,
        Err(e) => {
            return OnlineDownloadStatus::Failed {
                url,
                error: format!("HTTP: {e}"),
            }
        }
    };
    if response.status() >= 300 {
        return OnlineDownloadStatus::Failed {
            url,
            error: format!("HTTP {}", response.status()),
        };
    }
    let mut bytes = Vec::new();
    if let Err(e) = std::io::Read::read_to_end(&mut response.into_reader(), &mut bytes) {
        return OnlineDownloadStatus::Failed {
            url,
            error: format!("read body: {e}"),
        };
    }
    if (bytes.len() as u64) <= MIN_INI_BYTES {
        return OnlineDownloadStatus::Failed {
            url,
            error: format!("слишком короткий ответ ({} байт)", bytes.len()),
        };
    }
    if let Err(e) = std::fs::write(&dest, &bytes) {
        return OnlineDownloadStatus::Failed {
            url,
            error: format!("write {dest}: {e}", dest = dest.display()),
        };
    }

    // Дополнительно убеждаемся, что скачанный файл действительно содержит правильную signature.
    match IniFile::load_file(&dest) {
        Ok(file) => {
            if file.signature.as_deref() == Some(ecu_signature) {
                OnlineDownloadStatus::Succeeded {
                    path: dest.display().to_string(),
                    url,
                }
            } else {
                let actual = file.signature.unwrap_or_default();
                OnlineDownloadStatus::Failed {
                    url,
                    error: format!(
                        "signature в скачанном файле не совпадает: ECU={ecu_signature}, INI={actual}"
                    ),
                }
            }
        }
        Err(e) => OnlineDownloadStatus::Failed {
            url,
            error: format!("ошибка парсинга: {e}"),
        },
    }
}

/// Собрать список локальных INI-кандидатов с пометкой совпадения с ECU signature.
/// Кандидаты сортируются: сначала совпадающие, затем по убыванию размера.
/// Если `ecu_signature` не задана, `matches_ecu` всегда `false`.
pub fn enumerate_local_candidates(ecu_signature: Option<&str>) -> Vec<IniCandidate> {
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut out: Vec<IniCandidate> = Vec::new();

    let push_candidate = |path: PathBuf, source: IniCandidateSource, out: &mut Vec<IniCandidate>, seen: &mut std::collections::HashSet<PathBuf>| {
        let canonical = path.canonicalize().unwrap_or(path);
        if !canonical.is_file() {
            return;
        }
        if !seen.insert(canonical.clone()) {
            return;
        }
        let size = canonical.metadata().map(|m| m.len()).unwrap_or(0);
        if size < MIN_INI_BYTES {
            return;
        }
        let (signature, bundle_target) = match IniFile::load_file(&canonical) {
            Ok(file) => {
                let bundle_target = file
                    .signature
                    .as_deref()
                    .and_then(parse_rusefi_signature)
                    .map(|s| s.bundle_target);
                (file.signature, bundle_target)
            }
            Err(_) => (None, None),
        };
        let matches_ecu = match (ecu_signature.filter(|s| !s.is_empty()), signature.as_deref()) {
            (Some(ecu), Some(ini)) => ecu == ini,
            _ => false,
        };
        let file_name = canonical
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        out.push(IniCandidate {
            path: canonical.display().to_string(),
            file_name,
            source,
            signature,
            matches_ecu,
            bundle_target,
            size_bytes: size,
        });
    };

    if let Ok(p) = std::env::var("RUSEFI_INI_PATH") {
        push_candidate(PathBuf::from(p), IniCandidateSource::EnvOverride, &mut out, &mut seen);
    }
    if let Ok(p) = std::env::var("RUSEFI_EXTRA_INI_PATH") {
        push_candidate(PathBuf::from(p), IniCandidateSource::EnvOverride, &mut out, &mut seen);
    }

    let cache_dir = ini_cache_dir();
    let mut env_dirs: Vec<PathBuf> = Vec::new();
    if let Ok(d) = std::env::var("RUSEFI_INI_DIR") {
        env_dirs.push(PathBuf::from(d));
    }
    if let Ok(d) = std::env::var("RUSEFI_GENERATED_INI_DIR") {
        env_dirs.push(PathBuf::from(d));
    }

    if let Some(entries) = std::fs::read_dir(&cache_dir).ok() {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("ini") {
                push_candidate(path, IniCandidateSource::Cache, &mut out, &mut seen);
            }
        }
    }

    for dir in search_directories() {
        let src = if env_dirs.iter().any(|d| d == &dir) {
            IniCandidateSource::EnvOverride
        } else if dir == cache_dir {
            IniCandidateSource::Cache
        } else {
            IniCandidateSource::LocalDir
        };
        let Some(entries) = std::fs::read_dir(&dir).ok() else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = match path.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if !name.ends_with(".ini") {
                continue;
            }
            push_candidate(path, src, &mut out, &mut seen);
        }
    }

    out.sort_by(|a, b| {
        b.matches_ecu
            .cmp(&a.matches_ecu)
            .then_with(|| b.size_bytes.cmp(&a.size_bytes))
            .then_with(|| a.file_name.cmp(&b.file_name))
    });
    out
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

/// Скопировать INI в `ini_cache_dir`, если он ещё не там.
/// Имя — `{hash}.ini` по rusEFI signature (как online-загрузка / Java Console).
pub fn install_ini_to_cache(source: &Path, file: &IniFile) -> Result<PathBuf, IniResolveError> {
    let cache_dir = ini_cache_dir();
    std::fs::create_dir_all(&cache_dir).map_err(|e| IniResolveError::LoadFailed {
        path: cache_dir.clone(),
        message: format!("не создать каталог кэша: {e}"),
    })?;

    let source_canon = source.canonicalize().unwrap_or_else(|_| source.to_path_buf());
    let cache_canon = cache_dir.canonicalize().unwrap_or(cache_dir);

    if source_canon.starts_with(&cache_canon) {
        return Ok(source_canon);
    }

    let dest_name = cache_file_name_for_ini(file, source)?;
    let dest = cache_canon.join(&dest_name);
    if dest == source_canon {
        return Ok(dest);
    }

    std::fs::copy(&source_canon, &dest).map_err(|e| IniResolveError::LoadFailed {
        path: dest.clone(),
        message: format!("copy from {}: {e}", source_canon.display()),
    })?;

    Ok(dest)
}

fn cache_file_name_for_ini(file: &IniFile, source: &Path) -> Result<String, IniResolveError> {
    if let Some(sig) = file.signature.as_deref() {
        if let Some(parsed) = parse_rusefi_signature(sig) {
            return Ok(format!("{}.ini", parsed.hash));
        }
    }
    source
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|n| n.ends_with(".ini") && !n.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| IniResolveError::LoadFailed {
            path: source.to_path_buf(),
            message: "нет rusEFI signature и некорректное имя файла для кэша".into(),
        })
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

    #[test]
    fn enumerate_candidates_marks_matching_proteus() {
        std::env::set_var("RUSEFI_INI_NO_DOWNLOAD", "1");
        std::env::remove_var("RUSEFI_INI_PATH");
        std::env::remove_var("RUSEFI_EXTRA_INI_PATH");
        let sig = "rusEFI master.2025.09.02.proteus_f7.4139280449";
        let cands = enumerate_local_candidates(Some(sig));
        assert!(!cands.is_empty(), "должен быть хотя бы один кандидат");
        let first_match = cands.iter().find(|c| c.matches_ecu);
        assert!(
            first_match.is_some(),
            "ожидаем совпадение с {sig}, найдены: {:?}",
            cands.iter().map(|c| &c.signature).collect::<Vec<_>>()
        );
        // matching кандидаты должны быть в начале (сортировка по `matches_ecu desc`).
        assert!(cands[0].matches_ecu, "первый кандидат должен быть match");
    }

    #[test]
    fn enumerate_candidates_without_signature_returns_all() {
        std::env::set_var("RUSEFI_INI_NO_DOWNLOAD", "1");
        let cands = enumerate_local_candidates(None);
        // Файлы найдены, но никто не помечен как matching.
        assert!(!cands.is_empty());
        assert!(!cands.iter().any(|c| c.matches_ecu));
    }

    #[test]
    fn online_status_serializes_with_tag() {
        let ok = OnlineDownloadStatus::Succeeded {
            path: "/x.ini".into(),
            url: "https://example".into(),
        };
        let json = serde_json::to_string(&ok).unwrap();
        assert!(json.contains("\"kind\":\"succeeded\""), "json: {json}");
        assert!(ok.is_success());

        let fail = OnlineDownloadStatus::Failed {
            url: "https://example".into(),
            error: "boom".into(),
        };
        assert!(!fail.is_success());
    }

    #[test]
    fn install_ini_to_cache_copies_external_file_by_hash() {
        let source_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_data")
            .canonicalize()
            .expect("test_data dir");
        let source = source_dir.join("rusefi_proteus_f7.ini");
        let cache_root = std::env::temp_dir().join(format!(
            "rusefui-ini-cache-test-{}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&cache_root).ok();
        std::env::set_var("RUSEFI_INI_CACHE_DIR", cache_root.display().to_string());

        let file = IniFile::load_file(&source).expect("load proteus ini");
        let sig = file.signature.as_deref().expect("signature in test ini");
        let hash = parse_rusefi_signature(sig).expect("parse signature").hash;
        let cached = install_ini_to_cache(&source, &file).expect("install to cache");
        assert_eq!(
            cached,
            cache_root.canonicalize().unwrap_or(cache_root.clone()).join(format!("{hash}.ini"))
        );
        assert!(cached.is_file());
        let cached_ini = IniFile::load_file(&cached).unwrap();
        assert_eq!(cached_ini.signature.as_deref(), Some(sig));

        // Повторный вызов для файла уже в кэше — без лишней копии.
        let again = install_ini_to_cache(&cached, &cached_ini).expect("already cached");
        assert_eq!(again, cached.canonicalize().unwrap_or(cached));

        std::env::remove_var("RUSEFI_INI_CACHE_DIR");
        std::fs::remove_dir_all(&cache_root).ok();
    }
}
