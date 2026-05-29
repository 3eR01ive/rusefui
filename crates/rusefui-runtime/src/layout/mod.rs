//! Layout YAML и навигация. Метаданные — только на инстансе в panel YAML.

mod instance;
mod nav;
mod type_catalog;

pub use instance::ComponentInstance;
pub use nav::{
    build_nav_paths, is_filter_nav_path, nav_region, NavArrowKey, NavExtension, NavMode,
    NavPathEntry, NavRegion, NavSnapshot, WorkspaceNav,
};
pub use type_catalog::{is_container, resolve_nav_activatable, resolve_nav_selectable};
