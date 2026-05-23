//! rusEFI TunerStudio binary protocol (msEnvelope_1.0).
//!
//! Reference: `rusefi/firmware/console/binary/tunerstudio*.cpp`,
//! `rusefi/java_console/io/.../IoHelper.java`.

mod commands;
mod crc;
mod error;
mod packet;
mod serial;

pub use commands::{DEFAULT_IO_TIMEOUT_MS, *};
pub use error::ProtocolError;
pub use packet::{make_crc_request, parse_crc_response, CrcResponse};
pub use serial::{ConnectionInfo, SerialLink};
