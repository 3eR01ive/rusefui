//! Virtual Dyno — порт алгоритма из virtualdyno-c++.

mod view;

pub use view::{
    dyno_config_from_values, DynoConfig, DynoRunOptions, DynoRunPoint, DynoView,
    DEFAULT_DYNO_CONFIG,
};
