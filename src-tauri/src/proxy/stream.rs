use bytes::Bytes;
use futures_util::Stream;
use http_body::Frame;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::oneshot;

/// Token usage extracted from an SSE stream.
#[derive(Debug, Default)]
pub struct ExtractedUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Sum `input_tokens + cache_creation_input_tokens + cache_read_input_tokens`
/// from an Anthropic usage JSON object.
pub fn sum_anthropic_input(usage: &serde_json::Value) -> u64 {
    let base = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_create = usage
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    base + cache_create + cache_read
}

/// Input/output from a JSON `usage` object (Anthropic with cache vs OpenAI-style `prompt_tokens`).
pub fn usage_io_from_usage_obj(usage: &serde_json::Value) -> (u64, u64) {
    let has_anthropic_shape = usage.get("input_tokens").is_some()
        || usage.get("cache_read_input_tokens").is_some()
        || usage.get("cache_creation_input_tokens").is_some();
    let input = if has_anthropic_shape {
        sum_anthropic_input(usage)
    } else {
        usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
    };
    let output = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| usage.get("completion_tokens").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    (input, output)
}

pub fn usage_from_response_json(json: &serde_json::Value) -> (u64, u64) {
    match json.get("usage") {
        Some(u) => usage_io_from_usage_obj(u),
        None => (0, 0),
    }
}

// ---------------------------------------------------------------------------
// SSE usage scanner (shared logic)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct UsageScanner {
    line_buf: String,
    input_tokens: u64,
    output_tokens: u64,
}

impl UsageScanner {
    fn scan(&mut self, chunk: &[u8]) {
        let text = String::from_utf8_lossy(chunk);
        self.line_buf.push_str(&text);

        while let Some(pos) = self.line_buf.find('\n') {
            let line = self.line_buf[..pos].trim().to_string();
            self.line_buf = self.line_buf[pos + 1..].to_string();
            self.process_line(&line);
        }
    }

    fn process_line(&mut self, line: &str) {
        let Some(data) = line.strip_prefix("data:") else {
            return;
        };
        let data = data.trim();
        if data == "[DONE]" {
            return;
        }
        let Ok(json) = serde_json::from_str::<serde_json::Value>(data) else {
            return;
        };

        // Anthropic: message_start → message.usage
        if let Some(usage) = json.pointer("/message/usage") {
            self.input_tokens = sum_anthropic_input(usage);
        }
        // Anthropic: message_delta → usage (may override)
        if let Some(usage) = json.get("usage") {
            if usage.get("input_tokens").is_some() {
                self.input_tokens = sum_anthropic_input(usage);
            }
            if let Some(ot) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                self.output_tokens = ot;
            }
        }
        // OpenAI: usage.prompt_tokens / completion_tokens
        if let Some(pt) = json
            .pointer("/usage/prompt_tokens")
            .and_then(|v| v.as_u64())
        {
            self.input_tokens = pt;
        }
        if let Some(ct) = json
            .pointer("/usage/completion_tokens")
            .and_then(|v| v.as_u64())
        {
            self.output_tokens = ct;
        }
    }

    fn finish(&self) -> ExtractedUsage {
        ExtractedUsage {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
        }
    }
}

// ---------------------------------------------------------------------------
// UsageCapturingStream – native SSE passthrough with usage extraction
// ---------------------------------------------------------------------------

/// Wraps an SSE byte stream, passing all data through unchanged while
/// extracting usage. Sends [`ExtractedUsage`] via oneshot when the stream ends.
pub struct UsageCapturingStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    scanner: UsageScanner,
    done_tx: Option<oneshot::Sender<ExtractedUsage>>,
}

impl UsageCapturingStream {
    pub fn new(
        inner: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
        done_tx: oneshot::Sender<ExtractedUsage>,
    ) -> Self {
        Self {
            inner: Box::pin(inner),
            scanner: UsageScanner::default(),
            done_tx: Some(done_tx),
        }
    }

    fn send_usage(&mut self) {
        if let Some(tx) = self.done_tx.take() {
            let _ = tx.send(self.scanner.finish());
        }
    }
}

impl Stream for UsageCapturingStream {
    type Item = Result<Frame<Bytes>, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                this.scanner.scan(&bytes);
                Poll::Ready(Some(Ok(Frame::data(bytes))))
            }
            Poll::Ready(Some(Err(e))) => {
                Poll::Ready(Some(Err(io::Error::new(io::ErrorKind::Other, e))))
            }
            Poll::Ready(None) => {
                this.send_usage();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
