mod resolve;
mod signature;

pub use resolve::{
    ini_cache_dir, resolve_ini_for_signature, search_directories, signatures_match, IniResolveError,
    ResolvedIni,
};

/// Явный путь к INI (только если задан `RUSEFI_INI_PATH`).
pub fn explicit_ini_path() -> Option<std::path::PathBuf> {
    std::env::var("RUSEFI_INI_PATH")
        .ok()
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
}
