use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use crate::component::{requires_rust_logic, ComponentLogic, EcuSyncOnMount, LogicComponentType};
use crate::components::config_table::ConfigTableLogic;
use crate::components::connection::ConnectionLogic;
use crate::components::dyno::DynoLogic;
use crate::components::ignition_table::IgnitionTableLogic;
use crate::components::knock::KnockLogic;
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
        payload: Value,
    ) -> Result<Value, String> {
        if !requires_rust_logic(component_type) {
            return Err(format!(
                "component type \"{component_type}\" has no Rust logic (presentation-only)"
            ));
        }

        if self.instances.contains_key(instance_id) {
            if matches!(
                LogicComponentType::from_str(component_type),
                Some(LogicComponentType::ConfigTable) | Some(LogicComponentType::IgnitionTable)
            ) && !payload.is_null()
            {
                return self.dispatch(instance_id, "set_bind", payload);
            }
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
            Some(LogicComponentType::Knock) => {
                Box::new(KnockLogic::new(Arc::clone(&self.session)))
            }
            Some(LogicComponentType::ConfigTable) => {
                Box::new(ConfigTableLogic::new(Arc::clone(&self.session)))
            }
            Some(LogicComponentType::IgnitionTable) => {
                Box::new(IgnitionTableLogic::new(Arc::clone(&self.session)))
            }
            None => {
                return Err(format!("unknown logic component: {component_type}"));
            }
        };

        self.instances.insert(instance_id.to_string(), logic);
        let mount_payload = if matches!(
            LogicComponentType::from_str(component_type),
            Some(LogicComponentType::ConfigTable) | Some(LogicComponentType::IgnitionTable)
        ) && !payload.is_null()
        {
            payload
        } else {
            Value::Null
        };
        self.dispatch(instance_id, "mount", mount_payload)
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
            LogicComponentType::Knock.as_str(),
            LogicComponentType::ConfigTable.as_str(),
        ]
    }

    /// Перечитать 2D-таблицы из текущего config (после `project_load` / смены INI).
    pub fn reload_config_tables(&mut self) -> Vec<(String, Value)> {
        let mut updates = Vec::new();
        for (id, logic) in &mut self.instances {
            if logic.meta().component_type != LogicComponentType::ConfigTable.as_str() {
                continue;
            }
            if let Ok(st) = logic.dispatch("reload", Value::Null) {
                updates.push((id.clone(), st));
            }
        }
        updates
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

    /// Knock scope FFT → компонент knock tuning.
    pub fn feed_knock_scope(
        &mut self,
        snap: &crate::sources::knock_scope::KnockScopeSnapshot,
    ) -> Vec<(String, Value)> {
        let mut updates = Vec::new();
        for (id, logic) in &mut self.instances {
            if let Some(state) = logic.feed_knock_scope(snap) {
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
