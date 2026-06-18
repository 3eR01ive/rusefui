use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::ignition_map::EngineParams;

use crate::component::{requires_rust_logic, ComponentLogic, EcuSyncOnMount, LogicComponentType};
use crate::components::command::CommandLogic;
use crate::components::config_table::ConfigTableLogic;
use crate::components::connection::ConnectionLogic;
use crate::components::dyno::DynoLogic;
use crate::components::ignition_table::IgnitionTableLogic;
use crate::components::ini_command_button::IniCommandButtonLogic;
use crate::components::knock::KnockLogic;
use crate::components::lua_script::LuaScriptLogic;
use crate::components::simulation::SimulationLogic;
use crate::sources::output_channels::OutputSnapshot;
use crate::session::EcuSession;

pub struct ComponentRuntime {
    session: Arc<EcuSession>,
    instances: HashMap<String, Box<dyn ComponentLogic>>,
    /// Сколько Vue-вью держат каждый instance_id (общая Rust-логика на несколько компонентов).
    mount_counts: HashMap<String, u32>,
    /// Общие для всех ignition-table: параметры автогенерации УОЗ.
    ignition_gen_params: Arc<Mutex<EngineParams>>,
}

impl ComponentRuntime {
    pub fn new(session: Arc<EcuSession>) -> Self {
        Self {
            session,
            instances: HashMap::new(),
            mount_counts: HashMap::new(),
            ignition_gen_params: Arc::new(Mutex::new(EngineParams::default())),
        }
    }

    pub fn ignition_gen_params(&self) -> EngineParams {
        self.ignition_gen_params.lock().unwrap().clone()
    }

    pub fn set_ignition_gen_params(&self, params: EngineParams) {
        *self.ignition_gen_params.lock().unwrap() = params;
    }

    /// Сброс UI-состояния компонентов при смене проекта (не ECU-сессия).
    pub fn reset_workspace(&self) {
        *self.ignition_gen_params.lock().unwrap() = EngineParams::default();
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
            // Ещё одно Vue-вью на ту же логику — считаем ссылки.
            *self.mount_counts.entry(instance_id.to_string()).or_insert(1) += 1;
            if !payload.is_null() {
                let remount_action = match LogicComponentType::from_str(component_type) {
                    Some(LogicComponentType::ConfigTable)
                    | Some(LogicComponentType::IgnitionTable) => "set_bind",
                    _ => "mount",
                };
                return self.dispatch(instance_id, remount_action, payload);
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
                Box::new(IgnitionTableLogic::new(
                    Arc::clone(&self.session),
                    Arc::clone(&self.ignition_gen_params),
                ))
            }
            Some(LogicComponentType::Command) => {
                Box::new(CommandLogic::new(Arc::clone(&self.session)))
            }
            Some(LogicComponentType::LuaScript) => {
                Box::new(LuaScriptLogic::new(Arc::clone(&self.session)))
            }
            Some(LogicComponentType::IniCommandButton) => {
                Box::new(IniCommandButtonLogic::new(Arc::clone(&self.session)))
            }
            None => {
                return Err(format!("unknown logic component: {component_type}"));
            }
        };

        self.instances.insert(instance_id.to_string(), logic);
        self.mount_counts.insert(instance_id.to_string(), 1);
        let mount_payload = if payload.is_null() {
            Value::Null
        } else {
            payload
        };
        self.dispatch(instance_id, "mount", mount_payload)
    }

    pub fn unmount(&mut self, instance_id: &str) {
        // Несколько вью могут делить одну логику — рвём только когда отписалось последнее.
        let remaining = match self.mount_counts.get_mut(instance_id) {
            Some(c) => {
                *c = c.saturating_sub(1);
                *c
            }
            None => 0,
        };
        if remaining > 0 {
            return;
        }
        self.mount_counts.remove(instance_id);
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
            LogicComponentType::Command.as_str(),
            LogicComponentType::LuaScript.as_str(),
            LogicComponentType::IniCommandButton.as_str(),
        ]
    }

    pub fn instance_component_type(&self, instance_id: &str) -> Option<String> {
        self.instances
            .get(instance_id)
            .map(|l| l.meta().component_type.clone())
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

    /// Состояние других ignition-table (общие params в сессии — обновить UI после `set_params`).
    pub fn peer_ignition_table_states(&self, except_id: &str) -> Vec<(String, Value)> {
        self.instances
            .iter()
            .filter(|(id, logic)| {
                id.as_str() != except_id
                    && logic.meta().component_type == LogicComponentType::IgnitionTable.as_str()
            })
            .map(|(id, logic)| (id.clone(), logic.state()))
            .collect()
    }

}
