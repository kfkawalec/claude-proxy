/// Auth extracted from the incoming client request (Claude Code).
#[derive(Debug, Clone, Default)]
pub struct IncomingAuth {
    pub api_key: Option<String>,
    pub bearer: Option<String>,
}

pub fn extract_auth(headers: &hyper::HeaderMap) -> IncomingAuth {
    IncomingAuth {
        api_key: headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .map(String::from),
        bearer: headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(String::from),
    }
}

/// Auth sent upstream to Anthropic or LiteLLM.
#[derive(Debug, Clone, Default)]
pub struct ResolvedAuth {
    pub key: Option<String>,
    pub bearer: Option<String>,
}
