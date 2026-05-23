//! Логика компонентов и подготовка данных на стороне Rust.
//!
//! Vue отвечает только за отрисовку `state` и отправку `dispatch(action)`.

mod component;
mod runtime;
pub mod components;

pub use component::{ComponentAction, ComponentLogic, ComponentMeta, LogicComponentType};
pub use runtime::ComponentRuntime;
