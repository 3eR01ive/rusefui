/// Hello / query (INI `queryCommand`, CRC envelope).
pub const TS_HELLO_COMMAND: u8 = b'S';

pub const TS_OUTPUT_COMMAND: u8 = b'O';
pub const TS_READ_COMMAND: u8 = b'R';
pub const TS_CHUNK_WRITE_COMMAND: u8 = b'C';
pub const TS_BURN_COMMAND: u8 = b'B';
pub const TS_CRC_CHECK_COMMAND: u8 = b'k';
pub const TS_IO_TEST_COMMAND: u8 = b'Z';
/// Консольная команда (`TS_EXECUTE`), как Java `BinaryProtocol.sendTextCommand`.
pub const TS_EXECUTE_COMMAND: u8 = b'E';

/// Страница настроек (`TS_PAGE_SETTINGS`).
pub const TS_PAGE_SETTINGS: u16 = 0;

/// Подсистема `TS_X14` для bench/ETB/stimulator команд (`executeTSCommand`).
pub const TS_SUBSYSTEM_X14: u16 = 20;

pub const TS_X14_TRIGGER_STIMULATOR_ENABLE: u16 = 0x0D;
pub const TS_X14_TRIGGER_STIMULATOR_DISABLE: u16 = 0x0F;

pub const TS_RESPONSE_OK: u8 = 0;
pub const TS_RESPONSE_UNDERRUN: u8 = 0x80;
pub const TS_RESPONSE_OVERRUN: u8 = 0x81;
pub const TS_RESPONSE_CRC_FAILURE: u8 = 0x82;
pub const TS_RESPONSE_UNRECOGNIZED_COMMAND: u8 = 0x83;
pub const TS_RESPONSE_OUT_OF_RANGE: u8 = 0x84;
pub const TS_RESPONSE_BURN_OK: u8 = 4;

/// Default timeout aligned with rusEFI `BINARY_IO_TIMEOUT` (~1 s).
pub const DEFAULT_IO_TIMEOUT_MS: u64 = 1000;
