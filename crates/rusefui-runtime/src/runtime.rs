use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use crate::component::{requires_rust_logic, ComponentLogic, EcuSyncOnMount, LogicComponentType};
use crate::components::connection::ConnectionLogic;
use crate::components::dyno::DynoLogic;
use crate::components::simulation::SimulationLogic;
use crate::sources::output_channels::OutputSnapshot;
use crate::session::EcuSession;

pub struct ComponentRuntime {
    session: Arc<EcuSession>,
    instances: HashMap<String, Box<dyn ComponentLogic>>,
}

impl ComponentRuntime {
    pub fn new(session: Arc<EcuSession>) -> Self {
        Self {
            session,
            instances: HashMap::new(),
        }
    }

    pub fn session(&self) -> Arc<EcuSession> {
        Arc::clone(&self.session)
    }

    pub fn mount(
        &mut self,
        instance_id: &str,
        component_type: &str,
    ) -> Result<Value, String> {
        if !requires_rust_logic(component_type) {
            return Err(format!(
                "component type \"{component_type}\" has no Rust logic (presentation-only)"
            ));
        }

        if self.instances.contains_key(instance_id) {
            return Ok(self.state(instance_id)?);
        }

        let logic: Box<dyn ComponentLogic> = match LogicComponentType::from_str(component_type) {
            Some(LogicComponentType::Connection) => {
                Box::new(ConnectionLogic::new(Arc::clone(&self.session)))
            }
            Some(LogicComponentType::Simulation) => {
                Box::new(SimulationLogic::new(Arc::clone(&self.session)))
            }
            Some(LogicComponentType::Dyno) => {
                Box::new(DynoLogic::new(Arc::clone(&self.session)))
            }
            None => {
                return Err(format!("unknown logic component: {component_type}"));
            }
        };

        self.instances.insert(instance_id.to_string(), logic);
        self.dispatch(instance_id, "mount", Value::Null)
    }

    pub fn unmount(&mut self, instance_id: &str) {
        if let Some(logic) = self.instances.get_mut(instance_id) {
            let _ = logic.dispatch("unmount", Value::Null);
        }
        self.instances.remove(instance_id);
    }

    pub fn state(&self, instance_id: &str) -> Result<Value, String> {
        self.instances
            .get(instance_id)
            .map(|l| l.state())
            .ok_or_else(|| format!("unknown component instance: {instance_id}"))
    }

    pub fn dispatch(
        &mut self,
        instance_id: &str,
        action: &str,
        payload: Value,
    ) -> Result<Value, String> {
        let logic = self
            .instances
            .get_mut(instance_id)
            .ok_or_else(|| format!("unknown component instance: {instance_id}"))?;
        logic.dispatch(action, payload)
    }

    pub fn list_logic_types(&self) -> Vec<&'static str> {
        vec![
            LogicComponentType::Connection.as_str(),
            LogicComponentType::Simulation.as_str(),
            LogicComponentType::Dyno.as_str(),
        ]
    }

    /// Live output → компоненты с Rust-логикой (Virtual Dyno и т.д.).
    pub fn feed_output(&mut self, snap: &OutputSnapshot) -> Vec<(String, Value)> {
        let mut updates = Vec::new();
        for (id, logic) in &mut self.instances {
            if let Some(state) = logic.feed_output(snap) {
                updates.push((id.clone(), state));
            }
        }
        updates
    }

    pub fn ecu_sync_on_mount(&self, instance_id: &str) -> EcuSyncOnMount {
        self.instances
            .get(instance_id)
            .map(|l| l.ecu_sync_on_mount())
            .unwrap_or(EcuSyncOnMount::Full)
    }
}
