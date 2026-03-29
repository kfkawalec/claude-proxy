use super::{IncomingAuth, PreparedBody, ProxyBackend, ResolvedAuth, UpstreamTarget};
use crate::proxy::bridge;
use crate::proxy::logging::ApiLogger;
use crate::proxy::ProxyBody;
use crate::state::UsageData;
use bytes::Bytes;
use hyper::{Response, StatusCode};
use reqwest::RequestBuilder;
use std::collections::HashMap;

/// Map Claude model names to upstream (e.g. LiteLLM) model names.
pub fn map_model_to_litellm(model: &str, overrides: &HashMap<String, String>) -> String {
    if let Some(mapped) = overrides.get(model) {
        return mapped.clone();
    }
    let lower = model.to_lowercase();
    if lower.contains("opus") {
        if let Some(mapped) = overrides
            .get("claude_opus")
            .or_else(|| overrides.get("opus"))
        {
            return mapped.clone();
        }
        "gpt-codex".into()
    } else if lower.contains("sonnet") {
        if let Some(mapped) = overrides
            .get("claude_sonnet")
            .or_else(|| overrides.get("sonnet"))
        {
            return mapped.clone();
        }
        "gpt-chat".into()
    } else if lower.contains("haiku") {
        if let Some(mapped) = overrides
            .get("claude_haiku")
            .or_else(|| overrides.get("haiku"))
        {
            return mapped.clone();
        }
        "gpt-mini".into()
    } else {
        "gpt-mini".into()
    }
}

pub fn detect_vision(payload: &serde_json::Value) -> bool {
    payload
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|msgs| {
            msgs.iter().any(|msg| {
                msg.get("content")
                    .and_then(|c| c.as_array())
                    .map(|parts| {
                        parts.iter().any(|p| {
                            let t = p.get("type").and_then(|x| x.as_str()).unwrap_or("");
                            t == "image"
                                || t == "input_image"
                                || p.get("image").is_some()
                                || p.get("source")
                                    .and_then(|s| s.get("type"))
                                    .and_then(|x| x.as_str())
                                    .map(|v| v == "base64" || v == "url")
                                    .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

pub fn normalize_litellm_error(
    log: &ApiLogger,
    resp_bytes: &[u8],
    has_vision: bool,
) -> Option<(Bytes, StatusCode)> {
    let v = parse_litellm_failure(resp_bytes)?;
    let err_info = v.get("error_information").cloned().unwrap_or_default();
    let err_code = err_info
        .get("error_code")
        .and_then(|x| x.as_str())
        .or_else(|| {
            v.get("error")
                .and_then(|e| e.get("code"))
                .and_then(|x| x.as_str())
        })
        .unwrap_or("");
    let err_msg = err_info
        .get("error_message")
        .and_then(|x| x.as_str())
        .or_else(|| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|x| x.as_str())
        })
        .unwrap_or("Upstream provider returned an error");

    let low = err_msg.to_lowercase();
    let hint = if has_vision
        || low.contains("vision")
        || low.contains("image")
        || low.contains("multimodal")
    {
        " Selected model does not support image input (vision). Choose a vision-capable model."
    } else {
        ""
    };

    let normalized = serde_json::json!({
        "type": "error",
        "error": {
            "type": "invalid_request_error",
            "message": format!("Provider request failed ({err_code}): {err_msg}.{hint}"),
        }
    });
    let bytes = serde_json::to_vec(&normalized).ok()?;
    log.line(&format!(
        "normalized_litellm_error code={err_code} vision={has_vision}"
    ));
    Some((Bytes::from(bytes), StatusCode::BAD_REQUEST))
}

fn parse_litellm_failure(bytes: &[u8]) -> Option<serde_json::Value> {
    fn is_failure(v: &serde_json::Value) -> bool {
        let status_fail = v.get("status").and_then(|s| s.as_str()) == Some("failure")
            && v.get("error_information").is_some();
        let error_obj = v
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .map_or(false, |m| !m.is_empty());
        status_fail || error_obj
    }

    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) {
        if is_failure(&v) {
            return Some(v);
        }
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        for line in text.lines() {
            let l = line.trim();
            if let Some(payload) = l.strip_prefix("data:") {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload.trim()) {
                    if is_failure(&v) {
                        return Some(v);
                    }
                }
            }
        }
    }
    None
}

/// LiteLLM / hub: map models, inject API key headers, normalize provider errors.
#[derive(Debug, Default)]
pub struct LitellmBackend;

impl ProxyBackend for LitellmBackend {
    fn id(&self) -> &'static str {
        "litellm"
    }

    fn upstream_target(&self, litellm_endpoint: &str) -> UpstreamTarget {
        let host = litellm_endpoint.trim().trim_end_matches('/');
        let base_url = if host.starts_with("http://") || host.starts_with("https://") {
            host.to_string()
        } else if host.is_empty() {
            "http://localhost:4000".into()
        } else {
            format!("https://{host}")
        };
        UpstreamTarget {
            base_url,
            default_path: "/v1/messages".into(),
        }
    }

    fn should_apply_chat_completions_bridge(&self, path: &str, model: Option<&str>) -> bool {
        bridge::chat_completions_bridge_applies(path, model)
    }

    fn prepare_body(
        &self,
        mut body: Vec<u8>,
        overrides: &HashMap<String, String>,
    ) -> PreparedBody {
        let mut effective_model = None;
        let mut has_vision_input = false;
        if let Ok(mut payload) = serde_json::from_slice::<serde_json::Value>(&body) {
            has_vision_input = detect_vision(&payload);
            if let Some(model) = payload.get("model").and_then(|m| m.as_str()) {
                let mapped = map_model_to_litellm(model, overrides);
                effective_model = Some(mapped.clone());
                payload["model"] = serde_json::Value::String(mapped);
                body = serde_json::to_vec(&payload).unwrap_or(body);
            }
        }
        PreparedBody {
            body,
            effective_model,
            has_vision_input,
        }
    }

    fn resolve_auth(&self, litellm_key: &str, _incoming: &IncomingAuth) -> ResolvedAuth {
        let key = litellm_key.trim().to_string();
        if key.is_empty() {
            return ResolvedAuth {
                key: None,
                bearer: None,
            };
        }
        let bearer = if key.to_lowercase().starts_with("bearer ") {
            key.clone()
        } else {
            format!("Bearer {key}")
        };
        ResolvedAuth {
            key: Some(key),
            bearer: Some(bearer),
        }
    }

    fn should_capture_auth(&self) -> bool {
        false
    }

    fn unauthorized_if_missing_auth(&self, _auth: &ResolvedAuth) -> Option<Response<ProxyBody>> {
        None
    }

    fn apply_upstream_headers(
        &self,
        mut req: RequestBuilder,
        _incoming_headers: &hyper::HeaderMap,
        auth: &ResolvedAuth,
    ) -> RequestBuilder {
        req = req.header("content-type", "application/json");
        if let Some(key) = &auth.key {
            req = req.header("x-api-key", key);
        }
        if let Some(bearer) = &auth.bearer {
            req = req.header("authorization", bearer);
        }
        req
    }

    fn maybe_normalize_error(
        &self,
        log: &ApiLogger,
        bytes: &[u8],
        has_vision: bool,
    ) -> Option<(Bytes, StatusCode)> {
        normalize_litellm_error(log, bytes, has_vision)
    }

    fn log_rate_limit(&self, _log: &ApiLogger, _status: u16, _path: &str) {}

    fn apply_usage_from_buffered_response(
        &self,
        usage: &mut UsageData,
        is_messages: bool,
        effective_model: &Option<String>,
        resp_bytes: &[u8],
    ) {
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(resp_bytes) else {
            return;
        };
        let input = json
            .get("usage")
            .and_then(|u| u.get("input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output = json
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if !is_messages && input == 0 && output == 0 {
            return;
        }

        let pu = usage.by_provider.entry(self.id().to_string()).or_default();
        pu.input_tokens += input;
        pu.output_tokens += output;
        if is_messages {
            pu.requests += 1;
        }
        let key = effective_model.clone().unwrap_or_else(|| "unknown".into());
        let m = pu.per_model.entry(key).or_default();
        m.input_tokens += input;
        m.output_tokens += output;
        if is_messages {
            m.requests += 1;
        }
    }
}
