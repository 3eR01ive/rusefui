use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusefi_protocol::{ConnectionInfo, ProtocolError, SerialLink, DEFAULT_IO_TIMEOUT_MS};

use crate::ini::resolve_ini_for_signature;
use crate::protocol_log::ProtocolLogStore;
use crate::sources::config::ConfigSource;
use crate::sources::output_channels::{IniContext, OutputChannelsSource};

struct EcuSessionInner {
    link: Option<SerialLink>,
}

/// Общая сессия ECU: serial link + фоновый опрос output channels.
pub struct EcuSession {
    inner: Mutex<EcuSessionInner>,
    ini: Mutex<IniContext>,
    loaded_ini_path: Mutex<Option<PathBuf>>,
    output: OutputChannelsSource,
    config: ConfigSource,
    protocol_log: Arc<ProtocolLogStore>,
}

impl EcuSession {
    pub fn new_arc(protocol_log: Arc<ProtocolLogStore>) -> Arc<Self> {
        let ini_ctx = IniContext::disconnected();
        Arc::new(Self {
            inner: Mutex::new(EcuSessionInner { link: None }),
            ini: Mutex::new(ini_ctx.clone()),
            loaded_ini_path: Mutex::new(None),
            output: OutputChannelsSource::new(ini_ctx.clone()),
            config: ConfigSource::new(ini_ctx),
            protocol_log,
        })
    }

    pub fn protocol_log(&self) -> Arc<ProtocolLogStore> {
        Arc::clone(&self.protocol_log)
    }

    pub fn ini_context(&self) -> IniContext {
        self.ini.lock().unwrap().clone()
    }

    pub fn loaded_ini_path(&self) -> Option<PathBuf> {
        self.loaded_ini_path.lock().unwrap().clone()
    }

    pub fn output(&self) -> &OutputChannelsSource {
        &self.output
    }

    pub fn config(&self) -> &ConfigSource {
        &self.config
    }

    pub fn is_connected(&self) -> bool {
        self.inner.lock().unwrap().link.is_some()
    }

    pub fn connect(&self, port: &str, baud_rate: u32) -> Result<ConnectionInfo, String> {
        self.output.stop();
        self.config.stop();
        let tracer = Some(Arc::clone(&self.protocol_log) as Arc<dyn rusefi_protocol::ProtocolTracer>);
        let link = SerialLink::connect(port, baud_rate, DEFAULT_IO_TIMEOUT_MS, tracer)
            .map_err(|e| e.to_string())?;
        let info = link.info().clone();

        let resolved = match resolve_ini_for_signature(&info.signature) {
            Ok(resolved) => resolved,
            Err(e) => {
                self.protocol_log.log_info(&format!(
                    "Подключение отклонено: {e} (signature={})",
                    info.signature
                ));
                return Err(e.to_string());
            }
        };

        let ini_ctx = IniContext::from_ini(&resolved.file);
        *self.ini.lock().unwrap() = ini_ctx.clone();
        *self.loaded_ini_path.lock().unwrap() = Some(resolved.path.clone());
        self.output.replace_ini(ini_ctx.clone());
        self.config.replace_ini(ini_ctx);

        self.protocol_log.log_info(&format!(
            "Подключено: {} @ {} baud, signature={}, INI={}",
            info.port_name,
            info.baud_rate,
            info.signature,
            resolved.path.display()
        ));
        self.inner.lock().unwrap().link = Some(link);
        Ok(info)
    }

    pub fn disconnect(&self) {
        self.output.stop();
        self.config.stop();
        let mut guard = self.inner.lock().unwrap();
        if guard.link.is_some() {
            self.protocol_log.log_info("Отключено от ECU");
            guard.link = None;
        }
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

    /// Останавливает poll `O`, выполняет `f`, не перезапускает poll (вызовите `output().start` снаружи).
    pub fn run_without_output_poll<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Self) -> Result<R, String>,
    {
        self.output().stop();
        f(self)
    }
}
