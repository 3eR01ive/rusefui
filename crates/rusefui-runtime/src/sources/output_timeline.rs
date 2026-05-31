use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Максимум live-истории в RAM (полная частота опроса).
const LIVE_BUFFER_SEC: f64 = 120.0;
/// Запас точек на поле в live-буфере (~200 Hz × 120 с).
const LIVE_MAX_POINTS_PER_FIELD: usize = 25_000;
/// Строк CSV за один pull (старые → новые).
pub const FILE_CHUNK_ROWS_DEFAULT: usize = 4096;
/// Точек на поле за один pull после backfill.
pub const SERIES_CHUNK_MAX_POINTS: usize = 8192;
/// Верхняя граница точек на поле в snapshot для UI (IPC).
pub const SERIES_SNAPSHOT_MAX_POINTS: usize = 32_768;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TimelineMode {
    Empty,
    Live,
    File,
    LiveAndFile,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTimelineStatus {
    pub mode: TimelineMode,
    pub connected: bool,
    pub follow_live: bool,
    pub live_sec: f64,
    pub data_min_sec: f64,
    pub data_max_sec: f64,
    pub view_end_sec: f64,
    pub span_sec: f64,
    pub session_log_path: Option<String>,
    pub field_count: usize,
    /// Меняется при ingest / load_file / reset — UI подтягивает новые чанки.
    pub series_revision: u64,
    /// CSV читается построчно в RAM (не одним IPC-блоком).
    pub file_loading: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelinePoint {
    pub t: f64,
    pub v: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineFieldView {
    pub field: String,
    pub points: Vec<TimelinePoint>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTimelineView {
    pub t_min: f64,
    pub t_max: f64,
    pub live_sec: f64,
    pub follow_live: bool,
    pub series: Vec<TimelineFieldView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTimelineSeriesSnapshot {
    pub revision: u64,
    pub live_sec: f64,
    pub data_min_sec: f64,
    pub data_max_sec: f64,
    pub series: Vec<TimelineFieldView>,
}

/// Порция рядов для UI: файл (старые→новые) или live-хвост.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTimelineSeriesChunk {
    pub revision: u64,
    pub live_sec: f64,
    pub data_min_sec: f64,
    pub data_max_sec: f64,
    pub series: Vec<TimelineFieldView>,
    pub file_load_done: bool,
    pub has_more: bool,
}

#[derive(Debug, Clone)]
pub struct OutputTimelineViewQuery {
    pub fields: Vec<String>,
    pub pixel_width: u32,
}

#[derive(Debug, Clone)]
pub struct OutputTimelineSeriesQuery {
    pub fields: Vec<String>,
    pub max_points_per_field: usize,
}

#[derive(Debug, Clone)]
pub struct OutputTimelineChunkQuery {
    pub fields: Vec<String>,
    pub max_rows: usize,
    pub max_points_per_field: usize,
    pub reset_stream: bool,
}

struct FileLoaderState {
    reader: BufReader<File>,
    line_buf: String,
    col_map: HashMap<String, usize>,
    done: bool,
    rows_read: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTimelineViewControl {
    pub follow_live: Option<bool>,
    pub view_end_sec: Option<f64>,
    pub span_sec: Option<f64>,
    pub pan_sec: Option<f64>,
    pub zoom_factor: Option<f64>,
}

struct FieldSeries {
    points: VecDeque<(f64, f64)>,
}

pub struct OutputTimeline {
    session_start_ms: u64,
    fields: HashMap<String, FieldSeries>,
    field_order: Vec<String>,
    session_file: Option<PathBuf>,
    connected: bool,
    follow_live: bool,
    live_sec: f64,
    data_min_sec: f64,
    data_max_sec: f64,
    view_end_sec: f64,
    span_sec: f64,
    series_revision: u64,
    last_series_bump_ms: u64,
    file_loader: Option<FileLoaderState>,
    /// Индекс следующей точки для pull (на поле).
    stream_cursors: HashMap<String, usize>,
}

impl Default for OutputTimeline {
    fn default() -> Self {
        Self {
            fields: HashMap::new(),
            field_order: Vec::new(),
            session_file: None,
            session_start_ms: 0,
            connected: false,
            follow_live: true,
            live_sec: 0.0,
            data_min_sec: 0.0,
            data_max_sec: 0.0,
            view_end_sec: 0.0,
            span_sec: 30.0,
            series_revision: 0,
            last_series_bump_ms: 0,
            file_loader: None,
            stream_cursors: HashMap::new(),
        }
    }
}

impl OutputTimeline {
    pub fn reset_session(&mut self, field_names: &[String], span_sec: f64) {
        self.reset_session_with_start(field_names, span_sec, now_ms());
    }

    pub fn reset_session_with_start(
        &mut self,
        field_names: &[String],
        span_sec: f64,
        session_start_ms: u64,
    ) {
        self.session_start_ms = session_start_ms;
        self.fields.clear();
        self.field_order = field_names.to_vec();
        for name in field_names {
            self.fields.insert(name.clone(), FieldSeries {
                points: VecDeque::new(),
            });
        }
        self.session_file = None;
        self.connected = true;
        self.follow_live = true;
        self.live_sec = 0.0;
        self.data_min_sec = 0.0;
        self.data_max_sec = 0.0;
        self.view_end_sec = 0.0;
        self.span_sec = span_sec.clamp(1.0, 3600.0);
        self.file_loader = None;
        self.stream_cursors.clear();
        self.bump_series_revision();
    }

    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
        if connected {
            self.follow_live = true;
        }
    }

    pub fn set_session_file(&mut self, path: Option<PathBuf>) {
        self.session_file = path.clone();
        if path.is_some() {
            self.refresh_bounds_from_file();
        }
    }

    pub fn session_log_path(&self) -> Option<String> {
        self.session_file
            .as_ref()
            .map(|p| p.display().to_string())
    }

    pub fn load_file(&mut self, path: PathBuf) {
        self.session_file = Some(path.clone());
        self.connected = false;
        self.follow_live = false;
        self.stream_cursors.clear();
        self.start_file_loader(&path);
        self.refresh_bounds_from_file();
        self.clamp_span_to_data();
        self.clamp_view_end();
        self.bump_series_revision();
    }

    pub fn ingest_from_wall_ms(&mut self, timestamp_ms: u64, values: &HashMap<String, f64>) {
        let elapsed_sec = (timestamp_ms.saturating_sub(self.session_start_ms)) as f64 / 1000.0;
        self.ingest(elapsed_sec, values);
    }

    pub fn ingest(&mut self, elapsed_sec: f64, values: &HashMap<String, f64>) {
        if elapsed_sec > self.live_sec {
            self.live_sec = elapsed_sec;
        }
        if self.data_max_sec < elapsed_sec {
            self.data_max_sec = elapsed_sec;
        }
        if self.fields.is_empty() {
            for name in values.keys() {
                if !self.fields.contains_key(name) {
                    self.field_order.push(name.clone());
                    self.fields.insert(name.clone(), FieldSeries {
                        points: VecDeque::new(),
                    });
                }
            }
        }
        for (name, &v) in values {
            if !self.fields.contains_key(name) {
                self.field_order.push(name.clone());
                self.fields.insert(name.clone(), FieldSeries {
                    points: VecDeque::new(),
                });
            }
            if let Some(series) = self.fields.get_mut(name) {
                push_point(series, elapsed_sec, v);
                let popped = trim_live(series, elapsed_sec);
                if popped > 0 {
                    if let Some(cursor) = self.stream_cursors.get_mut(name) {
                        *cursor = cursor.saturating_sub(popped);
                    }
                }
            }
        }
        if self.follow_live {
            self.view_end_sec = self.live_sec;
        }
        self.maybe_bump_series_revision();
    }

    fn bump_series_revision(&mut self) {
        self.series_revision = self.series_revision.wrapping_add(1);
        self.last_series_bump_ms = now_ms();
    }

    /// Live ingest: не чаще ~4 раз/с, чтобы UI успевал подтягивать snapshot.
    fn maybe_bump_series_revision(&mut self) {
        let now = now_ms();
        if now.saturating_sub(self.last_series_bump_ms) >= 250 {
            self.bump_series_revision();
        }
    }

    pub fn apply_view_control(&mut self, ctrl: OutputTimelineViewControl) -> OutputTimelineStatus {
        if let Some(follow) = ctrl.follow_live {
            self.follow_live = follow;
            if follow {
                self.view_end_sec = self.effective_live_sec();
            }
        }
        if let Some(span) = ctrl.span_sec {
            self.span_sec = span.clamp(0.5, 3600.0);
            self.clamp_span_to_data();
        }
        if let Some(end) = ctrl.view_end_sec {
            self.follow_live = false;
            self.view_end_sec = end;
        }
        if let Some(pan) = ctrl.pan_sec {
            self.follow_live = false;
            let (lo, hi) = self.view_end_range();
            self.view_end_sec = clamp_f64(self.view_end_sec + pan, lo, hi);
        }
        if let Some(zoom) = ctrl.zoom_factor {
            if zoom.is_finite() && zoom > 0.0 {
                self.span_sec = (self.span_sec / zoom).clamp(0.5, 3600.0);
                self.clamp_span_to_data();
                if self.follow_live {
                    self.view_end_sec = self.effective_live_sec();
                }
            }
        }
        self.clamp_view_end();
        self.status()
    }

    /// Текущее окно просмотра (для синхронизации с composite timeline).
    pub fn view_control_snapshot(&self) -> OutputTimelineViewControl {
        OutputTimelineViewControl {
            follow_live: Some(self.follow_live),
            view_end_sec: Some(self.view_end_sec),
            span_sec: Some(self.span_sec),
            pan_sec: None,
            zoom_factor: None,
        }
    }

    pub fn status(&self) -> OutputTimelineStatus {
        OutputTimelineStatus {
            mode: self.mode(),
            connected: self.connected,
            follow_live: self.follow_live,
            live_sec: self.effective_live_sec(),
            data_min_sec: self.data_min_sec,
            data_max_sec: self.data_max_sec.max(self.live_sec),
            view_end_sec: self.view_end_sec,
            span_sec: self.span_sec,
            session_log_path: self.session_log_path(),
            field_count: self.field_order.len(),
            series_revision: self.series_revision,
            file_loading: self.file_loader.is_some(),
        }
    }

    /// Потоковая отдача рядов: CSV построчно (старые→новые), затем live-хвост.
    pub fn pull_series_chunk(&mut self, q: &OutputTimelineChunkQuery) -> OutputTimelineSeriesChunk {
        if q.reset_stream {
            self.stream_cursors.clear();
        }

        let max_rows = q.max_rows.clamp(64, 65_536);
        let max_pts = q
            .max_points_per_field
            .clamp(256, SERIES_SNAPSHOT_MAX_POINTS);

        if let Some(loader) = self.file_loader.take() {
            let mut loader = loader;
            if !loader.done {
                loader.read_rows(self, max_rows);
            }
            if loader.done {
                self.finalize_file_load();
            } else {
                self.file_loader = Some(loader);
            }
        }

        let mut series = Vec::with_capacity(q.fields.len());
        let mut any_unread = false;

        for field in &q.fields {
            let cursor = self.stream_cursors.entry(field.clone()).or_insert(0);
            let Some(fs) = self.fields.get(field) else {
                continue;
            };
            let len = fs.points.len();
            if *cursor >= len {
                continue;
            }
            let end = (*cursor + max_pts).min(len);
            let points: Vec<TimelinePoint> = fs
                .points
                .iter()
                .skip(*cursor)
                .take(end - *cursor)
                .map(|(t, v)| TimelinePoint { t: *t, v: *v })
                .collect();
            *cursor = end;
            if !points.is_empty() {
                series.push(TimelineFieldView {
                    field: field.clone(),
                    points,
                });
            }
            if *cursor < fs.points.len() {
                any_unread = true;
            }
        }

        let file_load_done = self.file_loader.is_none();
        let has_more = self.file_loader.is_some() || any_unread;

        OutputTimelineSeriesChunk {
            revision: self.series_revision,
            live_sec: self.effective_live_sec(),
            data_min_sec: self.data_min_sec,
            data_max_sec: self.data_max_sec.max(self.live_sec),
            series,
            file_load_done,
            has_more,
        }
    }

    /// Полный снимок рядов в RAM (decimate только для лимита IPC).
    pub fn series_snapshot(&self, q: &OutputTimelineSeriesQuery) -> OutputTimelineSeriesSnapshot {
        let max_pts = q
            .max_points_per_field
            .clamp(256, SERIES_SNAPSHOT_MAX_POINTS);
        let t_min = self.data_min_sec;
        let t_max = self.data_max_sec.max(self.live_sec);

        let mut series = Vec::with_capacity(q.fields.len());
        for field in &q.fields {
            let mut points = self.collect_field_points(field, t_min, t_max);
            decimate_in_place(&mut points, max_pts);
            series.push(TimelineFieldView {
                field: field.clone(),
                points: points
                    .into_iter()
                    .map(|(t, v)| TimelinePoint { t, v })
                    .collect(),
            });
        }

        OutputTimelineSeriesSnapshot {
            revision: self.series_revision,
            live_sec: self.effective_live_sec(),
            data_min_sec: self.data_min_sec,
            data_max_sec: self.data_max_sec.max(self.live_sec),
            series,
        }
    }

    pub fn query_view(&self, q: &OutputTimelineViewQuery) -> OutputTimelineView {
        let mut t_max = if self.follow_live {
            self.effective_live_sec()
        } else {
            self.view_end_sec
        };
        let mut t_min = (t_max - self.span_sec).max(self.data_min_sec);
        if t_max <= t_min + 1e-9 {
            t_max = t_min + self.span_sec;
        }
        // В начале сессии — фиксированная шкала 0..span (кривая растёт слева, не прилипает к правому краю).
        if self.follow_live && t_max < self.span_sec {
            t_min = self.data_min_sec;
            t_max = self.span_sec;
        }
        let max_points = q.pixel_width.max(64).min(4096) as usize;

        let mut series = Vec::with_capacity(q.fields.len());
        for field in &q.fields {
            let mut points = self.collect_field_points(field, t_min, t_max);
            decimate_in_place(&mut points, max_points);
            series.push(TimelineFieldView {
                field: field.clone(),
                points: points
                    .into_iter()
                    .map(|(t, v)| TimelinePoint { t, v })
                    .collect(),
            });
        }

        OutputTimelineView {
            t_min,
            t_max,
            live_sec: self.effective_live_sec(),
            follow_live: self.follow_live,
            series,
        }
    }

    pub fn value_at(&self, field: &str, t: f64) -> Option<f64> {
        let points = self.collect_field_points(field, t - 0.001, t + 0.001);
        points.last().map(|(_, v)| *v).or_else(|| {
            self.fields
                .get(field)
                .and_then(|s| s.points.back().map(|(_, v)| *v))
        })
    }

    fn mode(&self) -> TimelineMode {
        let has_live = self.live_sec > 0.0 || self.fields.values().any(|s| !s.points.is_empty());
        let has_file = self.session_file.is_some();
        match (has_live, has_file) {
            (true, true) => TimelineMode::LiveAndFile,
            (true, false) => TimelineMode::Live,
            (false, true) => TimelineMode::File,
            (false, false) => TimelineMode::Empty,
        }
    }

    pub fn live_sec(&self) -> f64 {
        self.effective_live_sec()
    }

    pub fn session_start_ms(&self) -> u64 {
        self.session_start_ms
    }

    fn effective_live_sec(&self) -> f64 {
        self.live_sec.max(self.data_max_sec)
    }

    /// Длительность доступных данных (сек).
    fn data_duration_sec(&self) -> f64 {
        let hi = self.data_max_sec.max(self.live_sec);
        (hi - self.data_min_sec).max(0.001)
    }

    /// Окно не шире записанной истории — иначе min_end > max_end и panic в clamp.
    fn clamp_span_to_data(&mut self) {
        let max_span = self.data_duration_sec();
        if self.span_sec > max_span {
            self.span_sec = max_span.max(0.5);
        }
    }

    /// Допустимый правый край окна: lo <= hi всегда.
    fn view_end_range(&self) -> (f64, f64) {
        let hi = self.data_max_sec.max(self.live_sec);
        let lo = (self.data_min_sec + self.span_sec).min(hi);
        (lo, hi)
    }

    fn clamp_view_end(&mut self) {
        self.clamp_span_to_data();
        let (lo, hi) = self.view_end_range();
        self.view_end_sec = clamp_f64(self.view_end_sec, lo, hi);
    }

    fn collect_field_points(&self, field: &str, t_min: f64, t_max: f64) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        if let Some(series) = self.fields.get(field) {
            for &(t, v) in &series.points {
                if t >= t_min && t <= t_max {
                    out.push((t, v));
                }
            }
        }
        out
    }

    fn start_file_loader(&mut self, path: &PathBuf) {
        self.fields.clear();
        self.field_order.clear();
        self.data_min_sec = 0.0;
        self.data_max_sec = 0.0;
        self.live_sec = 0.0;
        self.file_loader = None;

        let Ok(file) = File::open(path) else {
            return;
        };
        let mut reader = BufReader::new(file);
        let mut header_line = String::new();
        loop {
            header_line.clear();
            match reader.read_line(&mut header_line) {
                Ok(0) => return,
                Err(_) => return,
                Ok(_) => {}
            }
            let line = header_line.trim_end_matches(['\r', '\n']);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if !line.starts_with("timestamp_ms,elapsed_sec") {
                return;
            }
            header_line = line.to_string();
            break;
        }
        self.field_order = parse_header_fields(&header_line);
        for name in &self.field_order {
            self.fields.insert(
                name.clone(),
                FieldSeries {
                    points: VecDeque::new(),
                },
            );
        }
        let col_map: HashMap<String, usize> = self
            .field_order
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i))
            .collect();

        self.file_loader = Some(FileLoaderState {
            reader,
            line_buf: String::new(),
            col_map,
            done: false,
            rows_read: 0,
        });
    }

    fn finalize_file_load(&mut self) {
        self.file_loader = None;
    }

    fn append_file_row(&mut self, elapsed: f64, values: &[f64], col_map: &HashMap<String, usize>) {
        if self.data_min_sec == 0.0 && self.data_max_sec == 0.0 && elapsed.is_finite() {
            self.data_min_sec = elapsed;
            self.data_max_sec = elapsed;
        } else {
            if elapsed < self.data_min_sec {
                self.data_min_sec = elapsed;
            }
            if elapsed > self.data_max_sec {
                self.data_max_sec = elapsed;
            }
        }
        for (name, &col) in col_map {
            if let Some(&v) = values.get(col) {
                if let Some(series) = self.fields.get_mut(name) {
                    push_point(series, elapsed, v);
                }
            }
        }
    }

    fn refresh_bounds_from_file(&mut self) {
        let Some(path) = self.session_file.as_ref() else {
            return;
        };
        let Ok(file) = File::open(path) else {
            return;
        };
        let reader = BufReader::new(file);
        let mut min_t = f64::INFINITY;
        let mut max_t = f64::NEG_INFINITY;
        for line in reader.lines().map_while(Result::ok) {
            if !line.contains(',') || line.starts_with('#') {
                continue;
            }
            if line.starts_with("timestamp_ms,") {
                if self.field_order.is_empty() {
                    self.field_order = parse_header_fields(&line);
                }
                continue;
            }
            let mut parts = line.split(',');
            let _ = parts.next();
            if let Some(elapsed) = parts.next().and_then(|s| s.parse::<f64>().ok()) {
                if elapsed < min_t {
                    min_t = elapsed;
                }
                if elapsed > max_t {
                    max_t = elapsed;
                }
            }
        }
        if min_t.is_finite() {
            self.data_min_sec = min_t;
        }
        if max_t.is_finite() {
            self.data_max_sec = max_t;
        }
        if self.follow_live {
            self.view_end_sec = self.effective_live_sec();
        }
        self.clamp_view_end();
    }
}

/// `f64::clamp` паникует, если `lo > hi` (например span шире, чем длина лога).
fn clamp_f64(v: f64, lo: f64, hi: f64) -> f64 {
    if !v.is_finite() {
        return if hi.is_finite() { hi } else { lo };
    }
    if !lo.is_finite() || !hi.is_finite() {
        return v;
    }
    if lo > hi {
        return hi;
    }
    v.clamp(lo, hi)
}

impl FileLoaderState {
    fn read_rows(&mut self, tl: &mut OutputTimeline, max_rows: usize) -> usize {
        let mut count = 0usize;
        while count < max_rows {
            self.line_buf.clear();
            let read = match self.reader.read_line(&mut self.line_buf) {
                Ok(0) => {
                    self.done = true;
                    break;
                }
                Ok(n) => n,
                Err(_) => {
                    self.done = true;
                    break;
                }
            };
            if read == 0 {
                self.done = true;
                break;
            }
            let line = self.line_buf.trim_end_matches(['\r', '\n']);
            if line.is_empty() || line.starts_with('#') || line.starts_with("timestamp_ms,") {
                continue;
            }
            let mut parts = line.split(',');
            let _ = parts.next();
            let Some(elapsed_s) = parts.next() else {
                continue;
            };
            let Ok(elapsed) = elapsed_s.parse::<f64>() else {
                continue;
            };
            let values: Vec<f64> = parts.filter_map(|s| s.parse().ok()).collect();
            tl.append_file_row(elapsed, &values, &self.col_map);
            self.rows_read += 1;
            count += 1;
        }
        count
    }
}

fn push_point(series: &mut FieldSeries, t: f64, v: f64) {
    if let Some((last_t, last_v)) = series.points.back_mut() {
        if (*last_t - t).abs() < 1e-9 {
            *last_v = v;
            return;
        }
    }
    series.points.push_back((t, v));
}

fn trim_live(series: &mut FieldSeries, t_now: f64) -> usize {
    let min_t = t_now - LIVE_BUFFER_SEC;
    let mut popped = 0usize;
    while series
        .points
        .front()
        .is_some_and(|(t, _)| *t < min_t)
    {
        series.points.pop_front();
        popped += 1;
    }
    while series.points.len() > LIVE_MAX_POINTS_PER_FIELD {
        series.points.pop_front();
        popped += 1;
    }
    popped
}

fn decimate_in_place(points: &mut Vec<(f64, f64)>, max_points: usize) {
    if points.len() <= max_points {
        return;
    }
    let stride = (points.len() as f64 / max_points as f64).ceil() as usize;
    if stride <= 1 {
        return;
    }
    let mut decimated = Vec::with_capacity(max_points);
    let mut i = 0;
    while i < points.len() {
        let end = (i + stride).min(points.len());
        // Последняя точка чанка — сохраняет порядок времени для polyline.
        decimated.push(points[end - 1]);
        i += stride;
    }
    *points = decimated;
}

fn parse_header_fields(header: &str) -> Vec<String> {
    header
        .split(',')
        .skip(2)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::output_data_log::OutputDataLogWriter;
    use crate::sources::output_channels::IniContext;
    use rusefi_protocol::ConnectionInfo;
    use std::sync::Arc;

    fn test_ini() -> IniContext {
        use rusefi_ini::{
            FieldKind, OutputChannelField, OutputChannels, ScalarField, ScalarType,
        };
        IniContext {
            signature: Some("test".into()),
            channels: Arc::new(OutputChannels {
                och_block_size: 64,
                fields: vec![
                    OutputChannelField {
                        name: "RPMValue".into(),
                        kind: FieldKind::Scalar(ScalarField {
                            ty: ScalarType::U16,
                            offset: 0,
                            page: 0,
                            units: String::new(),
                            scale: 1.0,
                            translate: 0.0,
                        }),
                    },
                    OutputChannelField {
                        name: "coolant".into(),
                        kind: FieldKind::Scalar(ScalarField {
                            ty: ScalarType::S16,
                            offset: 2,
                            page: 0,
                            units: String::new(),
                            scale: 1.0,
                            translate: 0.0,
                        }),
                    },
                ],
                by_name: HashMap::new(),
            }),
            block_size: 64,
            blocking_factor: 64,
            page_size: 4096,
            page_sizes: vec![4096],
            page_read_has_page_index: true,
            page_chunk_write_has_page_index: true,
            config_fields: HashMap::new(),
            ts_commands: HashMap::new(),
            inter_write_delay_ms: 10,
            page_activation_delay_ms: 500,
        }
    }

    #[test]
    fn pull_series_chunk_streams_file_oldest_first() {
        let stamp = now_ms();
        let dir = std::env::temp_dir().join(format!("rusefui-chunk-{stamp}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("RUSEFUI_OUTPUT_LOG_DIR", &dir);
        let info = ConnectionInfo {
            port_name: format!("chunk-{stamp}"),
            baud_rate: 115_200,
            signature: "sig".into(),
            handshake_command: 'S',
        };
        let ini = test_ini();
        let names: Vec<String> = ini
            .channels
            .fields
            .iter()
            .map(|f| f.name.clone())
            .collect();
        let mut log = OutputDataLogWriter::open(&info, &ini, None).unwrap();
        let mut values = HashMap::new();
        values.insert("RPMValue".into(), 1000.0);
        let t0 = now_ms();
        log.write_sample(t0, &values);
        values.insert("RPMValue".into(), 2000.0);
        log.write_sample(t0 + 5000, &values);
        values.insert("RPMValue".into(), 3000.0);
        log.write_sample(t0 + 10_000, &values);
        let (path, _) = log.close().unwrap();

        let mut tl = OutputTimeline::default();
        tl.load_file(path);
        let mut all_t = Vec::new();
        let mut reset = true;
        for _ in 0..100 {
            let chunk = tl.pull_series_chunk(&OutputTimelineChunkQuery {
                fields: vec!["RPMValue".into()],
                max_rows: 1,
                max_points_per_field: 100,
                reset_stream: reset,
            });
            reset = false;
            for s in &chunk.series {
                for p in &s.points {
                    all_t.push(p.t);
                }
            }
            if !chunk.has_more {
                break;
            }
        }
        assert!(all_t.len() >= 2);
        for w in all_t.windows(2) {
            assert!(w[0] <= w[1]);
        }

        std::env::remove_var("RUSEFUI_OUTPUT_LOG_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn query_live_and_file() {
        let dir = std::env::temp_dir().join(format!("rusefui-timeline-{}", now_ms()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("RUSEFUI_OUTPUT_LOG_DIR", &dir);
        let info = ConnectionInfo {
            port_name: "tty".into(),
            baud_rate: 115_200,
            signature: "sig".into(),
            handshake_command: 'S',
        };
        let ini = test_ini();
        let names: Vec<String> = ini
            .channels
            .fields
            .iter()
            .map(|f| f.name.clone())
            .collect();
        let mut log = OutputDataLogWriter::open(&info, &ini, None).unwrap();
        let mut values = HashMap::new();
        values.insert("RPMValue".into(), 1000.0);
        let t0 = now_ms();
        log.write_sample(t0, &values);
        values.insert("RPMValue".into(), 2000.0);
        log.write_sample(t0 + 5000, &values);
        let (path, _) = log.close().unwrap();

        let mut tl = OutputTimeline::default();
        tl.reset_session(&names, 10.0);
        tl.ingest_from_wall_ms(t0 + 5000, &values);
        tl.set_session_file(Some(path.clone()));
        tl.apply_view_control(OutputTimelineViewControl {
            follow_live: Some(false),
            view_end_sec: Some(5.0),
            span_sec: Some(10.0),
            pan_sec: None,
            zoom_factor: None,
        });
        let view = tl.query_view(&OutputTimelineViewQuery {
            fields: vec!["RPMValue".into()],
            pixel_width: 800,
        });
        assert!(view.series[0].points.len() >= 1);

        std::env::remove_var("RUSEFUI_OUTPUT_LOG_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn zoom_out_and_pan_never_panics_when_span_exceeds_data() {
        let mut tl = OutputTimeline::default();
        tl.reset_session(&["RPMValue".into()], 30.0);
        for i in 0..100 {
            let mut v = HashMap::new();
            v.insert("RPMValue".into(), 1000.0 + i as f64);
            tl.ingest(i as f64 * 3.0, &v);
        }
        tl.apply_view_control(OutputTimelineViewControl {
            follow_live: Some(false),
            view_end_sec: Some(tl.live_sec),
            span_sec: None,
            pan_sec: None,
            zoom_factor: None,
        });
        for _ in 0..20 {
            tl.apply_view_control(OutputTimelineViewControl {
                follow_live: None,
                view_end_sec: None,
                span_sec: None,
                pan_sec: None,
                zoom_factor: Some(0.8),
            });
        }
        assert!(tl.span_sec <= tl.data_duration_sec() + 1e-6);
        for _ in 0..50 {
            let _ = tl.apply_view_control(OutputTimelineViewControl {
                follow_live: None,
                view_end_sec: None,
                span_sec: None,
                pan_sec: Some(100.0),
                zoom_factor: None,
            });
            let _ = tl.apply_view_control(OutputTimelineViewControl {
                follow_live: None,
                view_end_sec: None,
                span_sec: None,
                pan_sec: Some(-100.0),
                zoom_factor: None,
            });
        }
    }

    #[test]
    fn pull_yields_new_points_after_live_trim() {
        let mut tl = OutputTimeline::default();
        tl.reset_session(&["RPMValue".into()], 30.0);
        for i in 0..30_000 {
            let mut v = HashMap::new();
            v.insert("RPMValue".into(), 1000.0 + i as f64);
            tl.ingest(i as f64 * 0.005, &v);
        }
        let mut reset = true;
        loop {
            let chunk = tl.pull_series_chunk(&OutputTimelineChunkQuery {
                fields: vec!["RPMValue".into()],
                max_rows: 4096,
                max_points_per_field: 8192,
                reset_stream: reset,
            });
            reset = false;
            if !chunk.has_more {
                break;
            }
        }
        let mut v = HashMap::new();
        v.insert("RPMValue".into(), 9999.0);
        tl.ingest(500.0, &v);
        let chunk = tl.pull_series_chunk(&OutputTimelineChunkQuery {
            fields: vec!["RPMValue".into()],
            max_rows: 4096,
            max_points_per_field: 8192,
            reset_stream: false,
        });
        assert!(!chunk.series.is_empty());
        assert!(chunk.series[0].points.iter().any(|p| (p.v - 9999.0).abs() < 1e-6));
    }
}
