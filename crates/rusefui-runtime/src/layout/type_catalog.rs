//! Nav/container — только из YAML инстанса, без каталога типов.

use super::instance::ComponentInstance;

pub fn is_container(instance: &ComponentInstance) -> bool {
    !instance.children.is_empty()
}

pub fn resolve_nav_selectable(instance: &ComponentInstance) -> bool {
    if let Some(v) = instance.nav_selectable {
        return v;
    }
    if instance.type_ == "text" && instance.props_variant() == Some("hint") {
        return false;
    }
    true
}

pub fn resolve_nav_activatable(instance: &ComponentInstance) -> bool {
    instance.nav_activatable.unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hint_text_from_props() {
        let inst = ComponentInstance {
            type_: "text".into(),
            props: Some(json!({ "variant": "hint" })),
            ..Default::default()
        };
        assert!(!resolve_nav_selectable(&inst));
    }

    #[test]
    fn explicit_instance_override() {
        let inst = ComponentInstance {
            type_: "text".into(),
            props: Some(json!({ "variant": "hint" })),
            nav_selectable: Some(true),
            ..Default::default()
        };
        assert!(resolve_nav_selectable(&inst));
    }
}
