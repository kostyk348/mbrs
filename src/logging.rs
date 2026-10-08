//! Data logging — the "Log Setup" dialog. Text or CSV, on an interval,
//! with the three end policies the original exposes (Stop / Restart / Continue).

use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogFormat {
    Text,
    Csv,
}

impl LogFormat {
    pub fn label(self) -> &'static str {
        match self {
            LogFormat::Text => "Text file",
            LogFormat::Csv => "CSV (Excel)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogPolicy {
    StopAtEnd,
    RestartAtEnd,
    Continue,
}

impl LogPolicy {
    pub fn label(self) -> &'static str {
        match self {
            LogPolicy::StopAtEnd => "Stop at end",
            LogPolicy::RestartAtEnd => "Restart at end",
            LogPolicy::Continue => "Continue",
        }
    }
    pub const ALL: &'static [LogPolicy] = &[LogPolicy::StopAtEnd, LogPolicy::RestartAtEnd, LogPolicy::Continue];
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogConfig {
    pub enabled: bool,
    pub path: String,
    pub format: LogFormat,
    pub interval_ms: u64,
    pub policy: LogPolicy,
    pub append: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: "/home/lain/mbrs-log.csv".into(),
            format: LogFormat::Csv,
            interval_ms: 1000,
            policy: LogPolicy::Continue,
            append: true,
        }
    }
}

pub struct LogWriter {
    file: Option<BufWriter<File>>,
    path: Option<String>,
    wrote_header: bool,
}

impl Default for LogWriter {
    fn default() -> Self {
        Self { file: None, path: None, wrote_header: false }
    }
}

impl LogWriter {
    pub fn new() -> Self {
        Self::default()
    }

    fn open(&mut self, cfg: &LogConfig) -> std::io::Result<()> {
        let mut o = OpenOptions::new();
        o.create(true).write(true);
        if cfg.append {
            o.append(true);
        } else {
            o.truncate(true);
        }
        let f = o.open(Path::new(&cfg.path))?;
        self.file = Some(BufWriter::new(f));
        self.path = Some(cfg.path.clone());
        self.wrote_header = false;
        Ok(())
    }

    /// Write one row. `header` is only emitted once per file.
    pub fn write_row(&mut self, cfg: &LogConfig, header: &[String], row: &[String]) -> std::io::Result<()> {
        if self.file.is_none() || self.path.as_deref() != Some(cfg.path.as_str()) {
            self.open(cfg)?;
        }
        let f = self.file.as_mut().unwrap();
        let sep = if cfg.format == LogFormat::Csv { ',' } else { '\t' };
        if !self.wrote_header {
            f.write_all(header.join(&sep.to_string()).as_bytes())?;
            f.write_all(b"\n")?;
            self.wrote_header = true;
        }
        f.write_all(row.join(&sep.to_string()).as_bytes())?;
        f.write_all(b"\n")?;
        f.flush()?;
        Ok(())
    }

    pub fn close(&mut self) {
        self.file = None;
        self.path = None;
        self.wrote_header = false;
    }
}
