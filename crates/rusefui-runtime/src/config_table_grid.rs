//! 2D-таблица калибровки: прямоугольное выделение, heatmap, билинейная интерполяция, навигация.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPos {
    pub row: usize,
    pub col: usize,
}

/// Нормализованный прямоугольник включительно (r0≤r1, c0≤c1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridRect {
    pub r0: usize,
    pub r1: usize,
    pub c0: usize,
    pub c1: usize,
}

impl GridRect {
    pub fn from_anchor_focus(anchor: CellPos, focus: CellPos) -> Self {
        Self {
            r0: anchor.row.min(focus.row),
            r1: anchor.row.max(focus.row),
            c0: anchor.col.min(focus.col),
            c1: anchor.col.max(focus.col),
        }
    }

    pub fn single(pos: CellPos) -> Self {
        Self {
            r0: pos.row,
            r1: pos.row,
            c0: pos.col,
            c1: pos.col,
        }
    }

    pub fn contains(&self, row: usize, col: usize) -> bool {
        row >= self.r0 && row <= self.r1 && col >= self.c0 && col <= self.c1
    }

    pub fn is_corner(&self, row: usize, col: usize) -> bool {
        (row == self.r0 || row == self.r1) && (col == self.c0 || col == self.c1)
    }

    pub fn rows(&self) -> usize {
        self.r1 - self.r0 + 1
    }

    pub fn cols(&self) -> usize {
        self.c1 - self.c0 + 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavDir {
    Up,
    Down,
    Left,
    Right,
}

impl NavDir {
    pub fn from_arrow(key: &str) -> Option<Self> {
        match key {
            "ArrowUp" => Some(Self::Up),
            "ArrowDown" => Some(Self::Down),
            "ArrowLeft" => Some(Self::Left),
            "ArrowRight" => Some(Self::Right),
            _ => None,
        }
    }

    fn delta(self) -> (isize, isize) {
        match self {
            Self::Up => (-1, 0),
            Self::Down => (1, 0),
            Self::Left => (0, -1),
            Self::Right => (0, 1),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TableGridState {
    pub rows: usize,
    pub cols: usize,
    pub values: Vec<f64>,
    pub cursor: CellPos,
    pub anchor: CellPos,
    /// Как TunerStudio: низкая нагрузка внизу, высокая сверху (курсор в visual-координатах).
    pub y_reversed: bool,
}

impl TableGridState {
    pub fn new(rows: usize, cols: usize, values: Vec<f64>) -> Self {
        Self::new_with_y_reversed(rows, cols, values, true)
    }

    pub fn new_with_y_reversed(
        rows: usize,
        cols: usize,
        values: Vec<f64>,
        y_reversed: bool,
    ) -> Self {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let need = rows * cols;
        let mut values = values;
        values.resize(need, 0.0);
        let cursor = CellPos { row: 0, col: 0 };
        Self {
            rows,
            cols,
            values,
            cursor,
            anchor: cursor,
            y_reversed,
        }
    }

    /// Строка в RAM ECU (низкая Y-нагрузка = 0).
    pub fn storage_row(&self, visual_row: usize) -> usize {
        if self.y_reversed {
            self.rows.saturating_sub(1).saturating_sub(visual_row)
        } else {
            visual_row
        }
    }

    pub fn visual_row(&self, storage_row: usize) -> usize {
        if self.y_reversed {
            self.rows.saturating_sub(1).saturating_sub(storage_row)
        } else {
            storage_row
        }
    }

    pub fn selection(&self) -> GridRect {
        GridRect::from_anchor_focus(self.anchor, self.cursor)
    }

    pub fn index_storage(&self, storage_row: usize, col: usize) -> usize {
        storage_row * self.cols + col
    }

    pub fn index_visual(&self, visual_row: usize, col: usize) -> usize {
        self.index_storage(self.storage_row(visual_row), col)
    }

    pub fn value_at_visual(&self, visual_row: usize, col: usize) -> f64 {
        self.values[self.index_visual(visual_row, col)]
    }

    pub fn set_value_at_visual(&mut self, visual_row: usize, col: usize, v: f64) {
        let i = self.index_visual(visual_row, col);
        if i < self.values.len() {
            self.values[i] = v;
        }
    }

    pub fn clamp_pos(&self, row: isize, col: isize) -> CellPos {
        CellPos {
            row: row.clamp(0, self.rows as isize - 1) as usize,
            col: col.clamp(0, self.cols as isize - 1) as usize,
        }
    }

    /// Перемещение курсора; выделение схлопывается в одну ячейку.
    pub fn move_cursor(&mut self, dir: NavDir) {
        let (dr, dc) = dir.delta();
        let next = self.clamp_pos(
            self.cursor.row as isize + dr,
            self.cursor.col as isize + dc,
        );
        self.cursor = next;
        self.anchor = next;
    }

    /// Расширение прямоугольника: якорь фиксирован, двигается только focus (курсор).
    pub fn extend_selection(&mut self, dir: NavDir) {
        let (dr, dc) = dir.delta();
        self.cursor = self.clamp_pos(
            self.cursor.row as isize + dr,
            self.cursor.col as isize + dc,
        );
    }

    pub fn select_cell(&mut self, row: usize, col: usize) {
        let pos = self.clamp_pos(row as isize, col as isize);
        self.cursor = pos;
        self.anchor = pos;
    }
}

/// Список (linear_index, new_value) для записи в config.
pub fn nudge_rect_values(state: &TableGridState, rect: GridRect, delta: f64) -> Vec<(usize, f64)> {
    let mut out = Vec::new();
    for row in rect.r0..=rect.r1 {
        for col in rect.c0..=rect.c1 {
            let v = state.value_at_visual(row, col) + delta;
            out.push((state.index_visual(row, col), v));
        }
    }
    out
}

/// Билинейная интерполяция внутри прямоугольника; углы не меняются.
pub fn interpolate_rect(state: &TableGridState, rect: GridRect) -> Vec<(usize, f64)> {
    if rect.rows() < 2 && rect.cols() < 2 {
        return Vec::new();
    }

    let v00 = state.value_at_visual(rect.r0, rect.c0);
    let v01 = state.value_at_visual(rect.r0, rect.c1);
    let v10 = state.value_at_visual(rect.r1, rect.c0);
    let v11 = state.value_at_visual(rect.r1, rect.c1);

    let dr = (rect.r1 - rect.r0) as f64;
    let dc = (rect.c1 - rect.c0) as f64;

    let mut out = Vec::new();
    for row in rect.r0..=rect.r1 {
        for col in rect.c0..=rect.c1 {
            if rect.is_corner(row, col) {
                continue;
            }
            let tr = if dr > 0.0 {
                (row - rect.r0) as f64 / dr
            } else {
                0.0
            };
            let tc = if dc > 0.0 {
                (col - rect.c0) as f64 / dc
            } else {
                0.0
            };
            let top = v00 + (v01 - v00) * tc;
            let bottom = v10 + (v11 - v10) * tc;
            let v = top + (bottom - top) * tr;
            let v = (v * 10.0).round() / 10.0;
            out.push((state.index_visual(row, col), v));
        }
    }
    out
}

pub fn min_max_finite(values: &[f64]) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for &v in values {
        if v.is_finite() {
            min = min.min(v);
            max = max.max(v);
        }
    }
    if !min.is_finite() || !max.is_finite() {
        (0.0, 1.0)
    } else if (max - min).abs() < 1e-12 {
        (min, min + 1.0)
    } else {
        (min, max)
    }
}

/// t ∈ [0, 1]: синий → жёлтый → красный.
pub fn heat_color(t: f64) -> String {
    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.5 {
        let u = t * 2.0;
        (
            (30.0 + u * 220.0) as u8,
            (80.0 + u * 175.0) as u8,
            (200.0 - u * 120.0) as u8,
        )
    } else {
        let u = (t - 0.5) * 2.0;
        (
            (250.0) as u8,
            (255.0 - u * 200.0) as u8,
            (80.0 - u * 80.0) as u8,
        )
    };
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

pub fn format_cell_value(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    if (v - v.round()).abs() < 1e-9 {
        return format!("{}", v.round() as i64);
    }
    let s = format!("{:.3}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCellView {
    pub row: usize,
    pub col: usize,
    pub value: f64,
    pub display: String,
    pub heat_bg: String,
    pub selected: bool,
    pub cursor: bool,
    pub corner: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableGridView {
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<TableCellView>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub selection: GridRectView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridRectView {
    pub r0: usize,
    pub r1: usize,
    pub c0: usize,
    pub c1: usize,
}

pub fn build_grid_view(state: &TableGridState) -> TableGridView {
    let rect = state.selection();
    let (vmin, vmax) = min_max_finite(&state.values);
    let span = vmax - vmin;

    let mut cells = Vec::with_capacity(state.rows * state.cols);
    for visual_row in 0..state.rows {
        for col in 0..state.cols {
            let value = state.value_at_visual(visual_row, col);
            let t = if span > 0.0 {
                ((value - vmin) / span).clamp(0.0, 1.0)
            } else {
                0.5
            };
            cells.push(TableCellView {
                row: visual_row,
                col,
                value,
                display: format_cell_value(value),
                heat_bg: heat_color(t),
                selected: rect.contains(visual_row, col),
                cursor: state.cursor.row == visual_row && state.cursor.col == col,
                corner: rect.is_corner(visual_row, col),
            });
        }
    }

    TableGridView {
        rows: state.rows,
        cols: state.cols,
        cells,
        cursor_row: state.cursor.row,
        cursor_col: state.cursor.col,
        selection: GridRectView {
            r0: rect.r0,
            r1: rect.r1,
            c0: rect.c0,
            c1: rect.c1,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_table() -> TableGridState {
        // 3x3:
        // 10 20 30
        // 40 50 60
        // 70 80 90
        TableGridState::new(
            3,
            3,
            vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0],
        )
    }

    #[test]
    fn rect_from_anchor_focus() {
        let r = GridRect::from_anchor_focus(
            CellPos { row: 2, col: 0 },
            CellPos { row: 0, col: 2 },
        );
        assert_eq!((r.r0, r.r1, r.c0, r.c1), (0, 2, 0, 2));
    }

    #[test]
    fn move_cursor_collapses_selection() {
        let mut g = sample_table();
        g.anchor = CellPos { row: 0, col: 0 };
        g.cursor = CellPos { row: 2, col: 2 };
        g.move_cursor(NavDir::Right);
        assert_eq!(g.cursor, CellPos { row: 2, col: 2 });
        assert_eq!(g.anchor, g.cursor);

        g.cursor = CellPos { row: 0, col: 0 };
        g.anchor = CellPos { row: 2, col: 2 };
        g.move_cursor(NavDir::Right);
        assert_eq!(g.cursor, CellPos { row: 0, col: 1 });
        assert_eq!(g.anchor, g.cursor);
    }

    #[test]
    fn extend_selection_keeps_anchor() {
        let mut g = sample_table();
        g.anchor = CellPos { row: 0, col: 0 };
        g.cursor = CellPos { row: 0, col: 0 };
        g.extend_selection(NavDir::Down);
        g.extend_selection(NavDir::Right);
        assert_eq!(g.anchor, CellPos { row: 0, col: 0 });
        assert_eq!(g.cursor, CellPos { row: 1, col: 1 });
        let sel = g.selection();
        assert_eq!(sel.rows(), 2);
        assert_eq!(sel.cols(), 2);
    }

    #[test]
    fn interpolate_corners_fixed() {
        let g = sample_table();
        let rect = GridRect {
            r0: 0,
            r1: 2,
            c0: 0,
            c1: 2,
        };
        let updates: std::collections::HashMap<usize, f64> =
            interpolate_rect(&g, rect).into_iter().collect();
        assert!(!updates.contains_key(&0));
        assert!(!updates.contains_key(&2));
        assert!(!updates.contains_key(&6));
        assert!(!updates.contains_key(&8));
        let center = updates.get(&4).copied().unwrap();
        assert!((center - 50.0).abs() < 1e-6);
    }

    #[test]
    fn nudge_whole_rect() {
        let g = sample_table();
        let rect = GridRect::single(CellPos { row: 1, col: 1 });
        let updates = nudge_rect_values(&g, rect, 5.0);
        assert_eq!(updates.len(), 1);
        let storage_row = g.storage_row(1);
        assert_eq!(updates[0], (storage_row * g.cols + 1, 55.0));
    }
}
