//! Парсер подмножества TunerStudio INI для источников данных rusefui.

mod convert_panel;
mod decode;
mod defines;
mod enum_options;
mod error;
mod menu;
mod model;
mod parse;
mod tables;

pub use convert_panel::{convert_menu_panels, ConvertResult, PanelManifest, PanelManifestEntry};
pub use decode::{
    decode_array, decode_config_at, decode_config_fields, decode_config_scalars, decode_output_channels,
    decode_scalar_at, encode_array_element, encode_bits_value, encode_config_value, encode_scalar_value,
};
pub use error::IniError;
pub use menu::{parse_menu_section, IniMenu};
pub use model::{
    ArrayField, ArrayShape, ConfigFieldKind, EnumField, EnumOption, FieldKind, IniCurveDef,
    IniFile, IniTableDef, OutputChannelField, OutputChannels, ScalarField, ScalarType,
};
pub use parse::{parse_ini, split_ini_args};

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
