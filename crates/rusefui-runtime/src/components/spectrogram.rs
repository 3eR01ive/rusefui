use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};
use rusefi_protocol::KNOCK_SCOPE_BUFFER_BYTES;

use crate::component::{ComponentLogic, ComponentMeta, EcuSyncOnMount, LogicComponentType};
use crate::session::EcuSession;
use crate::sources::output_channels::OutputSnapshot;

/// Частота KNOCK_ADC на Proteus F4/F7 (см. `knock_config.h` / `docs/spectrogram.md`).
const KNOCK_ADC_HZ: f64 = 218_750.0;

const READY_FIELD: &str = "knockScopeReady";

const MSG_TUNE_HINT: &str = "В tune: enableKnockScope = yes, прошивка с -DKNOCK_SCOPE=TRUE. \
    После подключения ECU scope включится автоматически.";
const MSG_WAITING: &str = "Команда scope на ECU отправлена. Ждём knockScopeReady…";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SpectrogramViewState {
    connected: bool,
    scope_enabled: bool,
    ready_field_present: bool,
    knock_scope_ready: bool,
    capture_count: u64,
    sample_count: usize,
    samples: Vec<f32>,
    sample_min: f32,
    sample_max: f32,
    last_byte_len: usize,
    sample_rate_hz: f64,
    buffer_duration_ms: f64,
    message: Option<String>,
}

pub struct SpectrogramLogic {
    session: Arc<EcuSession>,
    scope_enabled: bool,
    capture_count: u64,
    samples: Vec<f32>,
    sample_min: f32,
    sample_max: f32,
    last_byte_len: usize,
    ready_field_present: bool,
    knock_scope_ready: bool,
    message: Option<String>,
    /// `l`+ENABLE уже ушёл на ECU (успешно).
    scope_ecu_armed: bool,
    dirty: bool,
}

enum ScopeEnableResult {
    Ok,
    NotConnected,
    PortBusy,
    Err(String),
}

impl SpectrogramLogic {
    pub fn new(session: Arc<EcuSession>) -> Self {
        Self {
            session,
            scope_enabled: false,
            capture_count: 0,
            samples: Vec::new(),
            sample_min: 0.0,
            sample_max: 0.0,
            last_byte_len: 0,
            ready_field_present: false,
            knock_scope_ready: false,
            message: None,
            scope_ecu_armed: false,
            dirty: true,
        }
    }

    fn parse_samples(bytes: &[u8]) -> (Vec<f32>, f32, f32) {
        let mut out = Vec::with_capacity(bytes.len() / 2);
        let mut min_v = f32::MAX;
        let mut max_v = f32::MIN;
        for chunk in bytes.chunks_exact(2) {
            let raw = u16::from_le_bytes([chunk[0], chunk[1]]);
            let v = (raw & 0x0FFF) as f32;
            out.push(v);
            min_v = min_v.min(v);
            max_v = max_v.max(v);
        }
        if out.is_empty() {
            min_v = 0.0;
            max_v = 0.0;
        }
        (out, min_v, max_v)
    }

    fn buffer_duration_ms(sample_count: usize) -> f64 {
        if sample_count == 0 {
            0.0
        } else {
            sample_count as f64 / KNOCK_ADC_HZ * 1000.0
        }
    }

    fn view_state(&self) -> SpectrogramViewState {
        SpectrogramViewState {
            connected: self.session.is_connected(),
            scope_enabled: self.scope_enabled,
            ready_field_present: self.ready_field_present,
            knock_scope_ready: self.knock_scope_ready,
            capture_count: self.capture_count,
            sample_count: self.samples.len(),
            samples: self.samples.clone(),
            sample_min: self.sample_min,
            sample_max: self.sample_max,
            last_byte_len: self.last_byte_len,
            sample_rate_hz: KNOCK_ADC_HZ,
            buffer_duration_ms: Self::buffer_duration_ms(self.samples.len()),
            message: self.message.clone(),
        }
    }

    fn to_json(&self) -> Value {
        serde_json::to_value(self.view_state()).unwrap_or(json!({}))
    }

    fn take_dirty_json(&mut self) -> Option<Value> {
        if self.dirty {
            self.dirty = false;
            Some(self.to_json())
        } else {
            None
        }
    }

    fn send_enable_scope_on_ecu(&self) -> ScopeEnableResult {
        if !self.session.is_connected() {
            return ScopeEnableResult::NotConnected;
        }
        match self
            .session
            .try_with_link(|link| link.set_knock_scope_enabled(true))
        {
            Some(Ok(())) => ScopeEnableResult::Ok,
            Some(Err(e)) => ScopeEnableResult::Err(format!("Не удалось включить knock scope: {e}")),
            None => ScopeEnableResult::PortBusy,
        }
    }

    /// Повторяет `l`+ENABLE после подключения ECU или если порт был занят.
    fn arm_scope_on_ecu_if_needed(&mut self) {
        if !self.scope_enabled || self.scope_ecu_armed {
            return;
        }
        match self.send_enable_scope_on_ecu() {
            ScopeEnableResult::Ok => {
                self.scope_ecu_armed = true;
                if self.capture_count == 0 {
                    self.message = Some(MSG_WAITING.into());
                }
                self.dirty = true;
            }
            ScopeEnableResult::NotConnected => {
                self.message = Some(format!(
                    "ECU не подключена. {MSG_TUNE_HINT}"
                ));
                self.dirty = true;
            }
            ScopeEnableResult::PortBusy => {}
            ScopeEnableResult::Err(e) => {
                self.message = Some(e);
                self.dirty = true;
            }
        }
    }

    fn disable_scope_on_ecu(&self) {
        if !self.session.is_connected() {
            return;
        }
        let _ = self
            .session
            .try_with_link(|link| link.set_knock_scope_enabled(false));
    }

    fn try_read_buffer(&mut self) {
        let Some(result) = self
            .session
            .try_with_link(|link| link.read_knock_scope_buffer())
        else {
            return;
        };

        match result {
            Ok(bytes) => {
                self.last_byte_len = bytes.len();
                let (samples, min_v, max_v) = Self::parse_samples(&bytes);
                if samples.is_empty() {
                    self.message = Some(format!(
                        "Пустой ответ (ожидалось до {KNOCK_SCOPE_BUFFER_BYTES} байт)"
                    ));
                } else {
                    self.samples = samples;
                    self.sample_min = min_v;
                    self.sample_max = max_v;
                    self.capture_count = self.capture_count.saturating_add(1);
                    self.message = None;
                }
                self.dirty = true;
            }
            Err(e) => {
                self.message = Some(format!("Чтение knock scope: {e}"));
                self.dirty = true;
            }
        }
    }
}

impl ComponentLogic for SpectrogramLogic {
    fn meta(&self) -> ComponentMeta {
        ComponentMeta {
            component_type: LogicComponentType::Spectrogram.as_str().to_string(),
            has_rust_logic: true,
        }
    }

    fn ecu_sync_on_mount(&self) -> EcuSyncOnMount {
        EcuSyncOnMount::OutputPollIfConfigLoaded
    }

    fn state(&self) -> Value {
        self.to_json()
    }

    fn dispatch(&mut self, action: &str, _payload: Value) -> Result<Value, String> {
        match action {
            "mount" => {
                self.scope_enabled = true;
                self.scope_ecu_armed = false;
                self.message = Some(MSG_TUNE_HINT.into());
                self.arm_scope_on_ecu_if_needed();
                self.dirty = true;
                Ok(self.to_json())
            }
            "unmount" => {
                self.scope_enabled = false;
                self.scope_ecu_armed = false;
                self.disable_scope_on_ecu();
                self.dirty = true;
                Ok(self.to_json())
            }
            "enable_scope" => {
                self.scope_enabled = true;
                self.scope_ecu_armed = false;
                self.arm_scope_on_ecu_if_needed();
                self.dirty = true;
                Ok(self.to_json())
            }
            "disable_scope" => {
                self.scope_enabled = false;
                self.scope_ecu_armed = false;
                self.disable_scope_on_ecu();
                self.dirty = true;
                Ok(self.to_json())
            }
            _ => Err(format!("unknown action: {action}")),
        }
    }

    fn feed_output(&mut self, snap: &OutputSnapshot) -> Option<Value> {
        self.arm_scope_on_ecu_if_needed();

        self.ready_field_present = snap.values.contains_key(READY_FIELD);
        self.knock_scope_ready = snap
            .values
            .get(READY_FIELD)
            .copied()
            .map(|v| v >= 0.5)
            .unwrap_or(false);

        if !self.ready_field_present
            && self.scope_ecu_armed
            && self.capture_count == 0
            && self.message.as_deref() == Some(MSG_WAITING)
        {
            self.message = Some(format!(
                "{MSG_WAITING} Поле knockScopeReady нет в INI — нужен свежий INI с knock_scope_host."
            ));
            self.dirty = true;
        }

        if self.scope_enabled && self.knock_scope_ready {
            self.try_read_buffer();
        }

        self.take_dirty_json()
    }
}
