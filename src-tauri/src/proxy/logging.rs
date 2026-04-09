use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
const MAX_LOG_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// When `CLAUDE_PROXY_LOG_REQUEST_BODY` is `1`/`true`/`yes`, log the outbound request body
/// (after model mapping) for diffing token usage. Can be large and sensitive;
/// avoid sharing logs. File logging is on by default (`proxy.log`); set `CLAUDE_PROXY_FILE_LOG=0` to turn off.
fn request_body_log_enabled() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("CLAUDE_PROXY_LOG_REQUEST_BODY")
            .map(|v| {
                let v = v.trim().to_lowercase();
                v == "1" || v == "true" || v == "yes"
            })
            .unwrap_or(false)
    })
}

fn request_body_log_max_bytes() -> usize {
    static CACHED: OnceLock<usize> = OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("CLAUDE_PROXY_LOG_REQUEST_BODY_MAX")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .filter(|&n| n > 0)
            .unwrap_or(512 * 1024)
    })
}

/// Append proxy lines to `~/.config/claude-proxy/proxy.log` by default.
/// Set `CLAUDE_PROXY_FILE_LOG=0` / `false` / `no` to disable file (then lines go to stdout only).
fn file_log_enabled() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        match std::env::var("CLAUDE_PROXY_FILE_LOG") {
            Ok(v) => {
                let v = v.trim().to_lowercase();
                !(v == "0" || v == "false" || v == "no")
            }
            Err(_) => true,
        }
    })
}

fn log_path() -> PathBuf {
    crate::platform::home_dir()
        .join(".config")
        .join("claude-proxy")
        .join("proxy.log")
}

struct LogWriter {
    writer: BufWriter<std::fs::File>,
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
    })
}

/// File-backed API / proxy logger with a cached buffered writer when file logging is enabled.
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
        let now = chrono::Local::now().format("%H:%M:%S");
        self.line(&format!(
            "[proxy] {now} → {provider} {upstream_url} model={model} {method} {path} auth={auth_out_masked}",
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

    /// Logs body size always; full UTF-8 prefix when `CLAUDE_PROXY_LOG_REQUEST_BODY=1`
    /// (trimmed by `CLAUDE_PROXY_LOG_REQUEST_BODY_MAX`, default 512 KiB).
    pub fn request_body_debug(&self, body: &[u8]) {
        self.line(&format!("request_body_bytes={}", body.len()));
        if !request_body_log_enabled() {
            return;
        }
        let max = request_body_log_max_bytes();
        let s = String::from_utf8_lossy(body);
        let prefix = truncate_bytes_utf8(&s, max);
        let suffix = if s.len() > prefix.len() {
            format!(" ...[truncated total={} bytes]", s.len())
        } else {
            String::new()
        };
        self.line(&format!("request_body_utf8={prefix}{suffix}"));
    }
}

fn write_line(message: &str) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{ts}] {message}");

    if file_log_enabled() {
        let lock = log_state();
        let Ok(mut guard) = lock.lock() else {
            return;
        };
        if guard.is_none() {
            maybe_rotate();
            *guard = open_writer();
        }
        if let Some(lw) = guard.as_mut() {
            let _ = writeln!(lw.writer, "{line}");
            let _ = lw.writer.flush();
        }
    } else {
        println!("{line}");
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let end = truncate_end_char_boundary(s, max);
    format!("{}...[truncated {} bytes]", &s[..end], s.len() - end)
}

fn truncate_end_char_boundary(s: &str, max_bytes: usize) -> usize {
    if s.len() <= max_bytes {
        return s.len();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn truncate_bytes_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let end = truncate_end_char_boundary(s, max_bytes);
    s[..end].to_string()
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
