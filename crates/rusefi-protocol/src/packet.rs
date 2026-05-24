use crate::commands::TS_RESPONSE_OK;
use crate::crc::crc32;
use crate::error::ProtocolError;

/// Build an outgoing CRC envelope: `[u16_be size][payload][u32_be crc]`.
pub fn make_crc_request(payload: &[u8]) -> Vec<u8> {
    assert!(
        payload.len() <= u16::MAX as usize,
        "payload too large for TS envelope"
    );

    let mut packet = vec![0u8; payload.len() + 6];
    let len = payload.len() as u16;
    packet[0] = (len >> 8) as u8;
    packet[1] = (len & 0xFF) as u8;
    packet[2..2 + payload.len()].copy_from_slice(payload);

    let crc = crc32(payload);
    packet[payload.len() + 2] = (crc >> 24) as u8;
    packet[payload.len() + 3] = (crc >> 16) as u8;
    packet[payload.len() + 4] = (crc >> 8) as u8;
    packet[payload.len() + 5] = crc as u8;

    packet
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrcResponse {
    pub code: u8,
    pub payload: Vec<u8>,
}

impl CrcResponse {
    pub fn into_string_payload(self) -> Result<String, ProtocolError> {
        if self.code != TS_RESPONSE_OK {
            return Err(ProtocolError::ErrorResponse(self.code));
        }
        let s = String::from_utf8_lossy(&self.payload).trim_end_matches('\0').to_string();
        if s.is_empty() {
            return Err(ProtocolError::EmptySignature);
        }
        Ok(s)
    }
}

/// Parse a complete incoming CRC frame (header + body + tail already in `data`).
pub fn parse_crc_response(data: &[u8]) -> Result<CrcResponse, ProtocolError> {
    if data.len() < 6 {
        return Err(ProtocolError::InvalidPacket("too short".into()));
    }

    let body_len = u16::from_be_bytes([data[0], data[1]]) as usize;
    let expected_total = 2 + body_len + 4;
    if data.len() != expected_total {
        return Err(ProtocolError::InvalidPacket("length mismatch".into()));
    }

    let body = &data[2..2 + body_len];
    let expected_crc = u32::from_be_bytes([
        data[2 + body_len],
        data[2 + body_len + 1],
        data[2 + body_len + 2],
        data[2 + body_len + 3],
    ]);
    let actual_crc = crc32(body);
    if actual_crc != expected_crc {
        return Err(ProtocolError::CrcMismatch {
            expected: expected_crc,
            actual: actual_crc,
        });
    }

    let code = *body
        .first()
        .ok_or_else(|| ProtocolError::InvalidPacket("empty body".into()))?;
    let payload = body.get(1..).unwrap_or(&[]).to_vec();

    Ok(CrcResponse { code, payload })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{TS_HELLO_COMMAND, TS_RESPONSE_OK};

    #[test]
    fn roundtrip_hello_payload() {
        let req = make_crc_request(&[TS_HELLO_COMMAND]);
        assert_eq!(req.len(), 7);
        assert_eq!(&req[0..2], &[0, 1]);
        assert_eq!(req[2], TS_HELLO_COMMAND);

        // Simulate ECU response: OK + signature bytes
        let sig = b"rusEFI test signature\0";
        let mut body = vec![TS_RESPONSE_OK];
        body.extend_from_slice(sig);
        let mut frame = Vec::new();
        let len = body.len() as u16;
        frame.push((len >> 8) as u8);
        frame.push((len & 0xFF) as u8);
        frame.extend_from_slice(&body);
        let c = crc32(&body);
        frame.extend_from_slice(&c.to_be_bytes());

        let parsed = parse_crc_response(&frame).unwrap();
        assert_eq!(parsed.code, TS_RESPONSE_OK);
        let s = parsed.into_string_payload().unwrap();
        assert_eq!(s, "rusEFI test signature");
    }

    #[test]
    fn parse_large_config_read_response_like_firmware() {
        let data = vec![0xABu8; 1024];
        let mut body = vec![TS_RESPONSE_OK];
        body.extend_from_slice(&data);
        let len = body.len() as u16;
        let mut frame = vec![(len >> 8) as u8, (len & 0xFF) as u8];
        frame.extend_from_slice(&body);
        let crc = crate::crc::crc32(&body);
        frame.extend_from_slice(&crc.to_be_bytes());

        let parsed = parse_crc_response(&frame).unwrap();
        assert_eq!(parsed.code, TS_RESPONSE_OK);
        assert_eq!(parsed.payload.len(), 1024);
        assert_eq!(parsed.payload, data);
    }
}
