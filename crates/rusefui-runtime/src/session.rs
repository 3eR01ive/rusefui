use std::sync::{Arc, Mutex};

use rusefi_protocol::{ConnectionInfo, ProtocolError, SerialLink, DEFAULT_IO_TIMEOUT_MS};

use crate::ini::load_ini;
use crate::sources::output_channels::{IniContext, OutputChannelsSource};

struct EcuSessionInner {
    link: Option<SerialLink>,
}

/// Общая сессия ECU: serial link + фоновый опрос output channels.
pub struct EcuSession {
    inner: Mutex<EcuSessionInner>,
    output: OutputChannelsSource,
    ini: IniContext,
}

impl EcuSession {
    pub fn new_arc() -> Arc<Self> {
        let ini_file = load_ini().unwrap_or_else(|e| {
            panic!("failed to load ECU INI (set RUSEFI_INI_PATH or add test_data/rusefi_proteus_f7.ini): {e}");
        });
        let ini_ctx = IniContext::from_ini(&ini_file);
        Arc::new(Self {
            inner: Mutex::new(EcuSessionInner { link: None }),
            output: OutputChannelsSource::new(ini_ctx.clone()),
            ini: ini_ctx,
        })
    }

    pub fn ini_context(&self) -> &IniContext {
        &self.ini
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
