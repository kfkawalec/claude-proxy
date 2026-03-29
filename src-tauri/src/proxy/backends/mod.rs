//! Upstream backends per provider (`claude`, `litellm`, ...).
mod auth;

pub use auth::{extract_auth, IncomingAuth, ResolvedAuth};

pub mod claude;
pub mod litellm;

use claude::ClaudeBackend;
use litellm::LitellmBackend;

use crate::proxy::logging::ApiLogger;
use crate::proxy::ProxyBody;
use crate::state::UsageData;
use bytes::Bytes;
use hyper::{Response, StatusCode};
use reqwest::RequestBuilder;
use std::collections::HashMap;

/// Result of parsing / rewriting the request body before forwarding upstream.
pub struct PreparedBody {
    pub body: Vec<u8>,
    pub effective_model: Option<String>,
    pub has_vision_input: bool,
}

/// Resolved upstream target: base URL and default path.
pub struct UpstreamTarget {
    pub base_url: String,
    pub default_path: String,
}

/// Per-provider behavior: headers, auth, error shaping, usage accounting.
pub trait ProxyBackend: Send + Sync {
    fn id(&self) -> &'static str;

    /// Resolve the upstream base URL and default request path for this backend.
    fn upstream_target(&self, litellm_endpoint: &str) -> UpstreamTarget;

    fn prepare_body(
        &self,
        body: Vec<u8>,
        overrides: &HashMap<String, String>,
    ) -> PreparedBody;

    fn resolve_auth(&self, litellm_key: &str, incoming: &IncomingAuth) -> ResolvedAuth;

    /// If true, successful requests may update [`crate::state::CapturedClaudeAuth`].
    fn should_capture_auth(&self) -> bool;

    /// Return `Some` when the request must be rejected before upstream (e.g. missing Anthropic auth).
    fn unauthorized_if_missing_auth(&self, auth: &ResolvedAuth) -> Option<Response<ProxyBody>>;

    fn apply_upstream_headers(
        &self,
        req: RequestBuilder,
        incoming_headers: &hyper::HeaderMap,
        auth: &ResolvedAuth,
    ) -> RequestBuilder;

    /// LiteLLM maps provider errors into Anthropic-shaped JSON; Claude passes through.
    fn maybe_normalize_error(
        &self,
        log: &ApiLogger,
        bytes: &[u8],
        has_vision: bool,
    ) -> Option<(Bytes, StatusCode)>;

    /// e.g. Claude logs HTTP 429 from Anthropic.
    fn log_rate_limit(&self, log: &ApiLogger, status: u16, path: &str);

    fn apply_usage_from_buffered_response(
        &self,
        usage: &mut UsageData,
        is_messages: bool,
        effective_model: &Option<String>,
        resp_bytes: &[u8],
    );

    /// Anthropic Messages -> OpenAI chat completions bridge (LiteLLM-only).
    fn should_apply_chat_completions_bridge(&self, path: &str, model: Option<&str>) -> bool {
        let _ = (path, model);
        false
    }
}

static CLAUDE: ClaudeBackend = ClaudeBackend;
static LITELLM: LitellmBackend = LitellmBackend;

/// Resolve backend by configured provider id. Unknown ids fall back to Anthropic-compatible (`claude`).
pub fn backend_for(provider_id: &str) -> &'static dyn ProxyBackend {
    match provider_id {
        "litellm" => &LITELLM,
        _ => &CLAUDE,
    }
}
