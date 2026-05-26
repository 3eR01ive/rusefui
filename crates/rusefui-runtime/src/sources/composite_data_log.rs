use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusefi_protocol::ConnectionInfo;

use super::composite_logger::CompositeEventJson;

const FLUSH_EVERY_ROWS: u64 = 200;

pub fn composite_logs_dir() -> PathBuf {
    if let Ok(path) = std::env::var("RUSEFUI_COMPOSITE_LOG_DIR") {
        return PathBuf::from(path);
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rusefui")
        .join("composite_logs")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn slug_part(raw: &str, max_len: usize) -> String {
    let mut out = String::with_capacity(max_len.min(raw.len()));
    for c in raw.chars() {
        if out.len() >= max_len {
            break;
        }
        let ch = if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
            c
        } else {
            '_'
        };
        out.push(ch);
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "ecu".into()
    } else {
        trimmed.to_string()
    }
}

/// `trigger_{recording_ms}_{port}_{sig}.csv` — уникально на каждый «Старт».
fn open_unique_trigger_file(
    dir: &Path,
    port: &str,
    sig: &str,
) -> Result<(File, PathBuf), String> {
    let mut recording_ms = now_ms();
    for attempt in 0..64 {
        let path = dir.join(format!("trigger_{recording_ms}_{port}_{sig}.csv"));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
        {
            Ok(file) => return Ok((file, path)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                recording_ms = now_ms().saturating_add(attempt + 1);
            }
            Err(e) => return Err(format!("create {}: {e}", path.display())),
        }
    }
    Err("не удалось выделить уникальное имя trigger log".into())
}

fn map_io(e: std::io::Error) -> String {
    e.to_string()
}

/// CSV trigger/composite log — ось `elapsed_sec` совпадает с output log (от `session_start_ms` + Δt_us).
pub struct CompositeDataLogWriter {
    path: PathBuf,
    session_start_ms: u64,
    t_us_anchor: Option<u64>,
    /// elapsed_sec в CSV = эта база + Δt_us (как у output log по сессии).
    recording_base_elapsed_sec: Option<f64>,
    writer: BufWriter<File>,
    rows: u64,
}

impl CompositeDataLogWriter {
    pub fn open(info: &ConnectionInfo, ini_path: Option<&Path>) -> Result<Self, String> {
        Self::open_at(info, ini_path, now_ms())
    }

    pub fn open_at(
        info: &ConnectionInfo,
        ini_path: Option<&Path>,
        session_start_ms: u64,
    ) -> Result<Self, String> {
        let dir = composite_logs_dir();
        fs::create_dir_all(&dir).map_err(|e| format!("composite_logs dir: {e}"))?;

        let port = slug_part(&info.port_name, 32);
        let sig = slug_part(&info.signature, 40);
        let (file, path) = open_unique_trigger_file(&dir, &port, &sig)?;

        let mut writer = BufWriter::new(file);
        writeln!(writer, "# rusefui composite / trigger log").map_err(map_io)?;
        writeln!(writer, "# session_start_ms={session_start_ms}").map_err(map_io)?;
        writeln!(
            writer,
            "# port={} baud={}",
            info.port_name, info.baud_rate
        )
        .map_err(map_io)?;
        writeln!(writer, "# signature={}", info.signature).map_err(map_io)?;
        if let Some(p) = ini_path {
            writeln!(writer, "# ini={}", p.display()).map_err(map_io)?;
        }
        writeln!(
            writer,
            "# columns: elapsed_sec — секунды от session_start_ms (как output log); t_us — ECU"
        )
        .map_err(map_io)?;
        writeln!(
            writer,
            "elapsed_sec,t_us,pri,sec,trg,sync,coil,inj,tdc_cycle"
        )
        .map_err(map_io)?;
        writer.flush().map_err(map_io)?;

        Ok(Self {
            path,
            session_start_ms,
            t_us_anchor: None,
            recording_base_elapsed_sec: None,
            writer,
            rows: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn session_start_ms(&self) -> u64 {
        self.session_start_ms
    }

    pub fn rows(&self) -> u64 {
        self.rows
    }

    fn session_elapsed_for(&mut self, t_us: u64) -> f64 {
        let anchor = *self.t_us_anchor.get_or_insert(t_us);
        if self.recording_base_elapsed_sec.is_none() {
            let now_ms = now_ms();
            self.recording_base_elapsed_sec = Some(
                (now_ms.saturating_sub(self.session_start_ms)) as f64 / 1000.0,
            );
        }
        let base = self.recording_base_elapsed_sec.unwrap_or(0.0);
        base + (t_us.saturating_sub(anchor)) as f64 / 1_000_000.0
    }

    pub fn write_events(&mut self, events: &[CompositeEventJson]) {
        for ev in events {
            let elapsed = self.session_elapsed_for(ev.t_us);
            let tdc = ev.tdc_cycle.map(|n| n.to_string()).unwrap_or_default();
            if writeln!(
                self.writer,
                "{elapsed:.6},{},{},{},{},{},{},{},{}",
                ev.t_us,
                ev.pri as u8,
                ev.sec as u8,
                ev.trg as u8,
                ev.sync as u8,
                ev.coil as u8,
                ev.inj as u8,
                tdc,
            )
            .is_err()
            {
                return;
            }
            self.rows += 1;
        }
        if self.rows.is_multiple_of(FLUSH_EVERY_ROWS) {
            let _ = self.writer.flush();
        }
    }

    pub fn close(mut self) -> Result<(PathBuf, u64), String> {
        self.writer.flush().map_err(map_io)?;
        Ok((self.path.clone(), self.rows))
    }
}
