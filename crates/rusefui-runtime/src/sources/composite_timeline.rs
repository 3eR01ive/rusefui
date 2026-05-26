//! Просмотр записанного composite/trigger log (viewer-proxy, ось `elapsed_sec`).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use serde::Serialize;

use super::composite_logger::CompositeEventJson;
use super::output_timeline::OutputTimelineViewControl;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompositeTimelineMode {
    Empty,
    Live,
    File,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositeTimelineStatus {
    pub mode: CompositeTimelineMode,
    pub follow_live: bool,
    pub data_min_sec: f64,
    pub data_max_sec: f64,
    pub view_end_sec: f64,
    pub span_sec: f64,
    pub session_log_path: Option<String>,
    pub event_count: usize,
    pub session_start_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositeTimelineView {
    pub t_min: f64,
    pub t_max: f64,
    pub follow_live: bool,
    pub events: Vec<CompositeEventJson>,
}

#[derive(Debug, Clone)]
pub struct CompositeTimelineViewQuery {
    pub pixel_width: u32,
    /// Окно оси времени с output log (секунды `elapsed_sec`); без привязки к данным trigger.
    pub view_end_sec: Option<f64>,
    pub span_sec: Option<f64>,
}

struct StoredEvent {
    elapsed_sec: f64,
    event: CompositeEventJson,
}

pub struct CompositeTimeline {
    events: Vec<StoredEvent>,
    session_file: Option<PathBuf>,
    session_start_ms: Option<u64>,
    follow_live: bool,
    data_min_sec: f64,
    data_max_sec: f64,
    view_end_sec: f64,
    span_sec: f64,
    /// Live-захват (ещё идёт запись в ring) — граница «сейчас».
    live_max_sec: f64,
}

impl Default for CompositeTimeline {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            session_file: None,
            session_start_ms: None,
            follow_live: false,
            data_min_sec: 0.0,
            data_max_sec: 0.0,
            view_end_sec: 0.0,
            span_sec: 0.5,
            live_max_sec: 0.0,
        }
    }
}

impl CompositeTimeline {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn set_live_capture(&mut self, active: bool) {
        if active {
            self.follow_live = true;
            self.live_max_sec = 0.0;
        } else {
            self.follow_live = false;
            self.refresh_bounds();
            self.view_end_sec = self.data_max_sec;
            self.clamp_view_end();
        }
    }

    pub fn update_live_from_events(&mut self, events: &[CompositeEventJson]) {
        if events.is_empty() {
            return;
        }
        let t_us0 = events[0].t_us;
        let t_last = events[events.len() - 1].t_us;
        self.live_max_sec = (t_last.saturating_sub(t_us0)) as f64 / 1_000_000.0;
        if self.follow_live {
            self.view_end_sec = self.live_max_sec;
        }
        self.data_max_sec = self.live_max_sec;
    }

    pub fn load_file(&mut self, path: PathBuf) -> Result<(), String> {
        let parsed = parse_composite_csv(&path)?;
        self.session_file = Some(path);
        self.session_start_ms = parsed.session_start_ms;
        self.events = parsed.events;
        self.follow_live = false;
        self.refresh_bounds();
        self.view_end_sec = self.data_max_sec;
        self.span_sec = self.data_duration_sec().min(30.0).max(0.5);
        self.clamp_view_end();
        Ok(())
    }

    pub fn session_log_path(&self) -> Option<String> {
        self.session_file.as_ref().map(|p| p.display().to_string())
    }

    pub fn apply_view_control(
        &mut self,
        ctrl: OutputTimelineViewControl,
    ) -> CompositeTimelineStatus {
        if let Some(follow) = ctrl.follow_live {
            self.follow_live = follow;
            if follow {
                self.view_end_sec = self.live_max_sec.max(self.data_max_sec);
            }
        }
        if let Some(span) = ctrl.span_sec {
            self.span_sec = span.clamp(0.005, 3600.0);
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
                self.span_sec = (self.span_sec / zoom).clamp(0.005, 3600.0);
                self.clamp_span_to_data();
            }
        }
        self.clamp_view_end();
        self.status()
    }

    pub fn status(&self) -> CompositeTimelineStatus {
        let mode = if self.session_file.is_some() {
            CompositeTimelineMode::File
        } else if self.follow_live || self.live_max_sec > 0.0 {
            CompositeTimelineMode::Live
        } else {
            CompositeTimelineMode::Empty
        };
        CompositeTimelineStatus {
            mode,
            follow_live: self.follow_live,
            data_min_sec: self.data_min_sec,
            data_max_sec: self.data_max_sec,
            view_end_sec: self.view_end_sec,
            span_sec: self.span_sec,
            session_log_path: self.session_log_path(),
            event_count: self.events.len(),
            session_start_ms: self.session_start_ms,
        }
    }

    pub fn query_view(&self, q: &CompositeTimelineViewQuery) -> CompositeTimelineView {
        let (t_min, t_max) = if let (Some(end), Some(span)) = (q.view_end_sec, q.span_sec) {
            let span = span.clamp(0.005, 3600.0);
            let end = if end.is_finite() { end } else { 0.0 };
            let t_max = end;
            let t_min = t_max - span;
            (t_min, t_max)
        } else {
            let mut t_max = if self.follow_live {
                self.live_max_sec.max(self.data_max_sec)
            } else {
                self.view_end_sec
            };
            let t_min = (t_max - self.span_sec).max(self.data_min_sec);
            if t_max <= t_min + 1e-12 {
                t_max = t_min + self.span_sec;
            }
            (t_min, t_max)
        };

        let max_points = q.pixel_width.max(64).min(4096) as usize;
        let mut out: Vec<CompositeEventJson> = self
            .events
            .iter()
            .filter(|e| e.elapsed_sec >= t_min && e.elapsed_sec <= t_max)
            .map(|e| {
                let mut ev = e.event.clone();
                // Ось графика = elapsed_sec (как output log), в µs для CompositeChart.
                ev.t_us = (e.elapsed_sec * 1_000_000.0).round() as u64;
                ev
            })
            .collect();

        if out.len() > max_points {
            let step = (out.len() + max_points - 1) / max_points;
            out = out.into_iter().step_by(step).collect();
        }

        CompositeTimelineView {
            t_min,
            t_max,
            follow_live: self.follow_live,
            events: out,
        }
    }

    fn refresh_bounds(&mut self) {
        if self.events.is_empty() {
            self.data_min_sec = 0.0;
            self.data_max_sec = 0.0;
            return;
        }
        self.data_min_sec = self.events.first().map(|e| e.elapsed_sec).unwrap_or(0.0);
        self.data_max_sec = self.events.last().map(|e| e.elapsed_sec).unwrap_or(0.0);
    }

    fn data_duration_sec(&self) -> f64 {
        (self.data_max_sec - self.data_min_sec).max(0.001)
    }

    fn clamp_span_to_data(&mut self) {
        let max_span = self.data_duration_sec();
        if self.span_sec > max_span {
            self.span_sec = max_span.max(0.005);
        }
    }

    fn view_end_range(&self) -> (f64, f64) {
        let hi = self.data_max_sec.max(self.live_max_sec);
        let lo = (self.data_min_sec + self.span_sec).min(hi);
        (lo, hi)
    }

    fn clamp_view_end(&mut self) {
        self.clamp_span_to_data();
        let (lo, hi) = self.view_end_range();
        self.view_end_sec = clamp_f64(self.view_end_sec, lo, hi);
    }
}

fn clamp_f64(v: f64, lo: f64, hi: f64) -> f64 {
    v.clamp(lo, hi)
}

struct ParsedCsv {
    session_start_ms: Option<u64>,
    events: Vec<StoredEvent>,
}

fn parse_composite_csv(path: &PathBuf) -> Result<ParsedCsv, String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let reader = BufReader::new(file);
    let mut session_start_ms = None;
    let mut events = Vec::new();

    for line in reader.lines() {
        let line = line.map_err(map_io)?;
        if line.starts_with("# session_start_ms=") {
            if let Some(rest) = line.strip_prefix("# session_start_ms=") {
                session_start_ms = rest.trim().parse().ok();
            }
            continue;
        }
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if line.starts_with("elapsed_sec,") {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 8 {
            continue;
        }
        let elapsed_sec: f64 = parts[0].parse().map_err(|_| "elapsed_sec".to_string())?;
        let t_us: u64 = parts[1].parse().map_err(|_| "t_us".to_string())?;
        let pri = parts[2] == "1";
        let sec = parts[3] == "1";
        let trg = parts[4] == "1";
        let sync = parts[5] == "1";
        let coil = parts[6] == "1";
        let inj = parts[7] == "1";
        let tdc_cycle = parts.get(8).and_then(|s| s.parse::<u64>().ok());

        events.push(StoredEvent {
            elapsed_sec,
            event: CompositeEventJson {
                t_us,
                pri,
                sec,
                trg,
                sync,
                coil,
                inj,
                tdc_cycle,
            },
        });
    }

    if events.is_empty() {
        return Err("в файле нет событий".into());
    }

    Ok(ParsedCsv {
        session_start_ms,
        events,
    })
}

fn map_io(e: std::io::Error) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::composite_logger::CompositeEventJson;

    #[test]
    fn query_with_output_viewport_allows_empty_window() {
        let mut tl = CompositeTimeline::default();
        tl.events.push(StoredEvent {
            elapsed_sec: 10.0,
            event: CompositeEventJson {
                t_us: 10_000_000,
                pri: true,
                sec: false,
                trg: false,
                sync: false,
                coil: false,
                inj: false,
                tdc_cycle: None,
            },
        });
        tl.refresh_bounds();

        let view = tl.query_view(&CompositeTimelineViewQuery {
            pixel_width: 800,
            view_end_sec: Some(0.8),
            span_sec: Some(0.5),
        });
        assert!((view.t_min - 0.3).abs() < 1e-9);
        assert!((view.t_max - 0.8).abs() < 1e-9);
        assert!(view.events.is_empty());
    }
}
