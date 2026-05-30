use std::cell::RefCell;
use std::collections::HashMap;
use rusefi_ini::config_field_ini_page;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, sleep, JoinHandle};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rusefi_ini::{
    decode_array, decode_config_fields_pages, decode_config_strings_pages, encode_array_element,
    encode_config_value, encode_string_value, ArrayShape, ConfigFieldKind, DEFAULT_INI_PAGE,
};
use rusefi_protocol::{ProtocolError, TS_PAGE_SETTINGS, TS_RESPONSE_OUT_OF_RANGE};
use serde::Serialize;

use crate::config_checklist::{evaluate_checklist, ChecklistRules, ChecklistSnapshot};
use crate::config_diff::{encode_scalar_into_page, encode_string_into_page};
use crate::project::ProjectEcuConfig;
use crate::session::EcuSession;
use crate::sources::output_channels::IniContext;
use crate::sources::pin_allocation::build_pin_usage;

/// INI `pageActivationDelay` + время async-записи flash после burn.
const BURN_SETTLE_MS: u64 = 1500;
const BURN_RETRIES: usize = 3;
/// INI `interWriteDelay` (типично 10 ms).
const INTER_WRITE_DELAY_MS: u64 = 10;

fn ecu_error_is_out_of_range(err: &str) -> bool {
    err.contains(&format!("0x{TS_RESPONSE_OUT_OF_RANGE:02X}"))
        || err.to_ascii_lowercase().contains("out of range")
        || err.contains("invalid page")
}

/// Какие INI-страницы реально читать с ECU (legacy — только page 1).
fn ecu_page_load_plan(ini: &IniContext) -> Vec<u32> {
    let sizes = if ini.page_sizes.is_empty() {
        vec![ini.page_size]
    } else {
        ini.page_sizes.clone()
    };
    if ini.page_read_has_page_index {
        sizes
    } else {
        sizes.into_iter().take(1).collect()
    }
}

struct EcuPagesLoadOutcome {
    pages: ConfigPageStore,
}

/// Чтение config-страниц с ECU. Page 1 обязательна; 2+ при 0x84 — нулевой буфер.
fn load_ecu_pages_once(
    session: &EcuSession,
    ini: &IniContext,
    mut on_progress: impl FnMut(u32, u32),
) -> Result<EcuPagesLoadOutcome, String> {
    let page_sizes = ecu_page_load_plan(ini);
    let total_bytes: u32 = page_sizes.iter().sum::<u32>().max(1);
    let chunk_size = ini.blocking_factor;
    let read_has_page = ini.page_read_has_page_index;
    let mut pages = ConfigPageStore::new();
    let mut base_loaded = 0u32;

    for (idx, &page_size) in page_sizes.iter().enumerate() {
        let protocol_page = idx as u16;
        let ini_page = (idx as u8) + 1;
        let read_result = session.try_with_link(|link| {
            link.read_config_page_full(
                protocol_page,
                page_size,
                chunk_size,
                read_has_page,
            )
        });

        match read_result {
            Some(Ok(bytes)) => {
                pages.insert(ini_page, bytes);
            }
            Some(Err(e)) => {
                let err = e.to_string();
                if idx == 0 {
                    return Err(err);
                }
                if ecu_error_is_out_of_range(&err) {
                    pages.insert(ini_page, vec![0u8; page_size as usize]);
                } else {
                    return Err(err);
                }
            }
            None => return Err("ECU занята — чтение config не началось".into()),
        }
        base_loaded += page_size;
        on_progress(base_loaded.min(total_bytes), total_bytes);
    }

    Ok(EcuPagesLoadOutcome { pages })
}

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
    /// INI `$output_pin_e_list` и т.п. — поля с одним пулом пинов.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin_pool: Option<String>,
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
    #[serde(default)]
    pub string_values: HashMap<String, String>,
    pub field_count: usize,
    pub last_error: Option<String>,
    /// Занятость пинов по пулам INI (пересчитывается в Rust при каждом снимке).
    #[serde(default)]
    pub pin_usage: HashMap<String, HashMap<u32, Vec<String>>>,
    /// Checklist (заполняется при emit снимка).
    #[serde(default)]
    pub checklist: ChecklistSnapshot,
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
            string_values: HashMap::new(),
            field_count: ini.config_fields.len(),
            last_error: None,
            pin_usage: HashMap::new(),
            checklist: ChecklistSnapshot::default(),
        }
    }
}

fn apply_decoded_values(
    snap: &mut ConfigSnapshot,
    ini: &IniContext,
    values: HashMap<String, f64>,
    string_values: HashMap<String, String>,
) {
    snap.values = values;
    snap.string_values = string_values;
    snap.pin_usage = build_pin_usage(&ini.config_fields, &snap.values);
}

/// INI page number (1-based) → сырой образ страницы.
pub type ConfigPageStore = HashMap<u8, Vec<u8>>;

pub struct ConfigSource {
    ini: Mutex<IniContext>,
    pages: Arc<Mutex<ConfigPageStore>>,
    snapshot: Arc<RwLock<ConfigSnapshot>>,
    loading: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
    live_ram_dirty_hook: Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>,
}

impl ConfigSource {
    pub fn new(ini: IniContext) -> Self {
        Self {
            ini: Mutex::new(ini.clone()),
            pages: Arc::new(Mutex::new(ConfigPageStore::new())),
            snapshot: Arc::new(RwLock::new(ConfigSnapshot::disconnected(&ini))),
            loading: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
            live_ram_dirty_hook: Arc::new(Mutex::new(None)),
        }
    }

    /// Колбэк из Tauri: RAM ECU изменён, нужен Burn во flash.
    pub fn set_live_ram_dirty_hook<F>(&self, hook: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        *self.live_ram_dirty_hook.lock().unwrap() = Some(Arc::new(hook));
    }

    pub fn is_live_ecu_editing(&self) -> bool {
        let snap = self.snapshot.read().unwrap();
        snap.connected && snap.loaded && !snap.read_only
    }

    fn notify_live_ram_dirty(&self) {
        if !self.is_live_ecu_editing() {
            return;
        }
        if let Some(hook) = self.live_ram_dirty_hook.lock().unwrap().as_ref() {
            hook();
        }
    }

    pub fn snapshot(&self) -> ConfigSnapshot {
        self.snapshot.read().unwrap().clone()
    }

    /// Снимок с checklist (если rules заданы).
    pub fn snapshot_with_checklist(&self, rules: Option<&ChecklistRules>) -> ConfigSnapshot {
        let mut snap = self.snapshot();
        if let Some(rules) = rules {
            snap.checklist = evaluate_checklist(&snap, rules, self);
        }
        snap
    }

    /// Сырой образ основной страницы INI page 1 (legacy name: page 0).
    pub fn page_raw(&self) -> Vec<u8> {
        self.page_raw_ini(DEFAULT_INI_PAGE)
    }

    pub fn page_raw_ini(&self, ini_page: u8) -> Vec<u8> {
        self.pages
            .lock()
            .unwrap()
            .get(&ini_page)
            .cloned()
            .unwrap_or_default()
    }

    pub fn config_pages(&self) -> ConfigPageStore {
        self.pages.lock().unwrap().clone()
    }

    pub fn set_config_pages(&self, pages: ConfigPageStore) {
        *self.pages.lock().unwrap() = pages;
    }

    pub fn patch_page_raw(&self, offset: usize, bytes: &[u8]) {
        self.patch_page_raw_ini(DEFAULT_INI_PAGE, offset, bytes);
    }

    pub fn patch_page_raw_ini(&self, ini_page: u8, offset: usize, bytes: &[u8]) {
        let mut pages = self.pages.lock().unwrap();
        let raw = pages.entry(ini_page).or_default();
        if offset + bytes.len() > raw.len() {
            raw.resize(offset + bytes.len(), 0);
        }
        raw[offset..offset + bytes.len()].copy_from_slice(bytes);
    }

    fn pages_decode_slices(&self) -> Vec<(u8, Vec<u8>)> {
        self.pages
            .lock()
            .unwrap()
            .iter()
            .map(|(p, v)| (*p, v.clone()))
            .collect()
    }

    fn decode_snapshot_maps(
        &self,
        ini: &IniContext,
    ) -> (HashMap<String, f64>, HashMap<String, String>) {
        let owned = self.pages_decode_slices();
        let slices: Vec<(u8, &[u8])> = owned.iter().map(|(p, v)| (*p, v.as_slice())).collect();
        let values = decode_config_fields_pages(&ini.config_fields, &slices);
        let string_values = decode_config_strings_pages(&ini.config_fields, &slices);
        (values, string_values)
    }

    /// Подставить снимок page 0 из файла проекта (offline preview).
    ///
    /// `expected_signature` — `project.ini.signature` при сохранении; layout page 0
    /// декодируется только при совпадении INI в сессии.
    pub fn apply_from_project(
        &self,
        ecu: &ProjectEcuConfig,
        expected_signature: Option<&str>,
    ) -> Result<(), String> {
        let ini = self.ini.lock().unwrap().clone();
        if ini.config_fields.is_empty() {
            return Err(
                "INI не загружен — сохраните проект с INI или укажите существующий ini.path"
                    .into(),
            );
        }

        if let Some(expected) = expected_signature.filter(|s| !s.is_empty()) {
            match ini.signature.as_deref() {
                Some(ini_sig) if ini_sig == expected => {}
                Some(ini_sig) => {
                    return Err(format!(
                        "INI в сессии ({ini_sig}) не совпадает с проектом ({expected}). \
                         Укажите ini.path из проекта или выберите INI с той же signature, что при сохранении."
                    ));
                }
                None => {
                    return Err(format!(
                        "Загруженный INI не содержит signature; проект сохранён с {expected}"
                    ));
                }
            }
        }

        let pages = pages_from_project_ecu(ecu, &ini)?;
        let total_raw: usize = pages.values().map(|v| v.len()).sum();
        let (values, string_values) = {
            let owned: Vec<(u8, Vec<u8>)> = pages
                .iter()
                .map(|(p, v)| (*p, v.clone()))
                .collect();
            let slices: Vec<(u8, &[u8])> = owned.iter().map(|(p, v)| (*p, v.as_slice())).collect();
            (
                decode_config_fields_pages(&ini.config_fields, &slices),
                decode_config_strings_pages(&ini.config_fields, &slices),
            )
        };

        *self.pages.lock().unwrap() = pages;

        let mut snap = self.snapshot.write().unwrap();
        snap.connected = false;
        snap.loaded = true;
        snap.read_only = true;
        snap.loading = false;
        snap.progress = 1.0;
        snap.bytes_loaded = ecu.page_size;
        snap.bytes_total = ecu.page_size;
        snap.raw_len = total_raw;
        apply_decoded_values(&mut snap, &ini, values, string_values);
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

    fn ensure_page_buf(ini: &IniContext, ini_page: u8, raw: &mut Vec<u8>) {
        let idx = ini_page.saturating_sub(1) as usize;
        let want = ini
            .page_sizes
            .get(idx)
            .copied()
            .unwrap_or(ini.page_size) as usize;
        if raw.is_empty() && want > 0 {
            raw.resize(want, 0);
        }
    }

    fn refresh_snapshot_from_pages(&self, ini: &IniContext, project_mode: bool) {
        let (values, string_values) = self.decode_snapshot_maps(ini);
        let total_raw: usize = self.pages.lock().unwrap().values().map(|v| v.len()).sum();
        let mut snap = self.snapshot.write().unwrap();
        snap.loaded = true;
        snap.read_only = project_mode;
        snap.connected = false;
        snap.loading = false;
        snap.progress = 1.0;
        snap.bytes_loaded = ini.page_size;
        snap.bytes_total = ini.page_size;
        snap.raw_len = total_raw;
        apply_decoded_values(&mut snap, ini, values, string_values);
        snap.field_count = ini.config_fields.len();
        snap.last_error = None;
    }

    /// Изменить строковое поле в RAM-снимке проекта (без ECU).
    pub fn set_string_local(&self, name: &str, value: &str) -> Result<(), String> {
        let ini = self.ini.lock().unwrap().clone();
        if ini.config_fields.is_empty() {
            return Err("INI не загружен".into());
        }
        let mut pages = self.pages.lock().unwrap();
        let raw = pages.entry(DEFAULT_INI_PAGE).or_default();
        Self::ensure_page_buf(&ini, DEFAULT_INI_PAGE, raw);
        encode_string_into_page(&ini, raw, name, value)?;
        drop(pages);
        self.refresh_snapshot_from_pages(&ini, true);
        Ok(())
    }

    /// Изменить поле в RAM-снимке проекта (без ECU).
    pub fn set_scalar_local(&self, name: &str, value: f64) -> Result<(), String> {
        let ini = self.ini.lock().unwrap().clone();
        if ini.config_fields.is_empty() {
            return Err("INI не загружен".into());
        }
        let mut pages = self.pages.lock().unwrap();
        let raw = pages.entry(DEFAULT_INI_PAGE).or_default();
        Self::ensure_page_buf(&ini, DEFAULT_INI_PAGE, raw);
        encode_scalar_into_page(&ini, raw, name, value)?;
        drop(pages);
        self.refresh_snapshot_from_pages(&ini, true);
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

        let ini_page = array.page;
        let mut pages = self.pages.lock().unwrap();
        let raw = pages.entry(ini_page).or_default();
        Self::ensure_page_buf(&ini, ini_page, raw);
        let off = offset as usize;
        if off + encoded.len() > raw.len() {
            raw.resize(off + encoded.len(), 0);
        }
        raw[off..off + encoded.len()].copy_from_slice(&encoded);
        drop(pages);
        self.refresh_snapshot_from_pages(&ini, true);
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
                    pin_pool: None,
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
                    pin_pool: e.enum_define.clone(),
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
                        pin_pool: None,
                    }
                },
                ConfigFieldKind::String(s) => ConfigFieldInfo {
                    name: name.clone(),
                    units: None,
                    ty: "string".into(),
                    options: None,
                    array_cols: None,
                    array_rows: None,
                    array_length: Some(s.length as usize),
                    pin_pool: None,
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
        let ini_page = array.page;
        let pages = self.pages.lock().unwrap();
        let bytes = pages.get(&ini_page).map(|v| v.as_slice()).unwrap_or(&[]);
        Ok(decode_array(array, bytes))
    }

    /// Размер 2D-таблицы из INI `[cols x rows]` → `(rows, cols)` для UI (строка = Y/load).
    pub fn get_array_matrix_size(&self, name: &str) -> Option<(usize, usize)> {
        let ini = self.ini.lock().unwrap();
        let ConfigFieldKind::Array(a) = ini.config_fields.get(name)? else {
            return None;
        };
        match a.shape {
            ArrayShape::Matrix { cols, rows } => Some((rows, cols)),
            _ => None,
        }
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

        let protocol_page = array.page.saturating_sub(1) as u16;
        self.write_verified_chunk(
            session,
            protocol_page,
            ini.page_read_has_page_index,
            ini.page_chunk_write_has_page_index,
            offset as u16,
            &encoded,
            &format!("{name}[{index}]"),
        )?;

        {
            let ini_page = array.page;
            let mut pages = self.pages.lock().unwrap();
            let raw = pages.entry(ini_page).or_default();
            let off = offset as usize;
            if off + encoded.len() > raw.len() {
                raw.resize(off + encoded.len(), 0);
            }
            raw[off..off + encoded.len()].copy_from_slice(&encoded);
        }

        Ok(())
    }

    fn finish_live_array_write(&self) -> Result<(), String> {
        let ini = self.ini.lock().unwrap().clone();
        let (values, string_values) = self.decode_snapshot_maps(&ini);
        let mut snap = self.snapshot.write().unwrap();
        apply_decoded_values(&mut snap, &ini, values, string_values);
        snap.last_error = None;
        drop(snap);
        self.notify_live_ram_dirty();
        Ok(())
    }

    /// Пакетная запись элементов массива в RAM (один пересчёт снимка).
    pub fn set_array_values_local(
        &self,
        name: &str,
        updates: &[(usize, f64)],
    ) -> Result<(), String> {
        if updates.is_empty() {
            return Ok(());
        }
        self.patch_array_pages(name, updates)?;
        let ini = self.ini.lock().unwrap().clone();
        self.refresh_snapshot_from_pages(&ini, true);
        Ok(())
    }

    /// Обновить элементы массива в RAM, сохранив connected/read_only снимка (live ECU).
    pub fn patch_array_values_snapshot(
        &self,
        name: &str,
        updates: &[(usize, f64)],
    ) -> Result<(), String> {
        if updates.is_empty() {
            return Ok(());
        }
        self.patch_array_pages(name, updates)?;
        let ini = self.ini.lock().unwrap().clone();
        let (values, string_values) = self.decode_snapshot_maps(&ini);
        let mut snap = self.snapshot.write().unwrap();
        let read_only = snap.read_only;
        let connected = snap.connected;
        apply_decoded_values(&mut snap, &ini, values, string_values);
        snap.read_only = read_only;
        snap.connected = connected;
        snap.last_error = None;
        drop(snap);
        self.notify_live_ram_dirty();
        Ok(())
    }

    fn patch_array_pages(&self, name: &str, updates: &[(usize, f64)]) -> Result<(), String> {
        let ini = self.ini.lock().unwrap().clone();
        let field = ini
            .config_fields
            .get(name)
            .ok_or_else(|| format!("unknown config field: {name}"))?;
        let ConfigFieldKind::Array(array) = field else {
            return Err(format!("{name} is not an array field"));
        };

        let ini_page = array.page;
        let mut pages = self.pages.lock().unwrap();
        let raw = pages.entry(ini_page).or_default();
        Self::ensure_page_buf(&ini, ini_page, raw);
        for &(index, value) in updates {
            let (offset, encoded) = encode_array_element(array, index, value)
                .ok_or_else(|| format!("cannot encode array value for {name}[{index}]"))?;
            let off = offset as usize;
            if off + encoded.len() > raw.len() {
                raw.resize(off + encoded.len(), 0);
            }
            raw[off..off + encoded.len()].copy_from_slice(&encoded);
        }
        Ok(())
    }

    /// Пакетная запись в ECU (по элементу, с verify).
    pub fn write_array_values(
        &self,
        session: &EcuSession,
        name: &str,
        updates: &[(usize, f64)],
    ) -> Result<(), String> {
        for &(index, value) in updates {
            self.write_array_value(session, name, index, value)?;
            sleep(Duration::from_millis(INTER_WRITE_DELAY_MS));
        }
        self.finish_live_array_write()
    }

    pub fn stop(&self) {
        self.loading.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        let ini = self.ini.lock().unwrap().clone();
        self.pages.lock().unwrap().clear();
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
        let page_sizes = ecu_page_load_plan(&ini);
        let total_bytes: u32 = page_sizes.iter().sum();
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.connected = session.is_connected();
            snap.loading = true;
            snap.progress = 0.0;
            snap.bytes_loaded = 0;
            snap.bytes_total = total_bytes.max(1);
            snap.field_count = field_count;
            snap.last_error = None;
        }
        on_update(self.snapshot.read().unwrap().clone());

        let loading = Arc::clone(&self.loading);
        let snapshot = Arc::clone(&self.snapshot);
        let pages_store = Arc::clone(&self.pages);
        let on_update = Arc::new(on_update);
        let config_fields = ini.config_fields.clone();
        let ini_ctx = ini.clone();
        let backup = {
            let snap = self.snapshot.read().unwrap();
            if snap.loaded {
                Some((
                    self.pages.lock().unwrap().clone(),
                    snap.clone(),
                ))
            } else {
                None
            }
        };

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
                    let mut load_result: Option<Result<ConfigPageStore, String>> = None;
                    for _ in 0..LOAD_RETRY_MAX {
                        if !session.is_connected() {
                            break;
                        }
                        match load_ecu_pages_once(&session, &ini_ctx, |loaded, total| {
                            emit_progress(loaded, total);
                        }) {
                            Ok(outcome) => {
                                load_result = Some(Ok(outcome.pages));
                                break;
                            }
                            Err(e) if e.contains("не началось") => {
                                thread::sleep(Duration::from_millis(LOAD_RETRY_MS));
                            }
                            Err(e) => {
                                load_result = Some(Err(e));
                                break;
                            }
                        }
                    }

                    match load_result {
                        Some(Ok(pages)) => {
                            let owned: Vec<(u8, Vec<u8>)> = pages
                                .iter()
                                .map(|(p, v)| (*p, v.clone()))
                                .collect();
                            let slices: Vec<(u8, &[u8])> =
                                owned.iter().map(|(p, v)| (*p, v.as_slice())).collect();
                            let values =
                                decode_config_fields_pages(&config_fields, &slices);
                            let string_values =
                                decode_config_strings_pages(&config_fields, &slices);
                            let total_raw: usize = pages.values().map(|v| v.len()).sum();
                            *pages_store.lock().unwrap() = pages;
                            let mut loaded = ConfigSnapshot {
                                connected: true,
                                loaded: true,
                                read_only: false,
                                loading: false,
                                progress: 1.0,
                                bytes_loaded: total_bytes,
                                bytes_total: total_bytes,
                                raw_len: total_raw,
                                values: HashMap::new(),
                                string_values: HashMap::new(),
                                field_count,
                                last_error: None,
                                pin_usage: HashMap::new(),
                                checklist: ChecklistSnapshot::default(),
                            };
                            apply_decoded_values(&mut loaded, &ini_ctx, values, string_values);
                            snap = loaded;
                        }
                        Some(Err(e)) => {
                            if let Some((pages, prev)) = &backup {
                                *pages_store.lock().unwrap() = pages.clone();
                                snap = prev.clone();
                            } else {
                                snap.loaded = false;
                            }
                            snap.loading = false;
                            snap.progress = 0.0;
                            snap.last_error = Some(e);
                        }
                        None => {
                            if let Some((pages, prev)) = &backup {
                                *pages_store.lock().unwrap() = pages.clone();
                                snap = prev.clone();
                            } else {
                                snap.loaded = false;
                            }
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
        let ini_page = config_field_ini_page(field);
        let protocol_page = ini_page.saturating_sub(1) as u16;
        let offset = config_field_offset(field);
        let current = self.page_raw_ini(ini_page);
        let encoded = encode_config_value(field, value, &current)
            .ok_or_else(|| format!("cannot encode value for {name}"))?;

        if offset > u16::MAX as u32 {
            return Err(format!("offset {name} exceeds protocol limit"));
        }

        self.write_verified_chunk(
            session,
            protocol_page,
            ini.page_read_has_page_index,
            ini.page_chunk_write_has_page_index,
            offset as u16,
            &encoded,
            name,
        )?;

        {
            let mut pages = self.pages.lock().unwrap();
            let raw = pages.entry(ini_page).or_default();
            let off = offset as usize;
            if off + encoded.len() > raw.len() {
                raw.resize(off + encoded.len(), 0);
            }
            raw[off..off + encoded.len()].copy_from_slice(&encoded);
        }

        {
            let mut snap = self.snapshot.write().unwrap();
            snap.values.insert(name.to_string(), value);
            snap.pin_usage = build_pin_usage(&ini.config_fields, &snap.values);
            snap.last_error = None;
        }

        self.notify_live_ram_dirty();
        Ok(())
    }

    /// Запись строки в RAM ECU (`C`) + verify-read.
    pub fn write_string(
        &self,
        session: &EcuSession,
        name: &str,
        value: &str,
    ) -> Result<(), String> {
        self.ensure_ecu_writable()?;
        let ini = self.ini.lock().unwrap().clone();
        let field = ini
            .config_fields
            .get(name)
            .ok_or_else(|| format!("unknown config field: {name}"))?;
        let ConfigFieldKind::String(s) = field else {
            return Err(format!("{name} is not a string field"));
        };
        let ini_page = s.page;
        let protocol_page = ini_page.saturating_sub(1) as u16;
        let offset = s.offset;
        let encoded = encode_string_value(s, value)
            .ok_or_else(|| format!("cannot encode value for {name}"))?;

        if offset > u16::MAX as u32 {
            return Err(format!("offset {name} exceeds protocol limit"));
        }

        self.write_verified_chunk(
            session,
            protocol_page,
            ini.page_read_has_page_index,
            ini.page_chunk_write_has_page_index,
            offset as u16,
            &encoded,
            name,
        )?;

        {
            let mut pages = self.pages.lock().unwrap();
            let raw = pages.entry(ini_page).or_default();
            let off = offset as usize;
            if off + encoded.len() > raw.len() {
                raw.resize(off + encoded.len(), 0);
            }
            raw[off..off + encoded.len()].copy_from_slice(&encoded);
        }

        {
            let mut snap = self.snapshot.write().unwrap();
            snap.string_values.insert(name.to_string(), value.to_string());
            snap.last_error = None;
        }

        self.notify_live_ram_dirty();
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

    /// Перечитать все config-страницы INI с ECU (после burn / переподключения).
    pub fn reload_page_from_ecu(&self, session: &EcuSession) -> Result<(), String> {
        if !session.is_connected() {
            return Err("ECU не подключена".into());
        }

        let ini = self.ini.lock().unwrap().clone();
        let page_sizes = ecu_page_load_plan(&ini);
        let total_bytes: u32 = page_sizes.iter().sum();

        let outcome = session.run_without_output_poll(|session| {
            load_ecu_pages_once(session, &ini, |_, _| {})
        })?;
        let pages = outcome.pages;
        let total_raw: usize = pages.values().map(|v| v.len()).sum();
        *self.pages.lock().unwrap() = pages;
        let (values, string_values) = self.decode_snapshot_maps(&ini);

        let mut snap = self.snapshot.write().unwrap();
        snap.connected = true;
        snap.loaded = true;
        snap.read_only = false;
        snap.loading = false;
        snap.progress = 1.0;
        snap.bytes_loaded = total_bytes;
        snap.bytes_total = total_bytes.max(1);
        snap.raw_len = total_raw;
        apply_decoded_values(&mut snap, &ini, values, string_values);
        snap.field_count = ini.config_fields.len();
        snap.last_error = None;

        Ok(())
    }

    fn write_verified_chunk(
        &self,
        session: &EcuSession,
        protocol_page: u16,
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
                    protocol_page,
                    offset,
                    encoded,
                    page_chunk_write_has_page_index,
                )?;
                sleep(Duration::from_millis(INTER_WRITE_DELAY_MS));

                let read_back = link.read_config_chunk(
                    protocol_page,
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
        ConfigFieldKind::String(s) => s.offset,
    }
}

pub(crate) fn pages_from_project_ecu(
    ecu: &ProjectEcuConfig,
    ini: &IniContext,
) -> Result<ConfigPageStore, String> {
    let page_sizes = if ini.page_sizes.is_empty() {
        vec![ini.page_size]
    } else {
        ini.page_sizes.clone()
    };

    let mut pages = ConfigPageStore::new();

    let mut raw = B64
        .decode(&ecu.raw_page0_base64)
        .map_err(|e| format!("page1 base64: {e}"))?;
    pad_page_raw(&mut raw, page_sizes.first().copied().unwrap_or(ini.page_size));
    pages.insert(DEFAULT_INI_PAGE, raw);

    for (key, b64) in &ecu.config_pages_base64 {
        let ini_page: u8 = key
            .parse()
            .map_err(|_| format!("некорректный номер страницы в проекте: {key}"))?;
        if ini_page == DEFAULT_INI_PAGE {
            continue;
        }
        let idx = ini_page.saturating_sub(1) as usize;
        let want = page_sizes.get(idx).copied().unwrap_or(0);
        let mut raw = B64.decode(b64).map_err(|e| format!("page{ini_page} base64: {e}"))?;
        if want > 0 {
            pad_page_raw(&mut raw, want);
        }
        pages.insert(ini_page, raw);
    }

    Ok(pages)
}

fn pad_page_raw(raw: &mut Vec<u8>, page_size: u32) {
    let want = page_size as usize;
    if raw.len() < want {
        raw.resize(want, 0);
    }
}

/// Пустой снимок config по размерам страниц INI (offline-редактирование без ECU).
pub(crate) fn build_default_ecu_config(ini: &IniContext) -> ProjectEcuConfig {
    let page_sizes = if ini.page_sizes.is_empty() {
        vec![ini.page_size]
    } else {
        ini.page_sizes.clone()
    };

    let mut pages = ConfigPageStore::new();
    for (idx, &size) in page_sizes.iter().enumerate() {
        let ini_page = (idx as u8) + 1;
        pages.insert(ini_page, vec![0u8; size as usize]);
    }

    let owned: Vec<(u8, Vec<u8>)> = pages.iter().map(|(p, v)| (*p, v.clone())).collect();
    let slices: Vec<(u8, &[u8])> = owned.iter().map(|(p, v)| (*p, v.as_slice())).collect();
    let values = decode_config_fields_pages(&ini.config_fields, &slices);
    build_project_ecu_config(&pages, ini, values)
}

pub(crate) fn build_project_ecu_config(
    pages: &ConfigPageStore,
    ini: &IniContext,
    values: HashMap<String, f64>,
) -> ProjectEcuConfig {
    let page1 = pages
        .get(&DEFAULT_INI_PAGE)
        .cloned()
        .unwrap_or_default();
    let mut config_pages_base64 = HashMap::new();
    for (p, raw) in pages {
        if *p == DEFAULT_INI_PAGE {
            continue;
        }
        config_pages_base64.insert(p.to_string(), B64.encode(raw));
    }
    ProjectEcuConfig {
        captured_at_ms: 0,
        page_size: ini.page_size,
        raw_page0_base64: B64.encode(&page1),
        config_pages_base64,
        values,
    }
}
