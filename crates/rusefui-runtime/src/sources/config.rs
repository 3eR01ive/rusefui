use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, sleep, JoinHandle};
use std::time::Duration;

use rusefi_ini::{
    decode_array, decode_config_at, decode_config_fields, encode_array_element,
    encode_config_value, ArrayShape, ConfigFieldKind,
};
use rusefi_protocol::{ProtocolError, TS_PAGE_SETTINGS};
use serde::Serialize;

use crate::session::EcuSession;
use crate::sources::output_channels::IniContext;

/// INI `pageActivationDelay` + время async-записи flash после burn.
const BURN_SETTLE_MS: u64 = 1500;
const BURN_RETRIES: usize = 3;
/// INI `interWriteDelay`.
const INTER_WRITE_DELAY_MS: u64 = 15;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEnumOption {
    pub value: u32,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFieldInfo {
    pub name: String,
    pub units: Option<String>,
    pub ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<ConfigEnumOption>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub array_cols: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub array_rows: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub array_length: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSnapshot {
    pub connected: bool,
    pub loaded: bool,
    pub loading: bool,
    /// 0.0 … 1.0 во время загрузки page 0.
    pub progress: f64,
    pub bytes_loaded: u32,
    pub bytes_total: u32,
    pub raw_len: usize,
    pub values: HashMap<String, f64>,
    pub field_count: usize,
    pub last_error: Option<String>,
}

impl ConfigSnapshot {
    pub fn disconnected(ini: &IniContext) -> Self {
        Self {
            connected: false,
            loaded: false,
            loading: false,
            progress: 0.0,
            bytes_loaded: 0,
            bytes_total: 0,
            raw_len: 0,
            values: HashMap::new(),
            field_count: ini.config_fields.len(),
            last_error: None,
        }
    }
}

pub struct ConfigSource {
    ini: Mutex<IniContext>,
    raw: Arc<Mutex<Vec<u8>>>,
    snapshot: Arc<RwLock<ConfigSnapshot>>,
    loading: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl ConfigSource {
    pub fn new(ini: IniContext) -> Self {
        Self {
            ini: Mutex::new(ini.clone()),
            raw: Arc::new(Mutex::new(Vec::new())),
            snapshot: Arc::new(RwLock::new(ConfigSnapshot::disconnected(&ini))),
            loading: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> ConfigSnapshot {
        self.snapshot.read().unwrap().clone()
    }

    pub fn list_fields(&self) -> Vec<ConfigFieldInfo> {
        self.ini
            .lock()
            .unwrap()
            .config_fields
            .iter()
            .map(|(name, f)| match f {
                ConfigFieldKind::Scalar(s) => ConfigFieldInfo {
                    name: name.clone(),
                    units: if s.units.is_empty() {
                        None
                    } else {
                        Some(s.units.clone())
                    },
                    ty: "scalar".into(),
                    options: None,
                    array_cols: None,
                    array_rows: None,
                    array_length: None,
                },
                ConfigFieldKind::Enum(e) => ConfigFieldInfo {
                    name: name.clone(),
                    units: None,
                    ty: "enum".into(),
                    options: Some(
                        e.options
                            .iter()
                            .map(|o| ConfigEnumOption {
                                value: o.value,
                                label: o.label.clone(),
                            })
                            .collect(),
                    ),
                    array_cols: None,
                    array_rows: None,
                    array_length: None,
                },
                ConfigFieldKind::Array(a) => {
                    let (array_cols, array_rows, array_length) = match a.shape {
                        ArrayShape::Vector(n) => (None, None, Some(n)),
                        ArrayShape::Matrix { cols, rows } => (Some(cols), Some(rows), None),
                    };
                    ConfigFieldInfo {
                        name: name.clone(),
                        units: if a.units.is_empty() {
                            None
                        } else {
                            Some(a.units.clone())
                        },
                        ty: "array".into(),
                        options: None,
                        array_cols,
                        array_rows,
                        array_length,
                    }
                },
            })
            .collect()
    }

    pub fn get_array(&self, name: &str) -> Result<Vec<f64>, String> {
        let ini = self.ini.lock().unwrap();
        let field = ini
            .config_fields
            .get(name)
            .ok_or_else(|| format!("unknown config field: {name}"))?;
        let ConfigFieldKind::Array(array) = field else {
            return Err(format!("{name} is not an array field"));
        };
        let raw = self.raw.lock().unwrap();
        Ok(decode_array(array, &raw))
    }

    pub fn write_array_value(
        &self,
        session: &EcuSession,
        name: &str,
        index: usize,
        value: f64,
    ) -> Result<(), String> {
        let ini = self.ini.lock().unwrap().clone();
        let field = ini
            .config_fields
            .get(name)
            .ok_or_else(|| format!("unknown config field: {name}"))?;
        let ConfigFieldKind::Array(array) = field else {
            return Err(format!("{name} is not an array field"));
        };
        let (offset, encoded) = encode_array_element(array, index, value)
            .ok_or_else(|| format!("cannot encode array value for {name}[{index}]"))?;

        if offset > u16::MAX as u32 {
            return Err(format!("offset {name}[{index}] exceeds protocol limit"));
        }

        session.with_link(|link| {
            link.write_config_chunk(
                TS_PAGE_SETTINGS,
                offset as u16,
                &encoded,
                true,
            )?;
            sleep(Duration::from_millis(INTER_WRITE_DELAY_MS));
            Ok(())
        })?;

        {
            let mut raw = self.raw.lock().unwrap();
            let off = offset as usize;
            if off + encoded.len() > raw.len() {
                raw.resize(off + encoded.len(), 0);
            }
            raw[off..off + encoded.len()].copy_from_slice(&encoded);
        }

        Ok(())
    }

    pub fn stop(&self) {
        self.loading.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        let ini = self.ini.lock().unwrap().clone();
        *self.raw.lock().unwrap() = Vec::new();
        *self.snapshot.write().unwrap() = ConfigSnapshot::disconnected(&ini);
    }

    pub fn replace_ini(&self, ini: IniContext) {
        self.stop();
        *self.ini.lock().unwrap() = ini.clone();
        *self.snapshot.write().unwrap() = ConfigSnapshot::disconnected(&ini);
    }

    pub fn start_load<F>(&self, session: Arc<EcuSession>, on_update: F)
    where
        F: Fn(ConfigSnapshot) + Send + Sync + 'static,
    {
        if self.snapshot.read().unwrap().loaded {
            return;
        }

        if self
            .loading
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }

        let ini = self.ini.lock().unwrap().clone();
        let field_count = ini.config_fields.len();
        let page_size = ini.page_size;
        let chunk_size = ini.blocking_factor;
        let read_has_page_index = ini.page_read_has_page_index;
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.connected = session.is_connected();
            snap.loading = true;
            snap.progress = 0.0;
            snap.bytes_loaded = 0;
            snap.bytes_total = page_size;
            snap.field_count = field_count;
            snap.last_error = None;
        }
        on_update(self.snapshot.read().unwrap().clone());

        let loading = Arc::clone(&self.loading);
        let snapshot = Arc::clone(&self.snapshot);
        let raw_store = Arc::clone(&self.raw);
        let on_update = Arc::new(on_update);
        let config_fields = ini.config_fields.clone();

        let handle = thread::Builder::new()
            .name("rusefui-config-load".into())
            .spawn(move || {
                let mut snap = snapshot.read().unwrap().clone();
                snap.connected = session.is_connected();
                snap.loading = true;

                if !session.is_connected() {
                    snap.loading = false;
                    snap.progress = 0.0;
                    snap.last_error = Some("ECU не подключена".into());
                } else {
                    let emit_progress = |loaded: u32, total: u32| {
                        let progress = if total == 0 {
                            0.0
                        } else {
                            (loaded as f64 / total as f64).clamp(0.0, 1.0)
                        };
                        let mut snap = snapshot.write().unwrap();
                        snap.loading = true;
                        snap.bytes_loaded = loaded;
                        snap.bytes_total = total;
                        snap.progress = progress;
                        on_update(snap.clone());
                    };

                    match session.with_link(|link| {
                        link.read_config_page_full_with_progress(
                            TS_PAGE_SETTINGS,
                            page_size,
                            chunk_size,
                            read_has_page_index,
                            emit_progress,
                        )
                    }) {
                        Ok(bytes) => {
                            let values = decode_config_fields(&config_fields, &bytes);
                            *raw_store.lock().unwrap() = bytes.clone();
                            snap = ConfigSnapshot {
                                connected: true,
                                loaded: true,
                                loading: false,
                                progress: 1.0,
                                bytes_loaded: page_size,
                                bytes_total: page_size,
                                raw_len: bytes.len(),
                                values,
                                field_count,
                                last_error: None,
                            };
                        }
                        Err(e) => {
                            snap.loaded = false;
                            snap.loading = false;
                            snap.progress = 0.0;
                            snap.last_error = Some(e);
                        }
                    }
                }

                loading.store(false, Ordering::SeqCst);
                *snapshot.write().unwrap() = snap.clone();
                on_update(snap);
            })
            .expect("spawn config load thread");

        *self.thread.lock().unwrap() = Some(handle);
    }

    /// Запись скаляра в RAM ECU (`C`) без verify-read и burn — как правка поля в TunerStudio до Save.
    pub fn write_scalar(
        &self,
        session: &EcuSession,
        name: &str,
        value: f64,
    ) -> Result<(), String> {
        let ini = self.ini.lock().unwrap().clone();
        let field = ini
            .config_fields
            .get(name)
            .ok_or_else(|| format!("unknown config field: {name}"))?;
        let offset = config_field_offset(field);
        let current = self.raw.lock().unwrap().clone();
        let encoded = encode_config_value(field, value, &current)
            .ok_or_else(|| format!("cannot encode value for {name}"))?;

        if offset > u16::MAX as u32 {
            return Err(format!("offset {name} exceeds protocol limit"));
        }

        session.with_link(|link| {
            link.write_config_chunk(
                TS_PAGE_SETTINGS,
                offset as u16,
                &encoded,
                true,
            )?;
            sleep(Duration::from_millis(INTER_WRITE_DELAY_MS));
            Ok(())
        })?;

        {
            let mut raw = self.raw.lock().unwrap();
            let off = offset as usize;
            if off + encoded.len() > raw.len() {
                raw.resize(off + encoded.len(), 0);
            }
            raw[off..off + encoded.len()].copy_from_slice(&encoded);
        }

        {
            let mut snap = self.snapshot.write().unwrap();
            snap.values.insert(name.to_string(), value);
        }

        Ok(())
    }

    pub fn set_scalar(
        &self,
        session: &EcuSession,
        name: &str,
        value: f64,
    ) -> Result<(), String> {
        let ini = self.ini.lock().unwrap().clone();
        let field = ini
            .config_fields
            .get(name)
            .ok_or_else(|| format!("unknown config field: {name}"))?;
        let offset = config_field_offset(field);
        let current = self.raw.lock().unwrap().clone();
        let encoded = encode_config_value(field, value, &current)
            .ok_or_else(|| format!("cannot encode value for {name}"))?;

        if offset > u16::MAX as u32 {
            return Err(format!("offset {name} exceeds protocol limit"));
        }

        let chunk_count = u16::try_from(encoded.len())
            .map_err(|_| format!("encoded value for {name} too large"))?;

        let read_has_page_index = ini.page_read_has_page_index;

        let actual = session.with_link(|link| {
            link.write_config_chunk(
                TS_PAGE_SETTINGS,
                offset as u16,
                &encoded,
                true,
            )?;
            sleep(Duration::from_millis(INTER_WRITE_DELAY_MS));

            let read_back = link.read_config_chunk(
                TS_PAGE_SETTINGS,
                offset as u16,
                chunk_count,
                read_has_page_index,
            )?;

            let off = offset as usize;
            let mut page = self.raw.lock().unwrap().clone();
            if off + read_back.len() > page.len() {
                page.resize(off + read_back.len(), 0);
            }
            page[off..off + read_back.len()].copy_from_slice(&read_back);

            let actual = decode_config_at(field, &page).ok_or_else(|| {
                ProtocolError::InvalidPacket(format!(
                    "config write verify decode failed at offset {} for {name}",
                    offset
                ))
            })?;

            if (actual - value).abs() > 1e-4 {
                return Err(ProtocolError::InvalidPacket(format!(
                    "config write verify failed for {name}: expected {value}, ECU has {actual} (offset {})",
                    offset
                )));
            }

            let mut last_err = None;
            for attempt in 0..BURN_RETRIES {
                match link.burn_config_page(TS_PAGE_SETTINGS) {
                    Ok(()) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        last_err = Some(e);
                        if attempt + 1 < BURN_RETRIES {
                            sleep(Duration::from_millis(15));
                        }
                    }
                }
            }
            if let Some(e) = last_err {
                return Err(e);
            }

            sleep(Duration::from_millis(BURN_SETTLE_MS));

            Ok(actual)
        })?;

        {
            let mut raw = self.raw.lock().unwrap();
            let off = offset as usize;
            if off + encoded.len() <= raw.len() {
                raw[off..off + encoded.len()].copy_from_slice(&encoded);
            }
        }

        {
            let mut snap = self.snapshot.write().unwrap();
            snap.values.insert(name.to_string(), actual);
        }

        Ok(())
    }
}

fn config_field_offset(field: &ConfigFieldKind) -> u32 {
    match field {
        ConfigFieldKind::Scalar(s) => s.offset,
        ConfigFieldKind::Enum(e) => e.bits.offset,
        ConfigFieldKind::Array(a) => a.offset,
    }
}
