//! Общая временная шкала проекта: клипы каналов (начало / конец записи).

use serde::{Deserialize, Serialize};

/// Временная заглушка: длительность клипа без явного `end_ms` (2 мин).
pub const DEFAULT_CLIP_DURATION_MS: u64 = 120_000;

/// Каналы общей шкалы (логические id, не поля INI).
pub mod channel {
    pub const LOGS: &str = "logs";
    pub const TRIGGER: &str = "trigger";
    pub const SPECTROGRAM: &str = "spectrogram";
    pub const RUNS: &str = "runs";

    pub fn all() -> &'static [&'static str] {
        &[LOGS, TRIGGER, SPECTROGRAM, RUNS]
    }

    pub fn is_valid(id: &str) -> bool {
        matches!(id, LOGS | TRIGGER | SPECTROGRAM | RUNS)
    }

    pub fn from_log_kind(kind: &str) -> Option<&'static str> {
        match kind {
            "output_csv" => Some(LOGS),
            "composite_csv" => Some(TRIGGER),
            "spectrogram_live" => Some(SPECTROGRAM),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTimelineRecordRef {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Legacy — не используется на шкале проекта.
    #[serde(default, rename = "offsetSec", skip_serializing)]
    #[allow(dead_code)]
    offset_sec: Option<f64>,
}

impl ProjectTimelineRecordRef {
    pub fn new(path: impl Into<String>, kind: Option<String>) -> Self {
        Self {
            path: path.into(),
            kind,
            offset_sec: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTimelineClip {
    pub id: String,
    pub channel: String,
    /// Начало записи на шкале проекта (Unix ms).
    pub start_ms: u64,
    /// Конец записи; `None` — `start_ms` + [`DEFAULT_CLIP_DURATION_MS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<u64>,
    pub record: ProjectTimelineRecordRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Старый формат (точечные метки) — только для миграции при загрузке.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTimelineMarkerLegacy {
    pub id: String,
    pub timestamp_ms: u64,
    pub channel: String,
    pub label: Option<String>,
    pub record: ProjectTimelineRecordRef,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTimeline {
    #[serde(default)]
    pub clips: Vec<ProjectTimelineClip>,
    #[serde(default, skip_serializing)]
    markers: Vec<ProjectTimelineMarkerLegacy>,
}

impl ProjectTimeline {
    pub fn migrate_legacy(&mut self) {
        if self.clips.is_empty() && !self.markers.is_empty() {
            for m in self.markers.drain(..) {
                self.clips.push(ProjectTimelineClip {
                    id: m.id,
                    channel: m.channel,
                    start_ms: m.timestamp_ms,
                    end_ms: None,
                    record: m.record,
                    label: m.label,
                });
            }
        }
    }
}

pub fn effective_clip_end_ms(clip: &ProjectTimelineClip) -> u64 {
    clip.end_ms
        .unwrap_or(clip.start_ms.saturating_add(DEFAULT_CLIP_DURATION_MS))
}

pub fn clip_with_default_end(mut clip: ProjectTimelineClip) -> ProjectTimelineClip {
    if clip.end_ms.is_none() {
        clip.end_ms = Some(effective_clip_end_ms(&clip));
    }
    clip
}

pub fn validate_channel(channel: &str) -> Result<(), String> {
    if channel::is_valid(channel) {
        Ok(())
    } else {
        Err(format!(
            "Неизвестный канал «{channel}» (ожидается logs | trigger | spectrogram | runs)"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_end_ms_default_duration() {
        let clip = ProjectTimelineClip {
            id: "c1".into(),
            channel: channel::LOGS.into(),
            start_ms: 5000,
            end_ms: None,
            record: ProjectTimelineRecordRef::new("/tmp/a.csv", Some("output_csv".into())),
            label: None,
        };
        assert_eq!(effective_clip_end_ms(&clip), 5000 + DEFAULT_CLIP_DURATION_MS);
        assert_eq!(
            clip_with_default_end(clip).end_ms,
            Some(5000 + DEFAULT_CLIP_DURATION_MS)
        );
    }

    #[test]
    fn clip_json_roundtrip() {
        let c = ProjectTimelineClip {
            id: "c1".into(),
            channel: channel::LOGS.into(),
            start_ms: 1000,
            end_ms: Some(5000),
            record: ProjectTimelineRecordRef {
                path: "/tmp/a.csv".into(),
                kind: Some("output_csv".into()),
                offset_sec: None,
            },
            label: Some("run".into()),
        };
        let text = serde_json::to_string(&c).unwrap();
        let back: ProjectTimelineClip = serde_json::from_str(&text).unwrap();
        assert_eq!(back.end_ms, Some(5000));
    }

    #[test]
    fn legacy_marker_migrates_to_clip() {
        let mut tl = ProjectTimeline {
            clips: vec![],
            markers: vec![ProjectTimelineMarkerLegacy {
                id: "m1".into(),
                timestamp_ms: 42,
                channel: channel::TRIGGER.into(),
                label: None,
                record: ProjectTimelineRecordRef {
                    path: "/x.csv".into(),
                    kind: Some("composite_csv".into()),
                    offset_sec: None,
                },
            }],
        };
        tl.migrate_legacy();
        assert_eq!(tl.clips.len(), 1);
        assert_eq!(tl.clips[0].start_ms, 42);
    }
}
