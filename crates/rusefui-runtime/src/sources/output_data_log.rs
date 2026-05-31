use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusefi_protocol::ConnectionInfo;

use super::output_channels::IniContext;

const FLUSH_EVERY_ROWS: u64 = 50;

pub fn output_logs_dir() -> PathBuf {
    if let Ok(path) = std::env::var("RUSEFUI_OUTPUT_LOG_DIR") {
        return PathBuf::from(path);
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rusefui")
        .join("output_logs")
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
    if out.is_empty() {
        "ecu".into()
    } else {
        out
    }
}

fn fmt_val(v: f64) -> String {
    if v.is_finite() {
        format!("{v:.6}")
    } else {
        String::new()
    }
}

/// CSV-лог output channels на одну сессию подключения ECU.
pub struct OutputDataLogWriter {
    path: PathBuf,
    started_ms: u64,
    field_names: Vec<String>,
    writer: BufWriter<File>,
    rows: u64,
}

impl OutputDataLogWriter {
    pub fn open(
        info: &ConnectionInfo,
        ini: &IniContext,
        ini_path: Option<&Path>,
    ) -> Result<Self, String> {
        Self::open_at(info, ini, ini_path, now_ms())
    }

    pub fn open_at(
        info: &ConnectionInfo,
        ini: &IniContext,
        ini_path: Option<&Path>,
        started_ms: u64,
    ) -> Result<Self, String> {
        let dir = output_logs_dir();
        fs::create_dir_all(&dir).map_err(|e| format!("output_logs dir: {e}"))?;

        let port = slug_part(&info.port_name, 32);
        let sig = slug_part(&info.signature, 40);
        let path = dir.join(format!("output_{started_ms}_{port}_{sig}.csv"));

        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|e| format!("create {}: {e}", path.display()))?;

        let mut writer = BufWriter::new(file);
        writeln!(writer, "# rusefui output channels log").map_err(map_io)?;
        writeln!(writer, "# started_ms={started_ms}").map_err(map_io)?;
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
        writeln!(writer, "# field_count={}", ini.channels.fields.len()).map_err(map_io)?;

        let field_names: Vec<String> = ini
            .channels
            .fields
            .iter()
            .map(|f| f.name.clone())
            .collect();

        write!(writer, "timestamp_ms,elapsed_sec").map_err(map_io)?;
        for name in &field_names {
            write!(writer, ",{name}").map_err(map_io)?;
        }
        writeln!(writer).map_err(map_io)?;
        writer.flush().map_err(map_io)?;

        Ok(Self {
            path,
            started_ms,
            field_names,
            writer,
            rows: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn rows(&self) -> u64 {
        self.rows
    }

    pub fn write_sample(&mut self, timestamp_ms: u64, values: &HashMap<String, f64>) {
        let elapsed_sec = (timestamp_ms.saturating_sub(self.started_ms)) as f64 / 1000.0;
        if write!(self.writer, "{timestamp_ms},{elapsed_sec:.6}").is_err() {
            return;
        }
        for name in &self.field_names {
            if write!(self.writer, ",").is_err() {
                return;
            }
            if let Some(v) = values.get(name) {
                if write!(self.writer, "{}", fmt_val(*v)).is_err() {
                    return;
                }
            }
        }
        if writeln!(self.writer).is_err() {
            return;
        }
        self.rows += 1;
        if self.rows.is_multiple_of(FLUSH_EVERY_ROWS) {
            let _ = self.writer.flush();
        }
    }

    pub fn close(mut self) -> Result<(PathBuf, u64), String> {
        self.writer.flush().map_err(map_io)?;
        Ok((self.path.clone(), self.rows))
    }
}

fn map_io(e: std::io::Error) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusefi_ini::{
        FieldKind, OutputChannelField, OutputChannels, ScalarField, ScalarType,
    };
    use std::sync::Arc;

    fn test_ini() -> IniContext {
        IniContext {
            signature: Some("test.sig".into()),
            channels: Arc::new(OutputChannels {
                och_block_size: 256,
                fields: vec![
                    OutputChannelField {
                        name: "RPMValue".into(),
                        kind: FieldKind::Scalar(ScalarField {
                            ty: ScalarType::U16,
                            offset: 0,
                            page: 0,
                            units: "rpm".into(),
                            scale: 1.0,
                            translate: 0.0,
                        }),
                    },
                    OutputChannelField {
                        name: "coolant".into(),
                        kind: FieldKind::Scalar(ScalarField {
                            ty: ScalarType::S16,
                            offset: 4,
                            page: 0,
                            units: "C".into(),
                            scale: 1.0,
                            translate: 0.0,
                        }),
                    },
                ],
                by_name: HashMap::new(),
            }),
            block_size: 256,
            blocking_factor: 256,
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
    fn writes_csv_rows() {
        let dir = std::env::temp_dir().join(format!("rusefui-outlog-{}", now_ms()));
        std::env::set_var("RUSEFUI_OUTPUT_LOG_DIR", &dir);

        let info = ConnectionInfo {
            port_name: "/dev/ttyUSB0".into(),
            baud_rate: 115_200,
            signature: "rusEFI test".into(),
            handshake_command: 'S',
        };
        let mut log = OutputDataLogWriter::open(&info, &test_ini(), None).unwrap();
        let mut values = HashMap::new();
        values.insert("RPMValue".into(), 1200.0);
        values.insert("coolant".into(), -6.25);
        log.write_sample(1_000, &values);
        let (path, rows) = log.close().unwrap();
        assert_eq!(rows, 1);
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("timestamp_ms,elapsed_sec,RPMValue,coolant"));
        assert!(text.contains("1000,0.000000,1200.000000,-6.250000"));

        std::env::remove_var("RUSEFUI_OUTPUT_LOG_DIR");
        let _ = fs::remove_dir_all(dir);
    }
}
