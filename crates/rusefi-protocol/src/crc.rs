/// CRC-32 compatible with Java `java.util.zip.CRC32` / firmware `crc32()`.
pub fn crc32(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}
