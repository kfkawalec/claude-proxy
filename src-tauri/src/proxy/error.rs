use bytes::Bytes;
use http_body::Frame;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::StreamBody;
use hyper::{Response, StatusCode};

use super::ProxyBody;

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

        let body = serde_json::json!({
            "type": "error",
            "error": {"type": "invalid_request_error", "message": message}
        });
        let bytes = Bytes::from(serde_json::to_vec(&body).unwrap());
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
