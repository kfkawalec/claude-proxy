use bytes::Bytes;

use super::logging::ApiLogger;

const BRIDGE_MODELS: &[&str] = &["oss-minimax", "minimax-m2.7"];

/// Models that go through Messages -> OpenAI chat completions rewrite (LiteLLM path only).
pub(crate) fn chat_completions_bridge_applies(path: &str, model: Option<&str>) -> bool {
    path == "/v1/messages" && model.map_or(false, |m| BRIDGE_MODELS.contains(&m))
}

pub struct BridgeRequest {
    pub body: Vec<u8>,
    pub path: String,
    pub stream: bool,
    pub model: String,
}

pub fn rewrite_request(body: &[u8]) -> Option<BridgeRequest> {
    let payload: serde_json::Value = serde_json::from_slice(body).ok()?;
    let stream = payload
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let model = payload.get("model")?.as_str()?.to_string();

    let messages = payload
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|msg| {
                    let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                    let content = flatten_content(msg.get("content"));
                    serde_json::json!({"role": role, "content": content})
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut out = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": stream,
    });
    for key in &["max_tokens", "temperature", "top_p", "stop"] {
        if let Some(v) = payload.get(*key) {
            out[*key] = v.clone();
        }
    }

    let body = serde_json::to_vec(&out).ok()?;
    Some(BridgeRequest {
        body,
        path: "/v1/chat/completions".to_string(),
        stream,
        model,
    })
}

fn flatten_content(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| {
                if p.get("type").and_then(|t| t.as_str()) == Some("text") {
                    p.get("text").and_then(|t| t.as_str()).map(String::from)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Transform a buffered upstream response (OpenAI format) into Anthropic format.
/// Used for the bridge buffered path (non-streaming or non-SSE responses).
pub fn transform_response(
    resp_bytes: &[u8],
    content_type: &str,
    stream_requested: bool,
    model: &str,
) -> Option<(Bytes, String)> {
    if content_type.contains("text/event-stream") && stream_requested {
        let sse = transform_stream_response_buffered(resp_bytes, model);
        return Some((Bytes::from(sse), "text/event-stream; charset=utf-8".into()));
    }

    let chat: serde_json::Value = serde_json::from_slice(resp_bytes).ok()?;
    let anth = chat_to_anthropic(&chat, model);
    let bytes = serde_json::to_vec(&anth).ok()?;
    Some((Bytes::from(bytes), "application/json".into()))
}

/// Buffered SSE transform: OpenAI chat completions SSE -> Anthropic Messages SSE.
/// Only used as fallback when the real-time streaming path cannot be used.
fn transform_stream_response_buffered(raw: &[u8], model: &str) -> Vec<u8> {
    let text = String::from_utf8_lossy(raw);
    let mut output = String::new();
    let mut started = false;
    let mut block_started = false;
    let mut finish_reason = "end_turn".to_string();
    let mut usage_completion = 0u64;

    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("data:") {
            continue;
        }
        let payload = line.trim_start_matches("data:").trim();
        if payload == "[DONE]" {
            break;
        }
        let Ok(chunk) = serde_json::from_str::<serde_json::Value>(payload) else {
            continue;
        };

        if !started {
            let id = chunk
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("msg_bridge");
            output.push_str(&format!(
                "event: message_start\ndata: {}\n\n",
                serde_json::json!({
                    "type": "message_start",
                    "message": {
                        "id": id, "type": "message", "role": "assistant",
                        "content": [], "model": model,
                        "stop_reason": serde_json::Value::Null,
                        "stop_sequence": serde_json::Value::Null,
                        "usage": {"input_tokens":0,"output_tokens":0,
                                  "cache_creation_input_tokens":0,"cache_read_input_tokens":0}
                    }
                })
            ));
            output.push_str(&format!(
                "event: content_block_start\ndata: {}\n\n",
                serde_json::json!({"type":"content_block_start","index":0,
                    "content_block":{"type":"text","text":""}})
            ));
            started = true;
            block_started = true;
        }

        if let Some(usage) = chunk.get("usage") {
            usage_completion = usage
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(usage_completion);
        }

        let choice = chunk
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first());
        let Some(choice) = choice else { continue };

        if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            finish_reason = match fr {
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
            output.push_str(&format!(
                "event: content_block_delta\ndata: {}\n\n",
                serde_json::json!({"type":"content_block_delta","index":0,
                    "delta":{"type":"text_delta","text":delta_text}})
            ));
        }
    }

    if block_started {
        output.push_str(&format!(
            "event: content_block_stop\ndata: {}\n\n",
            serde_json::json!({"type":"content_block_stop","index":0})
        ));
    }
    if started {
        output.push_str(&format!(
            "event: message_delta\ndata: {}\n\n",
            serde_json::json!({
                "type":"message_delta",
                "delta":{"stop_reason":&finish_reason,"stop_sequence":serde_json::Value::Null},
                "usage":{"output_tokens":usage_completion}
            })
        ));
        output.push_str(&format!(
            "event: message_stop\ndata: {}\n\n",
            serde_json::json!({"type":"message_stop"})
        ));
    }

    ApiLogger::default().line(&format!(
        "bridge stream_transformed chunks_emitted={} finish_reason={}",
        output.matches("content_block_delta").count(),
        finish_reason
    ));

    output.into_bytes()
}

fn chat_to_anthropic(resp: &serde_json::Value, model: &str) -> serde_json::Value {
    let choice = resp
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first());
    let text = choice
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let finish = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("stop");
    let stop_reason = match finish {
        "length" => "max_tokens",
        "tool_calls" => "tool_use",
        _ => "end_turn",
    };
    let usage = resp.get("usage").cloned().unwrap_or_default();
    serde_json::json!({
        "id": resp.get("id").and_then(|v| v.as_str()).unwrap_or("msg_bridge"),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{"type":"text","text": text}],
        "stop_reason": stop_reason,
        "stop_sequence": serde_json::Value::Null,
        "usage": {
            "input_tokens": usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            "output_tokens": usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            "cache_creation_input_tokens": 0,
            "cache_read_input_tokens": 0
        }
    })
}
