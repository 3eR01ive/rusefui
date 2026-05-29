use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Инстанс компонента в дереве layout (из YAML).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentInstance {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub props: Option<Value>,
    #[serde(default)]
    pub children: Vec<ComponentInstance>,
    #[serde(default)]
    pub nav_selectable: Option<bool>,
    #[serde(default)]
    pub nav_activatable: Option<bool>,
}

impl ComponentInstance {
    pub fn child_path(&self, parent_path: &str, index: usize) -> String {
        if let Some(id) = &self.id {
            format!("{parent_path}/{id}")
        } else {
            format!("{parent_path}/{index}")
        }
    }

    pub fn props_variant(&self) -> Option<&str> {
        self.props.as_ref()?.get("variant")?.as_str()
    }
}
