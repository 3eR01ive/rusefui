mod panel_cache;
mod resolve;
mod signature;

pub use panel_cache::{
    cache_dir_for_project_ini, ensure_panels_for_ini, panels_root_for_project,
    read_manifest_from_dir, read_panel_yaml, PanelCacheStatus,
};
pub use resolve::{
    download_ini_for_signature, enumerate_local_candidates, find_any_local_ini, ini_cache_dir,
    install_ini_to_cache, load_ini_path, resolve_ini_for_signature, search_directories,
    signatures_match, IniCandidate, IniCandidateSource, IniResolveError, OnlineDownloadStatus,
    ResolvedIni,
};
pub use signature::{ini_download_target, parse_rusefi_signature, RusEfiSignature};

/// Явный путь к INI (только если задан `RUSEFI_INI_PATH`).
pub fn explicit_ini_path() -> Option<std::path::PathBuf> {
    std::env::var("RUSEFI_INI_PATH")
        .ok()
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
}
