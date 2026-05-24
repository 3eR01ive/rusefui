use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Типы компонентов с реализацией логики в Rust (не каждый UI-тип).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicComponentType {
    Connection,
    Simulation,
}

impl LogicComponentType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "connection" => Some(Self::Connection),
            "simulation" => Some(Self::Simulation),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connection => "connection",
            Self::Simulation => "simulation",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentMeta {
    pub component_type: String,
    pub has_rust_logic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentAction {
    pub action: String,
    #[serde(default)]
    pub payload: Value,
}

/// Что shell делает с сессией ECU сразу после mount инстанса.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcuSyncOnMount {
    /// Загрузить config при необходимости, затем output poll.
    Full,
    /// Не трогать config; только output poll, если config уже в памяти.
    OutputPollIfConfigLoaded,
    /// Без автоматической синхронизации (компонент сам ходит в ECU).
    None,
}

/// Сложный компонент: состояние для UI готовится здесь, Vue только рисует JSON.
pub trait ComponentLogic: Send {
    fn meta(&self) -> ComponentMeta;

    /// Снимок состояния для Vue (реактивный рендер).
    fn state(&self) -> Value;

    /// Действие из UI (кнопка, смена select, mount).
    fn dispatch(&mut self, action: &str, payload: Value) -> Result<Value, String>;

    /// Политика синхронизации ECU после mount (см. `component_mount` в shell).
    fn ecu_sync_on_mount(&self) -> EcuSyncOnMount {
        EcuSyncOnMount::Full
    }
}

pub fn requires_rust_logic(component_type: &str) -> bool {
    LogicComponentType::from_str(component_type).is_some()
}
