use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, sleep, JoinHandle};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rusefi_ini::{
    decode_array, decode_config_fields, encode_array_element,
    encode_config_value, ArrayShape, ConfigFieldKind,
};
use rusefi_protocol::{ProtocolError, TS_PAGE_SETTINGS};
use serde::Serialize;

use crate::config_diff::encode_scalar_into_page;
use crate::project::ProjectEcuConfig;
use crate::session::EcuSession;
use crate::sources::output_channels::IniContext;

/// INI `pageActivationDelay` + время async-записи flash после burn.
const BURN_SETTLE_MS: u64 = 1500;
const BURN_RETRIES: usize = 3;
/// INI `interWriteDelay` (типично 10 ms).
const INTER_WRITE_DELAY_MS: u64 = 10;

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
    /// Данные из файла проекта (редактируются offline, не live ECU).
    #[serde(default)]
    pub read_only: bool,
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
            read_only: false,
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

    /// Сырой образ page 0 (для encode точечных записей, напр. triggerSimulatorRpm).
    pub fn page_raw(&self) -> Vec<u8> {
        self.raw.lock().unwrap().clone()
    }

    pub fn patch_page_raw(&self, offset: usize, bytes: &[u8]) {
        let mut raw = self.raw.lock().unwrap();
        if offset + bytes.len() > raw.len() {
            raw.resize(offset + bytes.len(), 0);
        }
        raw[offset..offset + bytes.len()].copy_from_slice(bytes);
    }

    /// Подставить снимок page 0 из файла проекта (offline preview).
    pub fn apply_from_project(&self, ecu: &ProjectEcuConfig) -> Result<(), String> {
        let raw = B64
            .decode(&ecu.raw_page0_base64)
            .map_err(|e| format!("Некорректный base64 page0: {e}"))?;

        let ini = self.ini.lock().unwrap().clone();
        if ini.config_fields.is_empty() {
            return Err(
                "INI не загружен — сохраните проект с INI или укажите существующий ini.path"
                    .into(),
            );
        }

        let values = decode_config_fields(&ini.config_fields, &raw);

        *self.raw.lock().unwrap() = raw.clone();

        let mut snap = self.snapshot.write().unwrap();
        snap.connected = false;
        snap.loaded = true;
        snap.read_only = true;
        snap.loading = false;
        snap.progress = 1.0;
        snap.bytes_loaded = ecu.page_size;
        snap.bytes_total = ecu.page_size;
        snap.raw_len = raw.len();
        snap.values = values;
        snap.field_count = ini.config_fields.len();
        snap.last_error = None;
        Ok(())
    }

    fn ensure_ecu_writable(&self) -> Result<(), String> {
        let snap = self.snapshot.read().unwrap();
        if snap.read_only {
            return Err("Сейчас открыт config проекта — для записи на ECU подключитесь и дождитесь загрузки с блока.".into());
        }
        if !snap.connected {
            return Err("ECU не подключена".into());
        }
        Ok(())
    }

    fn ensure_page_raw(&self, ini: &IniContext, raw: &mut Vec<u8>) {
        if raw.is_empty() && ini.page_size > 0 {
            raw.resize(ini.page_size as usize, 0);
        }
    }

    fn refresh_snapshot_from_raw(&self, ini: &IniContext, raw: &[u8], project_mode: bool) {
        let values = decode_config_fields(&ini.config_fields, raw);
        let mut snap = self.snapshot.write().unwrap();
        snap.loaded = true;
        snap.read_only = project_mode;
        snap.connected = false;
        snap.loading = false;
        snap.progress = 1.0;
        snap.bytes_loaded = ini.page_size;
        snap.bytes_total = ini.page_size;
        snap.raw_len = raw.len();
        snap.values = values;
        snap.field_count = ini.config_fields.len();
        snap.last_error = None;
    }

    /// Изменить поле в RAM-снимке проекта (без ECU).
    pub fn set_scalar_local(&self, name: &str, value: f64) -> Result<(), String> {
        let ini = self.ini.lock().unwrap().clone();
        if ini.config_fields.is_empty() {
            return Err("INI не загружен".into());
        }
        let mut raw = self.raw.lock().unwrap();
        self.ensure_page_raw(&ini, &mut raw);
        encode_scalar_into_page(&ini, &mut raw, name, value)?;
        let raw_copy = raw.clone();
        drop(raw);
        self.refresh_snapshot_from_raw(&ini, &raw_copy, true);
        Ok(())
    }

    /// Изменить элемент таблицы/кривой в RAM-снимке проекта (без ECU).
    pub fn set_array_value_local(&self, name: &str, index: usize, value: f64) -> Result<(), String> {
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

        let mut raw = self.raw.lock().unwrap();
        self.ensure_page_raw(&ini, &mut raw);
        let off = offset as usize;
        if off + encoded.len() > raw.len() {
            raw.resize(off + encoded.len(), 0);
        }
        raw[off..off + encoded.len()].copy_from_slice(&encoded);
        let raw_copy = raw.clone();
        drop(raw);
        self.refresh_snapshot_from_raw(&ini, &raw_copy, true);
        Ok(())
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
        self.ensure_ecu_writable()?;
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

        self.write_verified_chunk(
            session,
            ini.page_read_has_page_index,
            ini.page_chunk_write_has_page_index,
            offset as u16,
            &encoded,
            &format!("{name}[{index}]"),
        )?;

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
        let snap = self.snapshot.read().unwrap();
        if snap.loaded {
            return;
        }
        drop(snap);

        if self
            .loading
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let _ = self.thread.lock().unwrap().take();

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
                    let last_progress_emit = RefCell::new(Instant::now());
                    let emit_progress = |loaded: u32, total: u32| {
                        let now = Instant::now();
                        if loaded < total
                            && now
                                .duration_since(*last_progress_emit.borrow())
                                < Duration::from_millis(250)
                        {
                            return;
                        }
                        *last_progress_emit.borrow_mut() = now;
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

                    const LOAD_RETRY_MS: u64 = 100;
                    const LOAD_RETRY_MAX: u32 = 120;
                    let mut load_result: Option<Result<Vec<u8>, String>> = None;
                    for _ in 0..LOAD_RETRY_MAX {
                        if !session.is_connected() {
                            break;
                        }
                        match session.try_with_link(|link| {
                            link.read_config_page_full_with_progress(
                                TS_PAGE_SETTINGS,
                                page_size,
                                chunk_size,
                                read_has_page_index,
                                emit_progress,
                            )
                        }) {
                            Some(Ok(bytes)) => {
                                load_result = Some(Ok(bytes));
                                break;
                            }
                            Some(Err(e)) => {
                                load_result = Some(Err(e));
                                break;
                            }
                            None => thread::sleep(Duration::from_millis(LOAD_RETRY_MS)),
                        }
                    }

                    match load_result {
                        Some(Ok(bytes)) => {
                            let values = decode_config_fields(&config_fields, &bytes);
                            *raw_store.lock().unwrap() = bytes.clone();
                            snap = ConfigSnapshot {
                                connected: true,
                                loaded: true,
                                read_only: false,
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
                        Some(Err(e)) => {
                            snap.loaded = false;
                            snap.loading = false;
                            snap.progress = 0.0;
                            snap.last_error = Some(e);
                        }
                        None => {
                            snap.loaded = false;
                            snap.loading = false;
                            snap.progress = 0.0;
                            snap.last_error = Some(
                                "ECU занята слишком долго — загрузка конфигурации отменена"
                                    .into(),
                            );
                        }
                    }
                }

                loading.store(false, Ordering::SeqCst);
                if snapshot.read().unwrap().read_only {
                    return;
                }
                *snapshot.write().unwrap() = snap.clone();
                on_update(snap);
            })
            .expect("spawn config load thread");

        *self.thread.lock().unwrap() = Some(handle);
    }

    /// Запись скаляра в RAM ECU (`C`) + verify-read; flash — отдельно [`Self::burn_to_flash`].
    pub fn write_scalar(
        &self,
        session: &EcuSession,
        name: &str,
        value: f64,
    ) -> Result<(), String> {
        self.ensure_ecu_writable()?;
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

        self.write_verified_chunk(
            session,
            ini.page_read_has_page_index,
            ini.page_chunk_write_has_page_index,
            offset as u16,
            &encoded,
            name,
        )?;

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
            snap.last_error = None;
        }

        Ok(())
    }

    /// `C` + verify + `B` (как было при сохранении поля целиком).
    pub fn set_scalar(
        &self,
        session: &EcuSession,
        name: &str,
        value: f64,
    ) -> Result<(), String> {
        self.write_scalar(session, name, value)?;
        self.burn_to_flash(session)
    }

    /// Commit settings page 0 во flash (`B`), затем перечитать page с ECU.
    pub fn burn_to_flash(&self, session: &EcuSession) -> Result<(), String> {
        if !session.is_connected() {
            return Err("ECU не подключена".into());
        }

        session.run_without_output_poll(|session| {
            session.with_link(|link| {
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
                Ok(())
            })
        })?;

        self.reload_page_from_ecu(session)?;

        let mut snap = self.snapshot.write().unwrap();
        snap.last_error = None;
        Ok(())
    }

    /// Перечитать page 0 с ECU в кэш (после burn / переподключения).
    pub fn reload_page_from_ecu(&self, session: &EcuSession) -> Result<(), String> {
        if !session.is_connected() {
            return Err("ECU не подключена".into());
        }

        let ini = self.ini.lock().unwrap().clone();
        let config_fields = ini.config_fields.clone();
        let page_size = ini.page_size;
        let chunk_size = ini.blocking_factor;
        let read_has_page_index = ini.page_read_has_page_index;

        let bytes = session.run_without_output_poll(|session| {
            session.with_link(|link| {
                link.read_config_page_full(
                    TS_PAGE_SETTINGS,
                    page_size,
                    chunk_size,
                    read_has_page_index,
                )
            })
        })?;

        let values = decode_config_fields(&config_fields, &bytes);
        *self.raw.lock().unwrap() = bytes.clone();

        let mut snap = self.snapshot.write().unwrap();
        snap.connected = true;
        snap.loaded = true;
        snap.read_only = false;
        snap.loading = false;
        snap.progress = 1.0;
        snap.bytes_loaded = page_size;
        snap.bytes_total = page_size;
        snap.raw_len = bytes.len();
        snap.values = values;
        snap.field_count = config_fields.len();
        snap.last_error = None;

        Ok(())
    }

    fn write_verified_chunk(
        &self,
        session: &EcuSession,
        page_read_has_page_index: bool,
        page_chunk_write_has_page_index: bool,
        offset: u16,
        encoded: &[u8],
        field_label: &str,
    ) -> Result<(), String> {
        let count = u16::try_from(encoded.len())
            .map_err(|_| format!("encoded chunk too large for {field_label}"))?;

        session.run_without_output_poll(|session| {
            session.with_link(|link| {
                link.write_config_chunk(
                    TS_PAGE_SETTINGS,
                    offset,
                    encoded,
                    page_chunk_write_has_page_index,
                )?;
                sleep(Duration::from_millis(INTER_WRITE_DELAY_MS));

                let read_back = link.read_config_chunk(
                    TS_PAGE_SETTINGS,
                    offset,
                    count,
                    page_read_has_page_index,
                )?;

                if read_back.as_slice() != encoded {
                    let sent = encoded
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let got = read_back
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    return Err(ProtocolError::InvalidPacket(format!(
                        "config write verify failed for {field_label} at offset {offset}: sent [{sent}] read [{got}]"
                    )));
                }
                Ok(())
            })
        })
    }
}

fn config_field_offset(field: &ConfigFieldKind) -> u32 {
    match field {
        ConfigFieldKind::Scalar(s) => s.offset,
        ConfigFieldKind::Enum(e) => e.bits.offset,
        ConfigFieldKind::Array(a) => a.offset,
    }
}
