use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
const MAX_LOG_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const ENABLE_FILE_LOG: bool = true;
const FLUSH_INTERVAL: Duration = Duration::from_secs(2);
const ROTATION_CHECK_WRITES: u32 = 50;

fn log_path() -> PathBuf {
    crate::platform::home_dir()
        .join(".config")
        .join("claude-proxy")
        .join("proxy.log")
}

struct LogWriter {
    writer: BufWriter<File>,
    last_flush: Instant,
    writes_since_check: u32,
}

static LOG_STATE: OnceLock<Mutex<Option<LogWriter>>> = OnceLock::new();

fn log_state() -> &'static Mutex<Option<LogWriter>> {
    LOG_STATE.get_or_init(|| Mutex::new(None))
}

fn maybe_rotate() {
    let path = log_path();
    if let Ok(meta) = std::fs::metadata(&path) {
        if let Ok(modified) = meta.modified() {
            if SystemTime::now()
                .duration_since(modified)
                .map_or(false, |age| age > MAX_LOG_AGE)
            {
                let _ = std::fs::remove_file(&path);
                return;
            }
        }
        if meta.len() > MAX_LOG_BYTES {
            let rotated = path.with_extension("log.1");
            let _ = std::fs::remove_file(&rotated);
            let _ = std::fs::rename(&path, &rotated);
        }
    }
}

fn open_writer() -> Option<LogWriter> {
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()?;
    Some(LogWriter {
        writer: BufWriter::new(file),
        last_flush: Instant::now(),
        writes_since_check: 0,
    })
}

/// File-backed API / proxy logger with a cached buffered writer.
#[derive(Clone, Copy, Default, Debug)]
pub struct ApiLogger;

impl ApiLogger {
    pub fn line(&self, message: &str) {
        write_line(message);
    }

    pub fn request_meta(
        &self,
        provider: &str,
        method: &str,
        path: &str,
        upstream_url: &str,
        model: &str,
        auth_out_masked: &str,
    ) {
        self.line(&format!(
            "req provider={provider} method={method} path={path} upstream={upstream_url} model={model} out_auth={auth_out_masked}",
        ));
    }

    pub fn upstream_status(&self, status: u16, provider: &str, path: &str) {
        self.line(&format!(
            "upstream status={status} provider={provider} path={path}",
        ));
    }

    pub fn upstream_body_preview(
        &self,
        provider: &str,
        status: u16,
        content_type: &str,
        preview: &str,
    ) {
        self.line(&format!(
            "upstream body provider={provider} status={status} ct={content_type} preview={preview}",
        ));
    }

    pub fn upstream_error(&self, provider: &str, path: &str, err: &str) {
        self.line(&format!(
            "upstream_error provider={provider} path={path} err={err}",
        ));
    }

    pub fn stream_response(&self, provider: &str, path: &str) {
        self.line(&format!(
            "upstream stream provider={provider} path={path} (body not buffered)",
        ));
    }
}

fn write_line(message: &str) {
    if !ENABLE_FILE_LOG {
        return;
    }
    let lock = log_state();
    let Ok(mut guard) = lock.lock() else {
        return;
    };

    if guard.is_none() {
        maybe_rotate();
        *guard = open_writer();
    }

    let Some(lw) = guard.as_mut() else {
        return;
    };

    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let _ = writeln!(lw.writer, "[{ts}] {message}");

    if lw.last_flush.elapsed() >= FLUSH_INTERVAL {
        let _ = lw.writer.flush();
        lw.last_flush = Instant::now();
    }

    lw.writes_since_check += 1;
    if lw.writes_since_check >= ROTATION_CHECK_WRITES {
        lw.writes_since_check = 0;
        let _ = lw.writer.flush();
        *guard = None;
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    format!("{}...[truncated {} chars]", &s[..max], s.len() - max)
}

pub fn mask_secret(value: &str) -> String {
    let t = value.trim();
    if t.is_empty() {
        return "<empty>".to_string();
    }
    if t.len() <= 10 {
        return "***".to_string();
    }
    format!("{}***{}", &t[..6], &t[t.len() - 4..])
}
