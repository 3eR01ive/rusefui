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

    pub fn is_single(&self) -> bool {
        self.r0 == self.r1 && self.c0 == self.c1
    }
}

/// 1D-выделение для оси таблицы (RPM / load bins).
#[derive(Debug, Clone, Copy)]
pub struct Axis1dState {
    pub cursor: usize,
    pub anchor: usize,
}

impl Axis1dState {
    pub fn new() -> Self {
        Self {
            cursor: 0,
            anchor: 0,
        }
    }

    pub fn selection(&self) -> (usize, usize) {
        (self.anchor.min(self.cursor), self.anchor.max(self.cursor))
    }

    pub fn select(&mut self, index: usize, len: usize) {
        if len == 0 {
            return;
        }
        let i = index.min(len - 1);
        self.cursor = i;
        self.anchor = i;
    }

    pub fn extend_to(&mut self, index: usize, len: usize) {
        if len == 0 {
            return;
        }
        self.cursor = index.min(len - 1);
    }

    pub fn translate(&mut self, delta: isize, len: usize) {
        if len == 0 {
            return;
        }
        let next = (self.cursor as isize + delta).clamp(0, len as isize - 1) as usize;
        self.cursor = next;
        self.anchor = next;
    }

    pub fn extend_delta(&mut self, delta: isize, len: usize) {
        if len == 0 {
            return;
        }
        self.cursor = (self.cursor as isize + delta).clamp(0, len as isize - 1) as usize;
    }
}

pub fn nudge_axis_range(
    values: &[f64],
    i0: usize,
    i1: usize,
    delta: f64,
) -> Vec<(usize, f64)> {
    let mut out = Vec::new();
    for i in i0..=i1 {
        if let Some(&cur) = values.get(i) {
            let next = cur + delta;
            if (next - cur).abs() >= 1e-9 {
                out.push((i, next));
            }
        }
    }
    out
}

pub fn set_axis_range(values: &[f64], i0: usize, i1: usize, value: f64) -> Vec<(usize, f64)> {
    let mut out = Vec::new();
    for i in i0..=i1 {
        let cur = values.get(i).copied().unwrap_or(0.0);
        if (cur - value).abs() >= 1e-9 {
            out.push((i, value));
        }
    }
    out
}

pub fn paste_1d_at(values: &[f64], start: usize, text: &str) -> Vec<(usize, f64)> {
    let nums = parse_tsv_numbers(text);
    if nums.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (off, &v) in nums.iter().enumerate() {
        let i = start + off;
        if i >= values.len() {
            break;
        }
        let cur = values[i];
        if (cur - v).abs() >= 1e-9 {
            out.push((i, v));
        }
    }
    out
}

pub fn copy_axis_to_tsv(values: &[f64], i0: usize, i1: usize) -> String {
    (i0..=i1)
        .filter_map(|i| values.get(i))
        .map(|v| format_cell_value(*v))
        .collect::<Vec<_>>()
        .join("\t")
}

fn parse_tsv_numbers(text: &str) -> Vec<f64> {
    let mut nums = Vec::new();
    for line in text.lines() {
        for cell in split_paste_line(line) {
            let Some(v) = parse_paste_cell(cell) else {
                continue;
            };
            nums.push(v);
        }
    }
    nums
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EditFocus {
    Grid,
    X,
    Y,
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

    /// Сдвинуть прямоугольник выделения на одну ячейку; `false` у края сетки.
    pub fn translate_selection(&mut self, dir: NavDir) -> bool {
        let rect = self.selection();
        if rect.is_single() {
            self.move_cursor(dir);
            return true;
        }
        let (dr, dc) = dir.delta();
        let nr0 = rect.r0 as isize + dr;
        let nc0 = rect.c0 as isize + dc;
        let nr1 = rect.r1 as isize + dr;
        let nc1 = rect.c1 as isize + dc;
        if nr0 < 0
            || nc0 < 0
            || nr1 >= self.rows as isize
            || nc1 >= self.cols as isize
        {
            return false;
        }
        self.anchor = CellPos {
            row: nr0 as usize,
            col: nc0 as usize,
        };
        self.cursor = CellPos {
            row: nr1 as usize,
            col: nc1 as usize,
        };
        true
    }
}

/// TSV для буфера обмена (строки — `\n`, колонки — `\t`).
pub fn copy_rect_to_tsv(state: &TableGridState, rect: GridRect) -> String {
    let mut lines = Vec::with_capacity(rect.rows());
    for row in rect.r0..=rect.r1 {
        let cols: Vec<String> = (rect.c0..=rect.c1)
            .map(|col| format_cell_value(state.value_at_visual(row, col)))
            .collect();
        lines.push(cols.join("\t"));
    }
    lines.join("\n")
}

/// Разделитель ячеек в строке буфера (TSV из Excel/LibreOffice или CSV).
fn split_paste_line(line: &str) -> Vec<&str> {
    if line.contains('\t') {
        return line.split('\t').collect();
    }
    if line.contains(';') {
        return line.split(';').collect();
    }
    if line.contains(',') {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() > 1
            && parts.iter().all(|p| {
                p.trim()
                    .replace(',', ".")
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .is_some()
            })
            && parts.iter().all(|p| !p.trim().contains('.'))
        {
            return parts;
        }
    }
    vec![line]
}

fn parse_paste_cell(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    s.replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

/// Вставка TSV/CSV из буфера: от левого верхнего угла `rect`, обрезка по границам таблицы.
pub fn paste_tsv_at(state: &TableGridState, rect: GridRect, text: &str) -> Vec<(usize, f64)> {
    let mut lines: Vec<&str> = text.split(['\r', '\n']).collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let mut updates = Vec::new();
    for (dr, line) in lines.iter().enumerate() {
        let row = rect.r0 + dr;
        if row >= state.rows {
            break;
        }
        for (dc, cell) in split_paste_line(line).into_iter().enumerate() {
            let col = rect.c0 + dc;
            if col >= state.cols {
                break;
            }
            let Some(value) = parse_paste_cell(cell) else {
                continue;
            };
            let idx = state.index_visual(row, col);
            let current = state.values.get(idx).copied().unwrap_or(0.0);
            if (current - value).abs() >= 1e-9 {
                updates.push((idx, value));
            }
        }
    }
    updates
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

pub fn interpolate_axis_range(values: &[f64], i0: usize, i1: usize) -> Vec<(usize, f64)> {
    if i1 <= i0 {
        return Vec::new();
    }
    let v0 = values.get(i0).copied().unwrap_or(0.0);
    let v1 = values.get(i1).copied().unwrap_or(0.0);
    let span = (i1 - i0) as f64;
    let mut out = Vec::new();
    for i in i0..=i1 {
        if i == i0 || i == i1 {
            continue;
        }
        let t = (i - i0) as f64 / span;
        let v = ((v0 + (v1 - v0) * t) * 10.0).round() / 10.0;
        let cur = values.get(i).copied().unwrap_or(0.0);
        if (cur - v).abs() >= 1e-9 {
            out.push((i, v));
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AxisCellView {
    pub index: usize,
    pub value: f64,
    pub display: String,
    pub selected: bool,
    pub cursor: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AxisBarView {
    pub cells: Vec<AxisCellView>,
    pub sel_i0: usize,
    pub sel_i1: usize,
    pub editable: bool,
}

pub fn build_axis_view(
    values: &[f64],
    axis: &Axis1dState,
    editable: bool,
    active: bool,
) -> AxisBarView {
    let (i0, i1) = axis.selection();
    let cells = values
        .iter()
        .enumerate()
        .map(|(index, &value)| AxisCellView {
            index,
            value,
            display: format_cell_value(value),
            selected: active && index >= i0 && index <= i1,
            cursor: active && index == axis.cursor,
        })
        .collect();
    AxisBarView {
        cells,
        sel_i0: if active { i0 } else { 0 },
        sel_i1: if active { i1 } else { 0 },
        editable,
    }
}

pub fn build_grid_view(state: &TableGridState, active: bool) -> TableGridView {
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
                selected: active && rect.contains(visual_row, col),
                cursor: active && state.cursor.row == visual_row && state.cursor.col == col,
                corner: active && rect.is_corner(visual_row, col),
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
            r0: if active { rect.r0 } else { 0 },
            r1: if active { rect.r1 } else { 0 },
            c0: if active { rect.c0 } else { 0 },
            c1: if active { rect.c1 } else { 0 },
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

    #[test]
    fn translate_selection_block() {
        let mut g = sample_table();
        g.anchor = CellPos { row: 0, col: 0 };
        g.cursor = CellPos { row: 1, col: 1 };
        assert!(g.translate_selection(NavDir::Right));
        assert_eq!(g.anchor, CellPos { row: 0, col: 1 });
        assert_eq!(g.cursor, CellPos { row: 1, col: 2 });
        assert!(!g.translate_selection(NavDir::Right));
    }

    #[test]
    fn interpolate_axis_range_linear() {
        let values = vec![1000.0, 2000.0, 9999.0, 4000.0, 5000.0];
        let updates: std::collections::HashMap<usize, f64> =
            interpolate_axis_range(&values, 0, 4).into_iter().collect();
        assert!(!updates.contains_key(&0));
        assert!(!updates.contains_key(&4));
        assert_eq!(updates.get(&2).copied().unwrap(), 3000.0);
    }

    #[test]
    fn copy_and_paste_tsv() {
        let g = sample_table();
        let rect = GridRect {
            r0: 0,
            r1: 1,
            c0: 0,
            c1: 1,
        };
        let tsv = copy_rect_to_tsv(&g, rect);
        assert_eq!(tsv, "70\t80\n40\t50");
        let updates = paste_tsv_at(&g, GridRect::single(CellPos { row: 2, col: 2 }), "1\t2\n3\t4");
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0], (2, 1.0));
    }
}
