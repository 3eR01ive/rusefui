use rusefi_ini::{default_test_ini_path, IniFile};

/// Путь к INI: `RUSEFI_INI_PATH` или `test_data/rusefi_proteus_f7.ini` в репозитории.
pub fn resolve_ini_path() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("RUSEFI_INI_PATH") {
        return std::path::PathBuf::from(path);
    }
    default_test_ini_path()
}

pub fn load_ini() -> Result<IniFile, String> {
    let path = resolve_ini_path();
    IniFile::load_file(&path).map_err(|e| format!("{path:?}: {e}"))
}
