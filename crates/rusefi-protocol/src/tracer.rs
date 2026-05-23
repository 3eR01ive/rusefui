use crate::error::ProtocolError;
use crate::packet::CrcResponse;

/// Трассировка TX/RX для UI и файлового лога.
pub trait ProtocolTracer: Send + Sync {
    fn on_tx(&self, payload: &[u8], frame: &[u8]);
    fn on_rx_ok(&self, request_payload: &[u8], frame: &[u8], response: &CrcResponse);
    fn on_rx_err(&self, request_payload: &[u8], error: &ProtocolError);
    fn on_info(&self, message: &str);
}
