use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};

use rusefi_ini::{decode_config_scalars, encode_scalar_value, ScalarField};
use rusefi_protocol::TS_PAGE_SETTINGS;
use serde::Serialize;
use serde_json::{json, Value};

use crate::session::EcuSession;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFieldInfo {
    pub name: String,
    pub units: Option<String>,
    pub ty: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSnapshot {
    pub connected: bool,
    pub loaded: bool,
    pub loading: bool,
    pub raw_len: usize,
    pub values: HashMap<String, f64>,
    pub field_count: usize,
    pub last_error: Option<String>,
}

impl ConfigSnapshot {
    pub fn disconnected(field_count: usize) -> Self {
        Self {
            connected: false,
            loaded: false,
            loading: false,
            raw_len: 0,
            values: HashMap::new(),
            field_count,
            last_error: None,
        }
    }
}

#[derive(Clone)]
pub struct ConfigContext {
    pub page_size: u32,
    pub blocking_factor: u16,
    pub config_scalars: HashMap<String, ScalarField>,
}

impl ConfigContext {
    pub fn from_ini_ctx(ini: &super::output_channels::IniContext) -> Self {
        Self {
            page_size: ini.page_size,
            blocking_factor: ini.blocking_factor,
            config_scalars: ini.config_scalars.clone(),
        }
    }

    pub fn list_fields(&self) -> Vec<ConfigFieldInfo> {
        self.config_scalars
            .iter()
            .map(|(name, f)| ConfigFieldInfo {
                name: name.clone(),
                units: if f.units.is_empty() {
                    None
                } else {
                    Some(f.units.clone())
                },
                ty: format!("{:?}", f.ty),
            })
            .collect()
    }
}

pub struct ConfigSource {
    ctx: Mutex<ConfigContext>,
    raw: Mutex<Vec<u8>>,
    snapshot: Arc<RwLock<ConfigSnapshot>>,
    loading: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl ConfigSource {
    pub fn new(ctx: ConfigContext) -> Self {
        let field_count = ctx.config_scalars.len();
        Self {
            ctx: Mutex::new(ctx),
            raw: Mutex::new(Vec::new()),
            snapshot: Arc::new(RwLock::new(ConfigSnapshot::disconnected(field_count))),
            loading: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> ConfigSnapshot {
        self.snapshot.read().unwrap().clone()
    }

    pub fn list_fields(&self) -> Vec<ConfigFieldInfo> {
        self.ctx.lock().unwrap().list_fields()
    }

    pub fn stop(&self) {
        self.loading.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        let field_count = self.ctx.lock().unwrap().config_scalars.len();
        *self.raw.lock().unwrap() = Vec::new();
        *self.snapshot.write().unwrap() = ConfigSnapshot::disconnected(field_count);
    }

    pub fn replace_ctx(&self, ctx: ConfigContext) {
        self.stop();
        let field_count = ctx.config_scalars.len();
        *self.ctx.lock().unwrap() = ctx;
        *self.snapshot.write().unwrap() = ConfigSnapshot::disconnected(field_count);
    }

    pub fn start_load<F>(&self, session: Arc<EcuSession>, on_done: F)
    where
        F: Fn(ConfigSnapshot) + Send + Sync + 'static,
    {
        self.stop();
        self.loading.store(true, Ordering::SeqCst);

        let ctx = self.ctx.lock().unwrap().clone();
        let field_count = ctx.config_scalars.len();
        {
            let mut snap = self.snapshot.write().unwrap();
            snap.connected = session.is_connected();
            snap.loading = true;
            snap.field_count = field_count;
            snap.last_error = None;
        }

        let loading = Arc::clone(&self.loading);
        let snapshot = Arc::clone(&self.snapshot);
        let raw_store = Arc::new(Mutex::new(Vec::new()));
        let raw_for_thread = Arc::clone(&raw_store);
        let on_done = Arc::new(on_done);

        let handle = thread::Builder::new()
            .name("rusefui-config-load".into())
            .spawn(move || {
                let mut snap = snapshot.read().unwrap().clone();
                snap.connected = session.is_connected();
                snap.loading = true;

                match session.with_link(|link| {
                    link.read_config_page_full(
                        TS_PAGE_SETTINGS,
                        ctx.page_size,
                        ctx.blocking_factor,
                    )
                }) {
                    Ok(bytes) => {
                        let values = decode_config_scalars(&ctx.config_scalars, &bytes);
                        *raw_for_thread.lock().unwrap() = bytes.clone();
                        snap = ConfigSnapshot {
                            connected: true,
                            loaded: true,
                            loading: false,
                            raw_len: bytes.len(),
                            values,
                            field_count,
                            last_error: None,
                        };
                    }
                    Err(e) => {
                        snap.loaded = false;
                        snap.loading = false;
                        snap.last_error = Some(e);
                    }
                }

                loading.store(false, Ordering::SeqCst);
                *snapshot.write().unwrap() = snap.clone();
                on_done(snap);
            })
            .expect("spawn config load thread");

        *self.raw.lock().unwrap() = Vec::new();
        *self.thread.lock().unwrap() = Some(handle);
        // raw_store will be copied back after thread - need shared raw in struct

        // Fix: use self.raw inside thread via cloning Arc - already raw_for_thread
        // After join we'd need to copy - better store Arc in ConfigSource

        *self.thread.lock().unwrap() = None; // BUG - I'm overwriting

        // Let me rewrite ConfigSource to use Arc<Mutex<Vec>> for raw as field
    }
}
