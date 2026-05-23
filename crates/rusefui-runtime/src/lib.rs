//! Логика компонентов и подготовка данных на стороне Rust.
//!
//! Vue отвечает только за отрисовку `state` и отправку `dispatch(action)`.

mod component;
mod runtime;
pub mod components;
pub mod session;
pub mod sources;

pub use component::{ComponentAction, ComponentLogic, ComponentMeta, LogicComponentType};
pub use runtime::ComponentRuntime;
pub use session::EcuSession;
pub use sources::output_channels::{OutputSnapshot, DEFAULT_OUTPUT_BLOCK_SIZE};
