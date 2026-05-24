use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("serial port: {0}")]
    Serial(#[from] serialport::Error),

    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),

    #[error("response timeout after {0} ms")]
    Timeout(u64),

    #[error("invalid packet: {0}")]
    InvalidPacket(String),

    #[error("CRC mismatch: expected 0x{expected:08X}, got 0x{actual:08X}")]
    CrcMismatch { expected: u32, actual: u32 },

    #[error("ECU error response 0x{0:02X}")]
    ErrorResponse(u8),

    #[error("empty signature")]
    EmptySignature,
}
