use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Максимум live-истории в RAM (полная частота опроса).
const LIVE_BUFFER_SEC: f64 = 120.0;
/// Запас точек на поле в live-буфере (~200 Hz × 120 с).
const LIVE_MAX_POINTS_PER_FIELD: usize = 25_000;

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

#[derive(Debug, Clone)]
pub struct OutputTimelineViewQuery {
    pub fields: Vec<String>,
    pub pixel_width: u32,
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
        self.session_file = Some(path);
        self.connected = false;
        self.follow_live = false;
        self.refresh_bounds_from_file();
        self.view_end_sec = self.data_max_sec.max(self.live_sec);
        self.clamp_view_end();
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
                trim_live(series, elapsed_sec);
            }
        }
        if self.follow_live {
            self.view_end_sec = self.live_sec;
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
        let buffer_start = self
            .fields
            .get(field)
            .and_then(|s| s.points.front().map(|(t, _)| *t))
            .unwrap_or(f64::INFINITY);
        if t_min < buffer_start {
            if let Some(file_points) = self.read_file_segment(field, t_min, t_max.min(buffer_start)) {
                out = merge_sorted(file_points, out);
            }
        }
        out
    }

    fn read_file_segment(&self, field: &str, t_min: f64, t_max: f64) -> Option<Vec<(f64, f64)>> {
        let path = self.session_file.as_ref()?;
        let file = File::open(path).ok()?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let header = loop {
            let line = lines.next()?.ok()?;
            if line.starts_with("timestamp_ms,elapsed_sec") {
                break line;
            }
        };
        let col_idx = parse_field_column(&header, field)?;
        let mut out = Vec::new();
        for line in lines.flatten() {
            if line.starts_with('#') {
                continue;
            }
            let mut parts = line.split(',');
            let _ts = parts.next()?;
            let elapsed = parts.next()?.parse::<f64>().ok()?;
            if elapsed < t_min {
                continue;
            }
            if elapsed > t_max {
                break;
            }
            let mut idx = 0usize;
            let mut val = None;
            for part in parts {
                if idx == col_idx {
                    val = part.parse::<f64>().ok();
                    break;
                }
                idx += 1;
            }
            if let Some(v) = val {
                out.push((elapsed, v));
            }
        }
        Some(out)
    }

    fn refresh_bounds_from_file(&mut self) {
        let Some(path) = self.session_file.as_ref() else {
            return;
        };
        let Ok(file) = File::open(path) else {
            return;
        };
        let reader = BufReader::new(file);
        let mut min_t = self.data_min_sec;
        let mut max_t = self.data_max_sec.max(self.live_sec);
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
        self.data_min_sec = min_t;
        self.data_max_sec = max_t;
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

fn push_point(series: &mut FieldSeries, t: f64, v: f64) {
    if let Some((last_t, last_v)) = series.points.back_mut() {
        if (*last_t - t).abs() < 1e-9 {
            *last_v = v;
            return;
        }
    }
    series.points.push_back((t, v));
}

fn trim_live(series: &mut FieldSeries, t_now: f64) {
    let min_t = t_now - LIVE_BUFFER_SEC;
    while series
        .points
        .front()
        .is_some_and(|(t, _)| *t < min_t)
    {
        series.points.pop_front();
    }
    while series.points.len() > LIVE_MAX_POINTS_PER_FIELD {
        series.points.pop_front();
    }
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

fn merge_sorted(a: Vec<(f64, f64)>, b: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let mut i = 0usize;
    let mut j = 0usize;
    while i < a.len() && j < b.len() {
        if a[i].0 <= b[j].0 {
            out.push(a[i]);
            i += 1;
        } else {
            out.push(b[j]);
            j += 1;
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
    out
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

fn parse_field_column(header: &str, field: &str) -> Option<usize> {
    parse_header_fields(header)
        .into_iter()
        .position(|name| name == field)
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
            page_read_has_page_index: true,
            page_chunk_write_has_page_index: true,
            config_fields: HashMap::new(),
            ts_commands: HashMap::new(),
            inter_write_delay_ms: 10,
            page_activation_delay_ms: 500,
        }
    }

    #[test]
    fn query_live_and_file() {
        let dir = std::env::temp_dir().join(format!("rusefui-timeline-{}", now_ms()));
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
}
