use thiserror::Error;

#[derive(Debug, Error)]
pub enum IniError {
    #[error("IO {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },

    #[error("parse: {0}")]
    Parse(String),
}
