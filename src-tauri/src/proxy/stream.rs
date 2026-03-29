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

        // Anthropic: message_start → message.usage.input_tokens
        if let Some(it) = json
            .pointer("/message/usage/input_tokens")
            .and_then(|v| v.as_u64())
        {
            self.input_tokens = it;
        }
        // Anthropic: message_delta may include full usage (input + output)
        if let Some(it) = json
            .pointer("/usage/input_tokens")
            .and_then(|v| v.as_u64())
        {
            self.input_tokens = it;
        }
        if let Some(ot) = json
            .pointer("/usage/output_tokens")
            .and_then(|v| v.as_u64())
        {
            self.output_tokens = ot;
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

// ---------------------------------------------------------------------------
// BridgeTransformStream – OpenAI SSE → Anthropic Messages SSE, real-time
// ---------------------------------------------------------------------------

/// Transforms an upstream OpenAI chat-completions SSE stream into Anthropic
/// Messages SSE format in real time. Also extracts usage and sends it via
/// oneshot when the stream completes.
pub struct BridgeTransformStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    model: String,
    line_buf: String,
    started: bool,
    block_started: bool,
    usage_prompt: u64,
    usage_completion: u64,
    finish_reason: String,
    done_tx: Option<oneshot::Sender<ExtractedUsage>>,
    finished: bool,
}

impl BridgeTransformStream {
    pub fn new(
        inner: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
        model: String,
        done_tx: oneshot::Sender<ExtractedUsage>,
    ) -> Self {
        Self {
            inner: Box::pin(inner),
            model,
            line_buf: String::new(),
            started: false,
            block_started: false,
            usage_prompt: 0,
            usage_completion: 0,
            finish_reason: "end_turn".into(),
            done_tx: Some(done_tx),
            finished: false,
        }
    }

    fn send_usage(&mut self) {
        if let Some(tx) = self.done_tx.take() {
            let _ = tx.send(ExtractedUsage {
                input_tokens: self.usage_prompt,
                output_tokens: self.usage_completion,
            });
        }
    }

    fn process_lines(&mut self, output: &mut Vec<u8>) {
        while let Some(pos) = self.line_buf.find('\n') {
            let line = self.line_buf[..pos].trim().to_string();
            self.line_buf = self.line_buf[pos + 1..].to_string();
            if line.is_empty() {
                continue;
            }

            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                self.emit_closing(output);
                return;
            }
            let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            self.transform_chunk(&chunk, output);
        }
    }

    fn transform_chunk(&mut self, chunk: &serde_json::Value, output: &mut Vec<u8>) {
        if !self.started {
            let id = chunk
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("msg_bridge");
            write_sse(
                output,
                "message_start",
                &serde_json::json!({
                    "type": "message_start",
                    "message": {
                        "id": id, "type": "message", "role": "assistant",
                        "content": [], "model": &self.model,
                        "stop_reason": serde_json::Value::Null,
                        "stop_sequence": serde_json::Value::Null,
                        "usage": {"input_tokens":0,"output_tokens":0,
                                  "cache_creation_input_tokens":0,"cache_read_input_tokens":0}
                    }
                }),
            );
            write_sse(
                output,
                "content_block_start",
                &serde_json::json!({
                    "type":"content_block_start","index":0,
                    "content_block":{"type":"text","text":""}
                }),
            );
            self.started = true;
            self.block_started = true;
        }

        if let Some(usage) = chunk.get("usage") {
            if let Some(pt) = usage.get("prompt_tokens").and_then(|v| v.as_u64()) {
                self.usage_prompt = pt;
            }
            if let Some(ct) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
                self.usage_completion = ct;
            }
        }

        let Some(choice) = chunk
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
        else {
            return;
        };

        if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            self.finish_reason = match fr {
                "length" => "max_tokens",
                "tool_calls" => "tool_use",
                _ => "end_turn",
            }
            .to_string();
        }

        let delta_text = choice
            .get("delta")
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("");
        if !delta_text.is_empty() {
            write_sse(
                output,
                "content_block_delta",
                &serde_json::json!({
                    "type":"content_block_delta","index":0,
                    "delta":{"type":"text_delta","text":delta_text}
                }),
            );
        }
    }

    fn emit_closing(&mut self, output: &mut Vec<u8>) {
        if self.block_started {
            write_sse(
                output,
                "content_block_stop",
                &serde_json::json!({"type":"content_block_stop","index":0}),
            );
        }
        if self.started {
            write_sse(
                output,
                "message_delta",
                &serde_json::json!({
                    "type":"message_delta",
                    "delta":{"stop_reason":&self.finish_reason,"stop_sequence":serde_json::Value::Null},
                    "usage":{"output_tokens":self.usage_completion}
                }),
            );
            write_sse(
                output,
                "message_stop",
                &serde_json::json!({"type":"message_stop"}),
            );
        }
        self.finished = true;
    }
}

fn write_sse(output: &mut Vec<u8>, event: &str, data: &serde_json::Value) {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = write!(s, "event: {event}\ndata: {data}\n\n");
    output.extend_from_slice(s.as_bytes());
}

impl Stream for BridgeTransformStream {
    type Item = Result<Frame<Bytes>, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if this.finished {
            return Poll::Ready(None);
        }

        loop {
            match this.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    let text = String::from_utf8_lossy(&bytes);
                    this.line_buf.push_str(&text);

                    let mut output = Vec::new();
                    this.process_lines(&mut output);

                    if this.finished {
                        this.send_usage();
                    }

                    if !output.is_empty() {
                        return Poll::Ready(Some(Ok(Frame::data(Bytes::from(output)))));
                    }

                    if this.finished {
                        return Poll::Ready(None);
                    }
                    continue;
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(io::Error::new(io::ErrorKind::Other, e))));
                }
                Poll::Ready(None) => {
                    let mut output = Vec::new();
                    if !this.finished {
                        this.emit_closing(&mut output);
                    }
                    this.send_usage();

                    if output.is_empty() {
                        return Poll::Ready(None);
                    }
                    return Poll::Ready(Some(Ok(Frame::data(Bytes::from(output)))));
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
