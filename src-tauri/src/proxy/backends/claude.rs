use super::{IncomingAuth, PreparedBody, ProxyBackend, ResolvedAuth, UpstreamTarget};
use crate::proxy::error::ProxyError;
use crate::proxy::logging::ApiLogger;
use crate::proxy::ProxyBody;
use crate::state::UsageData;
use bytes::Bytes;
use hyper::{Response, StatusCode};
use reqwest::RequestBuilder;
use std::collections::HashMap;

/// Anthropic API upstream: forward client headers, capture auth for the tray.
#[derive(Debug, Default)]
pub struct ClaudeBackend;

impl ProxyBackend for ClaudeBackend {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn upstream_target(&self, _litellm_endpoint: &str) -> UpstreamTarget {
        UpstreamTarget {
            base_url: "https://api.anthropic.com".into(),
            default_path: "/v1/messages".into(),
        }
    }

    fn prepare_body(
        &self,
        body: Vec<u8>,
        _overrides: &HashMap<String, String>,
    ) -> PreparedBody {
        let mut effective_model = None;
        if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body) {
            if let Some(model) = payload.get("model").and_then(|m| m.as_str()) {
                effective_model = Some(model.to_string());
            }
        }
        PreparedBody {
            body,
            effective_model,
            has_vision_input: false,
        }
    }

    fn resolve_auth(&self, _litellm_key: &str, incoming: &IncomingAuth) -> ResolvedAuth {
        ResolvedAuth {
            key: incoming.api_key.as_ref().filter(|k| *k != "dummy").cloned(),
            bearer: incoming.bearer.clone(),
        }
    }

    fn should_capture_auth(&self) -> bool {
        true
    }

    fn unauthorized_if_missing_auth(&self, auth: &ResolvedAuth) -> Option<Response<ProxyBody>> {
        if auth.key.is_none() && auth.bearer.is_none() {
            Some(
                ProxyError::Auth(
                    StatusCode::UNAUTHORIZED,
                    "Brak uwierzytelnienia do Anthropic".into(),
                )
                .into_response(),
            )
        } else {
            None
        }
    }

    fn apply_upstream_headers(
        &self,
        mut req: RequestBuilder,
        incoming_headers: &hyper::HeaderMap,
        _auth: &ResolvedAuth,
    ) -> RequestBuilder {
        for (name, value) in incoming_headers.iter() {
            let n = name.as_str();
            if n.eq_ignore_ascii_case("host")
                || n.eq_ignore_ascii_case("content-length")
                || n.eq_ignore_ascii_case("accept-encoding")
            {
                continue;
            }
            req = req.header(name, value);
        }
        req
    }

    fn maybe_normalize_error(
        &self,
        _log: &ApiLogger,
        _bytes: &[u8],
        _has_vision: bool,
    ) -> Option<(Bytes, StatusCode)> {
        None
    }

    fn log_rate_limit(&self, log: &ApiLogger, status: u16, path: &str) {
        if status == 429 {
            log.line(&format!("claude_429 path={path}"));
        }
    }

    fn apply_usage_from_buffered_response(
        &self,
        usage: &mut UsageData,
        is_messages: bool,
        effective_model: &Option<String>,
        resp_bytes: &[u8],
    ) {
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(resp_bytes) else {
            if is_messages {
                let pu = usage.by_provider.entry(self.id().to_string()).or_default();
                pu.requests += 1;
                let key = effective_model
                    .clone()
                    .unwrap_or_else(|| "unknown".into());
                pu.per_model.entry(key).or_default().requests += 1;
            }
            return;
        };
        let input = json
            .get("usage")
            .map(crate::proxy::stream::sum_anthropic_input)
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
