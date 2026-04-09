use bytes::Bytes;
use http_body::Frame;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::StreamBody;
use hyper::{Response, StatusCode};

use super::ProxyBody;

// ---------------------------------------------------------------------------
// Anthropic-shaped API errors (shared by proxy failures and backend normalization)
// ---------------------------------------------------------------------------

/// JSON body: `{"type":"error","error":{"type":"invalid_request_error","message":...}}`
pub fn anthropic_invalid_request_bytes(message: &str) -> Bytes {
    let body = serde_json::json!({
        "type": "error",
        "error": {
            "type": "invalid_request_error",
            "message": message
        }
    });
    Bytes::from(serde_json::to_vec(&body).expect("anthropic error json"))
}

/// Appended to LiteLLM-normalized messages when vision is involved.
pub const CLIENT_VISION_HINT: &str = " Selected model does not support image input (vision). Choose a vision-capable model.";

/// User-facing LiteLLM / hub copy (English).
pub mod litellm {
    pub const MODEL_NOT_ON_HUB_GENERIC: &str = "The requested model is not supported or is not configured on the LiteLLM hub (routing/fallback failed, e.g. HTTP 404). Pick a valid model or fix provider and fallback mapping in LiteLLM.";

    pub const THINKING_HISTORY_INCOMPATIBLE: &str = "Conversation history includes extended-thinking blocks that this upstream route rejects (invalid thinking/signature shape). Start a new chat or clear context when switching models or proxy routes; if it keeps happening, update the client or LiteLLM.";

    pub fn model_not_on_hub_named(model: &str) -> String {
        format!(
            "Model `{model}` is not supported or is not configured on the LiteLLM hub (routing/fallback failed, e.g. HTTP 404). Pick a model that exists in the hub or fix provider and fallback mapping in LiteLLM."
        )
    }
}

// ---------------------------------------------------------------------------
// Proxy-internal errors (auth, transport, etc.)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ProxyError {
    Auth(StatusCode, String),
    Upstream(StatusCode, String),
    Internal(String),
}

impl std::fmt::Display for ProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth(code, msg) => write!(f, "auth error ({code}): {msg}"),
            Self::Upstream(code, msg) => write!(f, "upstream error ({code}): {msg}"),
            Self::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for ProxyError {}

impl ProxyError {
    pub fn into_response(self) -> Response<ProxyBody> {
        let (status, message) = match &self {
            Self::Auth(code, msg) => (*code, msg.as_str()),
            Self::Upstream(code, msg) => (*code, msg.as_str()),
            Self::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.as_str()),
        };

        let bytes = anthropic_invalid_request_bytes(message);
        let s = futures_util::stream::iter(std::iter::once(Ok::<_, std::io::Error>(
            Frame::data(bytes),
        )));
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(UnsyncBoxBody::new(StreamBody::new(s)))
            .unwrap()
    }
}

impl From<hyper::Error> for ProxyError {
    fn from(err: hyper::Error) -> Self {
        Self::Internal(err.to_string())
    }
}
