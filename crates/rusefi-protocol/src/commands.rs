/// Query / hello (firmware accepts `Q` during port scan and `S` in INI).
pub const TS_QUERY_COMMAND: u8 = b'Q';
pub const TS_HELLO_COMMAND: u8 = b'S';

pub const TS_OUTPUT_COMMAND: u8 = b'O';
pub const TS_READ_COMMAND: u8 = b'R';
pub const TS_CHUNK_WRITE_COMMAND: u8 = b'C';
pub const TS_BURN_COMMAND: u8 = b'B';
pub const TS_CRC_CHECK_COMMAND: u8 = b'k';

pub const TS_RESPONSE_OK: u8 = 0;
pub const TS_RESPONSE_UNDERRUN: u8 = 0x80;
pub const TS_RESPONSE_OVERRUN: u8 = 0x81;
pub const TS_RESPONSE_CRC_FAILURE: u8 = 0x82;
pub const TS_RESPONSE_UNRECOGNIZED_COMMAND: u8 = 0x83;
pub const TS_RESPONSE_OUT_OF_RANGE: u8 = 0x84;
pub const TS_RESPONSE_BURN_OK: u8 = 4;

/// Default timeout aligned with rusEFI `BINARY_IO_TIMEOUT` (~1 s).
pub const DEFAULT_IO_TIMEOUT_MS: u64 = 1000;
