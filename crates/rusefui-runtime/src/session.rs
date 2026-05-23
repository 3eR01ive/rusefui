use std::sync::{Arc, Mutex};

use rusefi_protocol::{ConnectionInfo, ProtocolError, SerialLink, DEFAULT_IO_TIMEOUT_MS};

use crate::sources::output_channels::OutputChannelsSource;

struct EcuSessionInner {
    link: Option<SerialLink>,
}

/// Общая сессия ECU: serial link + фоновый опрос output channels.
pub struct EcuSession {
    inner: Mutex<EcuSessionInner>,
    output: OutputChannelsSource,
}

impl EcuSession {
    pub fn new_arc() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(EcuSessionInner { link: None }),
            output: OutputChannelsSource::new(),
        })
    }

    pub fn output(&self) -> &OutputChannelsSource {
        &self.output
    }

    pub fn is_connected(&self) -> bool {
        self.inner.lock().unwrap().link.is_some()
    }

    pub fn connect(&self, port: &str, baud_rate: u32) -> Result<ConnectionInfo, String> {
        self.output.stop();
        let link = SerialLink::connect(port, baud_rate, DEFAULT_IO_TIMEOUT_MS)
            .map_err(|e| e.to_string())?;
        let info = link.info().clone();
        self.inner.lock().unwrap().link = Some(link);
        Ok(info)
    }

    pub fn disconnect(&self) {
        self.output.stop();
        self.inner.lock().unwrap().link = None;
    }

    pub fn with_link<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&mut SerialLink) -> Result<R, ProtocolError>,
    {
        let mut guard = self.inner.lock().unwrap();
        let link = guard
            .link
            .as_mut()
            .ok_or_else(|| "ECU не подключена".to_string())?;
        f(link).map_err(|e| e.to_string())
    }
}
