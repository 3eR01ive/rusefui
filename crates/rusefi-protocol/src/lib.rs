//! rusEFI TunerStudio binary protocol (msEnvelope_1.0).
//!
//! Reference: `rusefi/firmware/console/binary/tunerstudio*.cpp`,
//! `rusefi/java_console/io/.../IoHelper.java`.

mod commands;
mod composite;
mod crc;
mod error;
mod log_format;
mod packet;
mod port_discovery;
mod serial;
mod tracer;

pub use commands::{DEFAULT_IO_TIMEOUT_MS, *};
pub use composite::{parse_composite_records, CompositeParseState, CompositeRecord};
pub use error::ProtocolError;
pub use packet::{make_crc_request, parse_crc_response, CrcResponse};
pub use port_discovery::{
    describe_serial_port, is_port_busy, is_rusefi_candidate, is_rusefi_signature,
    is_rusefi_usb_match, is_serial_port_busy, list_serial_ports, map_serial_open_error,
    port_exists, probe_rusefi_signature, rusefi_port_candidates, rusefi_usb_fingerprints,
    try_open_serial_port, SerialPortEntry, RUSEFI_SIGNATURE_PREFIX, RUSEFI_USB_PID,
    RUSEFI_USB_VID,
};
pub use serial::{pack_config_read_request, pack_config_write_request, ConnectionInfo, SerialLink};
pub use tracer::ProtocolTracer;
pub use log_format::{
    command_char, describe_payload, describe_response, hex_preview, is_config_page_read,
    is_output_poll,
};
