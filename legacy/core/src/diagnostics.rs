use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub timestamp: Instant,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warning => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

pub struct Diagnostics {
    log: Mutex<Vec<LogEntry>>,
    max_entries: usize,
    log_file: Mutex<Option<std::fs::File>>,
    stream_underruns: AtomicU64,
    stream_overruns: AtomicU64,
    routes_created: AtomicU64,
    routes_removed: AtomicU64,
    devices_lost: AtomicU64,
    devices_found: AtomicU64,
    device_reconnects: AtomicU64,
    last_refresh: Mutex<Option<Instant>>,
}

impl Diagnostics {
    pub fn new(max_entries: usize) -> Self {
        Self {
            log: Mutex::new(Vec::with_capacity(max_entries)),
            max_entries,
            log_file: Mutex::new(None),
            stream_underruns: AtomicU64::new(0),
            stream_overruns: AtomicU64::new(0),
            routes_created: AtomicU64::new(0),
            routes_removed: AtomicU64::new(0),
            devices_lost: AtomicU64::new(0),
            devices_found: AtomicU64::new(0),
            device_reconnects: AtomicU64::new(0),
            last_refresh: Mutex::new(None),
        }
    }

    pub fn log(&self, level: LogLevel, message: String) {
        let entry = LogEntry {
            timestamp: Instant::now(),
            level,
            message,
        };
        if let Ok(mut log) = self.log.lock() {
            if log.len() >= self.max_entries {
                log.remove(0);
            }
            log.push(entry.clone());
        }
        if let Ok(mut file_guard) = self.log_file.lock() {
            if let Some(file) = file_guard.as_mut() {
                let _ = writeln!(
                    file,
                    "[{:>8.2}s] [{}] {}",
                    entry.timestamp.elapsed().as_secs_f64(),
                    entry.level,
                    entry.message
                );
            }
        }
    }

    /// Enable appending every log entry to the given file (persistent diagnostics log).
    pub fn set_log_file(&self, path: &Path) -> std::io::Result<()> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        if let Ok(mut file_guard) = self.log_file.lock() {
            *file_guard = Some(file);
        }
        Ok(())
    }

    pub fn info(&self, message: String) {
        self.log(LogLevel::Info, message);
    }

    pub fn warn(&self, message: String) {
        self.log(LogLevel::Warning, message);
    }

    pub fn error(&self, message: String) {
        self.log(LogLevel::Error, message);
    }

    pub fn entries(&self) -> Vec<LogEntry> {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn record_underrun(&self) {
        self.stream_underruns.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_overrun(&self) {
        self.stream_overruns.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_route_created(&self) {
        self.routes_created.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_route_removed(&self) {
        self.routes_removed.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_device_lost(&self) {
        self.devices_lost.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_device_found(&self) {
        self.devices_found.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_device_reconnect(&self) {
        self.device_reconnects.fetch_add(1, Ordering::SeqCst);
    }

    pub fn set_last_refresh(&self) {
        if let Ok(mut last) = self.last_refresh.lock() {
            *last = Some(Instant::now());
        }
    }

    pub fn report(&self) -> DiagnosticsReport {
        DiagnosticsReport {
            stream_underruns: self.stream_underruns.load(Ordering::SeqCst),
            stream_overruns: self.stream_overruns.load(Ordering::SeqCst),
            routes_created: self.routes_created.load(Ordering::SeqCst),
            routes_removed: self.routes_removed.load(Ordering::SeqCst),
            devices_lost: self.devices_lost.load(Ordering::SeqCst),
            devices_found: self.devices_found.load(Ordering::SeqCst),
            device_reconnects: self.device_reconnects.load(Ordering::SeqCst),
            last_refresh: *self.last_refresh.lock().unwrap_or_else(|e| e.into_inner()),
            log_count: self.log.lock().map(|l| l.len()).unwrap_or(0),
        }
    }
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self::new(1000)
    }
}

impl Diagnostics {
    /// Write a human-readable crash/diagnostics report to a file in the temp directory.
    /// Returns the path written, or `None` if the file could not be created.
    pub fn write_crash_report(&self, panic_info: &str) -> Option<PathBuf> {
        let mut path = std::env::temp_dir();
        path.push(format!("tpt-audio-crash-{}.log", std::process::id()));
        self.write_report_to(&path, panic_info)
    }

    /// Write a report to an explicit path. `panic_info` may be empty for a normal dump.
    pub fn write_report_to(&self, path: &Path, panic_info: &str) -> Option<PathBuf> {
        let report = self.report();
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .ok()?;

        let _ = writeln!(file, "tpt-audio diagnostics report");
        let _ = writeln!(file, "===========================");
        if !panic_info.is_empty() {
            let _ = writeln!(file, "Panic: {panic_info}");
            let _ = writeln!(file, "---");
        }
        let _ = writeln!(file, "{report}");
        let _ = writeln!(file, "\nEvent log (oldest first):");
        for entry in self.entries() {
            let _ = writeln!(
                file,
                "[{:>8.2}s] [{}] {}",
                entry.timestamp.elapsed().as_secs_f64(),
                entry.level,
                entry.message
            );
        }
        let _ = file.flush();
        Some(path.to_path_buf())
    }
}

#[derive(Clone, Debug)]
pub struct DiagnosticsReport {
    pub stream_underruns: u64,
    pub stream_overruns: u64,
    pub routes_created: u64,
    pub routes_removed: u64,
    pub devices_lost: u64,
    pub devices_found: u64,
    pub device_reconnects: u64,
    pub last_refresh: Option<Instant>,
    pub log_count: usize,
}

impl std::fmt::Display for DiagnosticsReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Routes: {} created, {} removed | Stream: {} underruns, {} overruns | Devices: {} lost, {} found, {} reconnected | Log: {} entries",
            self.routes_created,
            self.routes_removed,
            self.stream_underruns,
            self.stream_overruns,
            self.devices_lost,
            self.devices_found,
            self.device_reconnects,
            self.log_count,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_basic() {
        let d = Diagnostics::new(10);
        d.info("test message".to_string());
        assert_eq!(d.entries().len(), 1);
        assert_eq!(d.entries()[0].message, "test message");
    }

    #[test]
    fn test_log_overflow() {
        let d = Diagnostics::new(3);
        for i in 0..5 {
            d.info(format!("msg {}", i));
        }
        assert_eq!(d.entries().len(), 3);
        assert_eq!(d.entries()[0].message, "msg 2");
    }

    #[test]
    fn test_counters() {
        let d = Diagnostics::new(100);
        d.record_route_created();
        d.record_route_created();
        d.record_route_removed();
        d.record_device_lost();
        let r = d.report();
        assert_eq!(r.routes_created, 2);
        assert_eq!(r.routes_removed, 1);
        assert_eq!(r.devices_lost, 1);
    }
}
