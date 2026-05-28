use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};

use crate::component::{ComponentLogic, ComponentMeta, EcuSyncOnMount, LogicComponentType};
use crate::config_table_grid::{
    build_grid_view, interpolate_rect, nudge_rect_values, NavDir, TableGridState,
};
use crate::session::EcuSession;

const DEFAULT_NUDGE_STEP: f64 = 0.1;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigTableViewState {
    title: String,
    x_label: String,
    y_label: String,
    x_values: Vec<f64>,
    y_values: Vec<f64>,
    grid: crate::config_table_grid::TableGridView,
    can_edit: bool,
    loading: bool,
    saving: bool,
    status_text: String,
    local_error: Option<String>,
}

pub struct ConfigTableLogic {
    session: Arc<EcuSession>,
    title: String,
    x_label: String,
    y_label: String,
    x_field: Option<String>,
    y_field: Option<String>,
    z_field: Option<String>,
    x_values: Vec<f64>,
    y_values: Vec<f64>,
    grid: TableGridState,
    loading: bool,
    saving: bool,
    local_error: Option<String>,
    edit_buffer: String,
    nudge_step: f64,
}

impl ConfigTableLogic {
    pub fn new(session: Arc<EcuSession>) -> Self {
        Self {
            session,
            title: String::new(),
            x_label: "X".into(),
            y_label: "Y".into(),
            x_field: None,
            y_field: None,
            z_field: None,
            x_values: Vec::new(),
            y_values: Vec::new(),
            grid: TableGridState::new(1, 1, vec![0.0]),
            loading: false,
            saving: false,
            local_error: None,
            edit_buffer: String::new(),
            nudge_step: DEFAULT_NUDGE_STEP,
        }
    }

    fn config(&self) -> &crate::sources::config::ConfigSource {
        self.session.config()
    }

    fn can_edit(&self) -> bool {
        let snap = self.config().snapshot();
        if !snap.loaded || snap.loading || self.z_field.is_none() {
            return false;
        }
        let live = self.session.is_connected() && !snap.read_only;
        let project = snap.read_only;
        live || project
    }

    fn status_text(&self) -> String {
        if let Some(e) = &self.local_error {
            return e.clone();
        }
        if self.saving {
            return "сохранение…".into();
        }
        if self.loading {
            return "загрузка…".into();
        }
        let snap = self.config().snapshot();
        if snap.loading {
            return "загрузка config…".into();
        }
        if !snap.loaded {
            return "ожидание config…".into();
        }
        if !self.session.is_connected() && !snap.read_only {
            return "нет подключения".into();
        }
        "таблица".into()
    }

    fn infer_dims(x_len: usize, y_len: usize, z_len: usize) -> (usize, usize) {
        if y_len > 0 {
            let cols = if x_len > 0 { x_len } else { 1 };
            let rows = y_len;
            (rows, cols)
        } else if x_len > 0 {
            let cols = x_len;
            let rows = if z_len > 0 {
                z_len.div_ceil(cols).max(1)
            } else {
                1
            };
            (rows, cols)
        } else {
            let side = (z_len as f64).sqrt().ceil() as usize;
            (side.max(1), side.max(1))
        }
    }

    fn table_dims(
        &self,
        z_name: &str,
        x_len: usize,
        y_len: usize,
        z_len: usize,
    ) -> (usize, usize) {
        if z_len == 0 {
            return (1, 1);
        }
        if x_len > 0 || y_len > 0 {
            return Self::infer_dims(x_len, y_len, z_len);
        }
        if let Some((rows, cols)) = self.config().get_array_matrix_size(z_name) {
            if z_len == rows * cols {
                return (rows, cols);
            }
        }
        Self::infer_dims(x_len, y_len, z_len)
    }

    fn y_values_for_display(&self) -> Vec<f64> {
        if self.y_values.is_empty() {
            return Vec::new();
        }
        if !self.grid.y_reversed {
            return self.y_values.clone();
        }
        (0..self.grid.rows)
            .map(|vr| {
                let sr = self.grid.storage_row(vr);
                self.y_values.get(sr).copied().unwrap_or(f64::NAN)
            })
            .collect()
    }

    fn reload(&mut self) -> Result<(), String> {
        let z_name = match self.z_field.as_deref() {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => return Ok(()),
        };
        let snap = self.config().snapshot();
        if !snap.loaded {
            return Ok(());
        }

        self.loading = true;
        self.local_error = None;

        let result = self.reload_inner(&z_name);

        self.loading = false;
        result
    }

    fn reload_inner(&mut self, z_name: &str) -> Result<(), String> {
        if let Some(ref name) = self.x_field {
            if !name.is_empty() {
                self.x_values = self.config().get_array(name)?;
            } else {
                self.x_values.clear();
            }
        } else {
            self.x_values.clear();
        }

        if let Some(ref name) = self.y_field {
            if !name.is_empty() {
                self.y_values = self.config().get_array(name)?;
            } else {
                self.y_values.clear();
            }
        } else {
            self.y_values.clear();
        }

        let z_values = self.config().get_array(z_name)?;
        let (rows, cols) = self.table_dims(
            z_name,
            self.x_values.len(),
            self.y_values.len(),
            z_values.len(),
        );
        let mut grid = TableGridState::new(rows, cols, z_values);
        let cr = grid.cursor.row.min(rows.saturating_sub(1));
        let cc = grid.cursor.col.min(cols.saturating_sub(1));
        grid.select_cell(cr, cc);
        self.grid = grid;
        Ok(())
    }

    fn apply_updates(&mut self, updates: &[(usize, f64)]) -> Result<(), String> {
        if updates.is_empty() {
            return Ok(());
        }
        let z_name = self
            .z_field
            .as_deref()
            .ok_or("zBins не задан")?
            .to_string();

        self.saving = true;
        self.local_error = None;

        let result = (|| -> Result<(), String> {
            let snap = self.config().snapshot();
            let live = self.session.is_connected() && snap.loaded && !snap.read_only;
            if live {
                self.config()
                    .write_array_values(&self.session, &z_name, updates)?;
            } else if snap.loaded && snap.read_only {
                self.config()
                    .set_array_values_local(&z_name, updates)?;
            } else {
                return Err(
                    "Нет config для редактирования — откройте проект или подключите ECU".into(),
                );
            }
            for &(idx, v) in updates {
                if idx < self.grid.values.len() {
                    self.grid.values[idx] = v;
                }
            }
            Ok(())
        })();

        self.saving = false;
        if let Err(e) = &result {
            self.local_error = Some(e.clone());
            let _ = self.reload();
        }
        result
    }

    fn handle_keydown(&mut self, payload: &Value) -> Result<(), String> {
        let key = payload
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let shift = payload.get("shift").and_then(|v| v.as_bool()).unwrap_or(false);
        let ctrl = payload.get("ctrl").and_then(|v| v.as_bool()).unwrap_or(false);

        if let Some(dir) = NavDir::from_arrow(key) {
            if ctrl && matches!(dir, NavDir::Up | NavDir::Down) {
                if !self.can_edit() {
                    return Ok(());
                }
                let delta = if dir == NavDir::Up {
                    self.nudge_step
                } else {
                    -self.nudge_step
                };
                let rect = self.grid.selection();
                let updates = nudge_rect_values(&self.grid, rect, delta);
                return self.apply_updates(&updates);
            }

            if shift {
                self.grid.extend_selection(dir);
            } else {
                self.grid.move_cursor(dir);
            }
            self.edit_buffer.clear();
            return Ok(());
        }
        Ok(())
    }

    fn apply_value_to_selection(&mut self, value: f64) -> Result<(), String> {
        if !self.can_edit() {
            return Ok(());
        }
        let rect = self.grid.selection();
        let mut updates = Vec::new();
        for r in rect.r0..=rect.r1 {
            for c in rect.c0..=rect.c1 {
                let idx = self.grid.index_visual(r, c);
                let current = self.grid.values.get(idx).copied().unwrap_or(0.0);
                if (current - value).abs() >= 1e-9 {
                    updates.push((idx, value));
                }
            }
        }
        self.apply_updates(&updates)
    }

    fn handle_type_key(&mut self, payload: &Value) -> Result<(), String> {
        let kind = payload.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        match kind {
            "char" => {
                let ch = payload
                    .get("ch")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.chars().next())
                    .ok_or("пустой символ ввода")?;
                match ch {
                    '0'..='9' => self.edit_buffer.push(ch),
                    '.' => {
                        if !self.edit_buffer.contains('.') {
                            if self.edit_buffer.is_empty() {
                                self.edit_buffer.push('0');
                            }
                            self.edit_buffer.push('.');
                        }
                    }
                    '-' => {
                        if self.edit_buffer.is_empty() {
                            self.edit_buffer.push('-');
                        }
                    }
                    _ => {}
                }
            }
            "backspace" => {
                self.edit_buffer.pop();
            }
            "commit" | "cancel" => {
                self.edit_buffer.clear();
                return Ok(());
            }
            _ => return Ok(()),
        }

        let parsed = self
            .edit_buffer
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite());
        if let Some(v) = parsed {
            self.apply_value_to_selection(v)?;
        }
        Ok(())
    }

    fn set_selection_value(&mut self, row: usize, col: usize, value: f64) -> Result<(), String> {
        if !self.can_edit() {
            return Ok(());
        }
        if !value.is_finite() {
            return Err("некорректное число".into());
        }

        let mut rect = self.grid.selection();
        if !rect.contains(row, col) {
            self.grid.select_cell(row, col);
            rect = self.grid.selection();
        }
        let mut updates = Vec::new();
        for r in rect.r0..=rect.r1 {
            for c in rect.c0..=rect.c1 {
                let idx = self.grid.index_visual(r, c);
                let current = self.grid.values.get(idx).copied().unwrap_or(0.0);
                if (current - value).abs() >= 1e-9 {
                    updates.push((idx, value));
                }
            }
        }
        self.apply_updates(&updates)
    }

    fn bind_str(payload: &Value, key: &str) -> Option<String> {
        payload
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    fn bind_f64(payload: &Value, key: &str) -> Option<f64> {
        let raw = payload.get(key)?;
        if let Some(v) = raw.as_f64() {
            return Some(v);
        }
        if let Some(s) = raw.as_str() {
            return s.trim().replace(',', ".").parse::<f64>().ok();
        }
        None
    }

    fn set_bind_from_payload(&mut self, payload: &Value) {
        if let Some(v) = Self::bind_str(payload, "title") {
            self.title = v;
        }
        if let Some(v) = Self::bind_str(payload, "xLabel") {
            self.x_label = v;
        }
        if let Some(v) = Self::bind_str(payload, "yLabel") {
            self.y_label = v;
        }
        if payload.get("xBins").is_some() {
            self.x_field = Self::bind_str(payload, "xBins");
        }
        if payload.get("yBins").is_some() {
            self.y_field = Self::bind_str(payload, "yBins");
        }
        if payload.get("zBins").is_some() {
            self.z_field = Self::bind_str(payload, "zBins");
        }
        if payload.get("nudgeStep").is_some() {
            self.nudge_step = Self::bind_f64(payload, "nudgeStep")
                .filter(|v| v.is_finite() && *v > 0.0)
                .unwrap_or(DEFAULT_NUDGE_STEP);
        }
    }

    fn view_state(&self) -> ConfigTableViewState {
        ConfigTableViewState {
            title: self.title.clone(),
            x_label: self.x_label.clone(),
            y_label: self.y_label.clone(),
            x_values: self.x_values.clone(),
            y_values: self.y_values_for_display(),
            grid: build_grid_view(&self.grid),
            can_edit: self.can_edit(),
            loading: self.loading,
            saving: self.saving,
            status_text: self.status_text(),
            local_error: self.local_error.clone(),
        }
    }

    fn to_json(&self) -> Value {
        serde_json::to_value(self.view_state()).unwrap_or(json!({}))
    }
}

impl ComponentLogic for ConfigTableLogic {
    fn meta(&self) -> ComponentMeta {
        ComponentMeta {
            component_type: LogicComponentType::ConfigTable.as_str().to_string(),
            has_rust_logic: true,
        }
    }

    fn state(&self) -> Value {
        self.to_json()
    }

    fn dispatch(&mut self, action: &str, payload: Value) -> Result<Value, String> {
        match action {
            "mount" => {
                if !payload.is_null() {
                    self.set_bind_from_payload(&payload);
                }
                self.reload()?;
            }
            "reload" => {
                self.reload()?;
            }
            "set_bind" => {
                self.set_bind_from_payload(&payload);
                self.reload()?;
            }
            "keydown" => {
                self.handle_keydown(&payload)?;
            }
            "interpolate" => {
                if !self.can_edit() {
                    return Ok(self.to_json());
                }
                let rect = self.grid.selection();
                let updates = interpolate_rect(&self.grid, rect);
                self.apply_updates(&updates)?;
            }
            "select_cell" => {
                let row = payload.get("row").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let col = payload.get("col").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let extend = payload
                    .get("extend")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if extend {
                    self.grid.cursor = self.grid.clamp_pos(row as isize, col as isize);
                } else {
                    self.grid.select_cell(row, col);
                }
                self.edit_buffer.clear();
            }
            "commit_cell" => {
                if !self.can_edit() {
                    return Ok(self.to_json());
                }
                let row = payload.get("row").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let col = payload.get("col").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let raw = payload
                    .get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let parsed: f64 = raw
                    .trim()
                    .replace(',', ".")
                    .parse()
                    .map_err(|_| "некорректное число".to_string())?;
                if !parsed.is_finite() {
                    return Err("некорректное число".into());
                }
                let idx = self.grid.index_visual(row, col);
                let current = self.grid.values.get(idx).copied().unwrap_or(0.0);
                if (current - parsed).abs() < 1e-9 {
                    return Ok(self.to_json());
                }
                self.apply_updates(&[(idx, parsed)])?;
            }
            "set_selection_value" => {
                let row = payload.get("row").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let col = payload.get("col").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let value = payload
                    .get("value")
                    .and_then(|v| v.as_f64())
                    .ok_or("некорректное число")?;
                self.set_selection_value(row, col, value)?;
            }
            "type_key" => {
                self.handle_type_key(&payload)?;
            }
            _ => {}
        }
        Ok(self.to_json())
    }

    fn ecu_sync_on_mount(&self) -> EcuSyncOnMount {
        EcuSyncOnMount::OutputPollIfConfigLoaded
    }
}
