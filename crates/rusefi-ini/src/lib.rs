//! Парсер подмножества TunerStudio INI для источников данных rusefui.

mod decode;
mod error;
mod model;
mod parse;

pub use decode::decode_output_channels;
pub use error::IniError;
pub use model::{IniFile, OutputChannelField, OutputChannels, ScalarField, ScalarType};
pub use parse::parse_ini;

/// Путь к тестовому INI в репозитории (`test_data/rusefi_proteus_f7.ini`).
pub fn default_test_ini_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_data/rusefi_proteus_f7.ini")
}

impl IniFile {
    pub fn load_test_proteus() -> Result<Self, IniError> {
        Self::load_file(default_test_ini_path())
    }

    pub fn load_file(path: impl AsRef<std::path::Path>) -> Result<Self, IniError> {
        let text = std::fs::read_to_string(path.as_ref()).map_err(|e| IniError::Io {
            path: path.as_ref().display().to_string(),
            source: e,
        })?;
        parse_ini(&text)
    }
}
