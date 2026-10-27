use super::manifest::Metric;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const HEADER: &str = "timestamp_ms,plugin,view,metric,unit,value";

/**
 * CSV writer for the pinned metrics panels. One row per metric sample, in long
 * format, so any spreadsheet or plotting tool can pivot it by plugin/view/metric.
 */
pub struct Recorder {
    path: PathBuf,
    writer: BufWriter<File>,
    rows: u64,
}

impl Recorder {
    pub fn create(path: PathBuf) -> std::io::Result<Recorder> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(&path)?;
        let mut writer = BufWriter::new(file);
        writeln!(writer, "{HEADER}")?;
        Ok(Recorder {
            path,
            writer,
            rows: 0,
        })
    }

    /**
     * Uses `IOCC_RECORD` when set, otherwise a timestamped file in the current
     * directory.
     */
    pub fn create_default() -> std::io::Result<Recorder> {
        Recorder::create(default_path())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /**
     * Appends one row per metric of a sample
     */
    pub fn write(&mut self, plugin: &str, view: &str, metrics: &[Metric]) -> std::io::Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        for metric in metrics {
            writeln!(
                self.writer,
                "{timestamp},{},{},{},{},{}",
                field(plugin),
                field(view),
                field(&metric.name),
                field(metric.unit.as_deref().unwrap_or("")),
                metric.value
            )?;
            self.rows += 1;
        }
        // Flush every sample so an unexpected exit does not lose the recording
        self.writer.flush()
    }

    /**
     * Flushes the file and returns its path and the number of data rows
     */
    pub fn finish(mut self) -> std::io::Result<(PathBuf, u64)> {
        self.writer.flush()?;
        Ok((self.path, self.rows))
    }
}

pub fn default_path() -> PathBuf {
    if let Some(path) = std::env::var_os("IOCC_RECORD").filter(|p| !p.is_empty()) {
        return PathBuf::from(path);
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let name = format!("iocc-record-{stamp}.csv");
    std::env::current_dir()
        .map(|dir| dir.join(&name))
        .unwrap_or_else(|_| PathBuf::from(name))
}

/**
 * Quotes a CSV field when it contains a comma, quote or line break
 */
fn field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_long_format_csv() {
        let path =
            std::env::temp_dir().join(format!("iocc-record-{}-writes.csv", std::process::id()));
        let mut recorder = Recorder::create(path.clone()).unwrap();
        recorder
            .write(
                "demo",
                "live",
                &[
                    Metric {
                        name: "Current".into(),
                        value: 1.5,
                        unit: Some("A".into()),
                    },
                    Metric {
                        name: "A,\"b\"".into(),
                        value: 2.0,
                        unit: None,
                    },
                ],
            )
            .unwrap();
        let (saved, rows) = recorder.finish().unwrap();
        assert_eq!(saved, path);
        assert_eq!(rows, 2);

        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], HEADER);
        assert_eq!(lines.len(), 3);
        assert!(lines[1].ends_with(",demo,live,Current,A,1.5"));
        assert!(lines[2].ends_with(",demo,live,\"A,\"\"b\"\"\",,2"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn default_path_can_be_overridden() {
        // SAFETY: this is the only test that changes IOCC_RECORD
        unsafe { std::env::set_var("IOCC_RECORD", "/tmp/iocc-test-record.csv") };
        assert_eq!(default_path(), PathBuf::from("/tmp/iocc-test-record.csv"));
        unsafe { std::env::remove_var("IOCC_RECORD") };
    }
}
