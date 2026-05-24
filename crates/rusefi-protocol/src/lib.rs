//! rusEFI TunerStudio binary protocol (msEnvelope_1.0).
//!
//! Reference: `rusefi/firmware/console/binary/tunerstudio*.cpp`,
//! `rusefi/java_console/io/.../IoHelper.java`.

mod commands;
mod crc;
mod error;
mod log_format;
mod packet;
mod serial;
mod tracer;

pub use commands::{DEFAULT_IO_TIMEOUT_MS, *};
pub use error::ProtocolError;
pub use packet::{make_crc_request, parse_crc_response, CrcResponse};
pub use serial::{pack_config_read_request, pack_config_write_request, ConnectionInfo, SerialLink};
pub use tracer::ProtocolTracer;
pub use log_format::{command_char, describe_payload, describe_response, hex_preview, is_output_poll};
