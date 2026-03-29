use crate::proxy::backends::{backend_for, extract_auth, ProxyBackend, ResolvedAuth};
use crate::proxy::bridge;
use crate::proxy::error::ProxyError;
use crate::proxy::logging::{ApiLogger, mask_secret};
use crate::proxy::stream::{BridgeTransformStream, ExtractedUsage, UsageCapturingStream};
use crate::proxy::ProxyBody;
use crate::state::{AppState, ProxyActivityEntry};
use tauri::Emitter;
use bytes::Bytes;
use http_body::Frame;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, StreamBody};
use hyper::{body::Incoming, Request, Response, StatusCode};
use std::sync::Arc;
use std::time::Instant;

// ---------------------------------------------------------------------------
// RequestContext – carries everything built during the "prepare" phase
// ---------------------------------------------------------------------------

struct RequestContext {
    method: hyper::Method,
    uri: hyper::Uri,
    headers: hyper::HeaderMap,
    body: Vec<u8>,
    provider_name: String,
    backend: &'static dyn ProxyBackend,
    auth: ResolvedAuth,
    effective_model: Option<String>,
    has_vision: bool,
    bridge: Option<bridge::BridgeRequest>,
    upstream_url: String,
    path_and_query: String,
    is_messages: bool,
    log: ApiLogger,
    started_at: Instant,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn handle_request(
    req: Request<Incoming>,
    state: Arc<AppState>,
) -> Result<Response<ProxyBody>, hyper::Error> {
    match handle_inner(req, state).await {
        Ok(resp) => Ok(resp),
        Err(e) => Ok(e.into_response()),
    }
}

// ---------------------------------------------------------------------------
// Pipeline: build_context → auth → send_upstream → process_response
// ---------------------------------------------------------------------------

async fn handle_inner(
    req: Request<Incoming>,
    state: Arc<AppState>,
) -> Result<Response<ProxyBody>, ProxyError> {
    let ctx = build_context(req, &state).await?;

    if ctx.backend.should_capture_auth() {
        store_claude_auth(&state, &ctx.auth).await;
    }
    if let Some(resp) = ctx.backend.unauthorized_if_missing_auth(&ctx.auth) {
        return Ok(resp);
    }

    log_request(&ctx);

    let upstream_resp = send_upstream(&ctx, &state.http_client).await?;
    process_response(ctx, upstream_resp, state).await
}

async fn build_context(
    req: Request<Incoming>,
    state: &AppState,
) -> Result<RequestContext, ProxyError> {
    let started_at = Instant::now();
    let log = ApiLogger::default();
    let (parts, body) = req.into_parts();

    let config = state.config.read().await;
    let provider_name = config.provider.clone();
    let litellm_endpoint = config.litellm.litellm_endpoint.clone();
    let litellm_key = config.litellm.litellm_api_key.clone();
    let overrides = config.model_overrides.clone();
    drop(config);

    let backend = backend_for(&provider_name);
    let incoming_auth = extract_auth(&parts.headers);

    let body_bytes = body.collect().await?.to_bytes().to_vec();
    let prepared = backend.prepare_body(body_bytes, &overrides);
    let mut body_vec = prepared.body;
    let effective_model = prepared.effective_model;
    let has_vision = prepared.has_vision_input;

    let auth = backend.resolve_auth(&litellm_key, &incoming_auth);

    let target = backend.upstream_target(&litellm_endpoint);
    let incoming_path = parts.uri.path().to_string();
    let mut path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| target.default_path.clone());

    let mut bridge_ctx: Option<bridge::BridgeRequest> = None;
    if backend.should_apply_chat_completions_bridge(&incoming_path, effective_model.as_deref()) {
        if let Some(br) = bridge::rewrite_request(&body_vec) {
            log.line(&format!(
                "bridge rewrite model={} stream={} {} → {}",
                br.model, br.stream, incoming_path, br.path
            ));
            path_and_query = br.path.clone();
            body_vec = br.body.clone();
            bridge_ctx = Some(br);
        }
    }

    let upstream_url = format!("{}{}", target.base_url, path_and_query);
    let is_messages = parts.method == hyper::Method::POST && parts.uri.path() == "/v1/messages";

    Ok(RequestContext {
        method: parts.method,
        uri: parts.uri,
        headers: parts.headers,
        body: body_vec,
        provider_name,
        backend,
        auth,
        effective_model,
        has_vision,
        bridge: bridge_ctx,
        upstream_url,
        path_and_query,
        is_messages,
        log,
        started_at,
    })
}

fn log_request(ctx: &RequestContext) {
    let model_label = ctx.effective_model.as_deref().unwrap_or("unknown");
    let now = chrono::Utc::now().format("%H:%M:%S");
    println!(
        "[proxy] {now} → {} {} {model_label}",
        ctx.provider_name, ctx.upstream_url
    );
    ctx.log.request_meta(
        &ctx.provider_name,
        ctx.method.as_str(),
        ctx.uri.path(),
        &ctx.upstream_url,
        model_label,
        &ctx.auth
            .bearer
            .as_deref()
            .map(mask_secret)
            .unwrap_or_else(|| "none".into()),
    );
}

async fn send_upstream(
    ctx: &RequestContext,
    client: &reqwest::Client,
) -> Result<reqwest::Response, ProxyError> {
    let req_builder = client.request(ctx.method.clone(), &ctx.upstream_url);
    let req_builder =
        ctx.backend
            .apply_upstream_headers(req_builder, &ctx.headers, &ctx.auth);

    req_builder.body(ctx.body.clone()).send().await.map_err(|err| {
        ctx.log
            .upstream_error(&ctx.provider_name, &ctx.path_and_query, &err.to_string());
        ProxyError::Upstream(StatusCode::BAD_GATEWAY, err.to_string())
    })
}

// ---------------------------------------------------------------------------
// Response processing: bridge-streaming / bridge-buffered / streaming / buffered
// ---------------------------------------------------------------------------

async fn process_response(
    ctx: RequestContext,
    upstream_resp: reqwest::Response,
    state: Arc<AppState>,
) -> Result<Response<ProxyBody>, ProxyError> {
    let status = upstream_resp.status();
    let upstream_headers = upstream_resp.headers().clone();
    let content_type = upstream_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();

    ctx.log
        .upstream_status(status.as_u16(), &ctx.provider_name, &ctx.path_and_query);

    let is_sse = status.is_success() && is_event_stream(&content_type);
    let bridge_wants_stream = ctx
        .bridge
        .as_ref()
        .map(|br| br.stream)
        .unwrap_or(false);
    let has_bridge = ctx.bridge.is_some();

    if has_bridge {
        if bridge_wants_stream && is_sse {
            return Ok(process_bridge_streaming(
                ctx,
                upstream_resp,
                status,
                &upstream_headers,
                state,
            ));
        }
        return process_bridge_buffered(
            ctx,
            upstream_resp,
            status,
            &upstream_headers,
            &content_type,
            state,
        )
        .await;
    }

    if is_sse {
        return Ok(process_streaming(
            ctx,
            upstream_resp,
            status,
            &upstream_headers,
            &content_type,
            state,
        ));
    }

    process_buffered(
        ctx,
        upstream_resp,
        status,
        &upstream_headers,
        content_type,
        state,
    )
    .await
}

/// Bridge + SSE: transform OpenAI SSE → Anthropic SSE in real-time.
fn process_bridge_streaming(
    ctx: RequestContext,
    upstream_resp: reqwest::Response,
    status: reqwest::StatusCode,
    upstream_headers: &reqwest::header::HeaderMap,
    state: Arc<AppState>,
) -> Response<ProxyBody> {
    let model = ctx
        .bridge
        .as_ref()
        .map(|br| br.model.clone())
        .unwrap_or_default();
    ctx.log
        .stream_response(&ctx.provider_name, &ctx.path_and_query);
    ctx.backend
        .log_rate_limit(&ctx.log, status.as_u16(), &ctx.path_and_query);

    let (usage_tx, usage_rx) = tokio::sync::oneshot::channel();
    let bridge_stream =
        BridgeTransformStream::new(upstream_resp.bytes_stream(), model, usage_tx);

    spawn_usage_updater(
        usage_rx,
        state,
        ctx.backend.id(),
        ctx.effective_model.clone(),
        ctx.is_messages,
        Some(activity_ctx_from(&ctx, status.as_u16())),
    );

    let body = UnsyncBoxBody::new(StreamBody::new(bridge_stream));
    build_upstream_response(
        status,
        upstream_headers,
        "text/event-stream; charset=utf-8",
        body,
    )
}

/// Bridge + buffered: full body buffering with optional format transformation.
async fn process_bridge_buffered(
    ctx: RequestContext,
    upstream_resp: reqwest::Response,
    status: reqwest::StatusCode,
    upstream_headers: &reqwest::header::HeaderMap,
    content_type: &str,
    state: Arc<AppState>,
) -> Result<Response<ProxyBody>, ProxyError> {
    let resp_bytes: Bytes = upstream_resp.bytes().await.unwrap_or_default();
    ctx.log.upstream_body_preview(
        &ctx.provider_name,
        status.as_u16(),
        content_type,
        &crate::proxy::logging::truncate(&String::from_utf8_lossy(&resp_bytes), 800),
    );

    let mut st = status;
    let mut ct = content_type.to_string();
    let mut bytes_out = resp_bytes;

    if let Some(normalized) = ctx
        .backend
        .maybe_normalize_error(&ctx.log, &bytes_out, ctx.has_vision)
    {
        bytes_out = normalized.0;
        st = normalized.1;
        ct = "application/json".into();
    }

    if let Some(ref br) = ctx.bridge {
        if st.is_success() {
            if let Some((transformed, c)) =
                bridge::transform_response(&bytes_out, &ct, br.stream, &br.model)
            {
                bytes_out = transformed;
                ct = c;
            }
        }
    }

    ctx.backend
        .log_rate_limit(&ctx.log, st.as_u16(), &ctx.path_and_query);
    update_buffered_usage(&ctx, &state, &bytes_out).await;

    let duration_ms = ctx.started_at.elapsed().as_millis() as u64;
    let (input_tokens, output_tokens) = extract_usage_tokens_from_json_bytes(&bytes_out);
    let err = upstream_error_summary(st.as_u16(), &bytes_out);
    schedule_proxy_activity(
        &state,
        &ctx,
        st.as_u16(),
        duration_ms,
        input_tokens,
        output_tokens,
        err,
    );

    Ok(build_upstream_response(
        st,
        upstream_headers,
        &ct,
        body_full(bytes_out),
    ))
}

/// Native SSE streaming with usage extraction.
fn process_streaming(
    ctx: RequestContext,
    upstream_resp: reqwest::Response,
    status: reqwest::StatusCode,
    upstream_headers: &reqwest::header::HeaderMap,
    content_type: &str,
    state: Arc<AppState>,
) -> Response<ProxyBody> {
    ctx.log
        .stream_response(&ctx.provider_name, &ctx.path_and_query);
    ctx.backend
        .log_rate_limit(&ctx.log, status.as_u16(), &ctx.path_and_query);

    let (usage_tx, usage_rx) = tokio::sync::oneshot::channel();
    let capturing_stream = UsageCapturingStream::new(upstream_resp.bytes_stream(), usage_tx);

    spawn_usage_updater(
        usage_rx,
        state,
        ctx.backend.id(),
        ctx.effective_model.clone(),
        ctx.is_messages,
        Some(activity_ctx_from(&ctx, status.as_u16())),
    );

    let body = UnsyncBoxBody::new(StreamBody::new(capturing_stream));
    build_upstream_response(status, upstream_headers, content_type, body)
}

/// Non-streaming buffered response.
async fn process_buffered(
    ctx: RequestContext,
    upstream_resp: reqwest::Response,
    status: reqwest::StatusCode,
    upstream_headers: &reqwest::header::HeaderMap,
    content_type: String,
    state: Arc<AppState>,
) -> Result<Response<ProxyBody>, ProxyError> {
    let mut resp_bytes: Bytes = upstream_resp.bytes().await.unwrap_or_default();
    ctx.log.upstream_body_preview(
        &ctx.provider_name,
        status.as_u16(),
        &content_type,
        &crate::proxy::logging::truncate(&String::from_utf8_lossy(&resp_bytes), 800),
    );

    let mut st = status;
    let mut ct = content_type;
    if let Some(normalized) = ctx
        .backend
        .maybe_normalize_error(&ctx.log, &resp_bytes, ctx.has_vision)
    {
        resp_bytes = normalized.0;
        st = normalized.1;
        ct = "application/json".into();
    }

    ctx.backend
        .log_rate_limit(&ctx.log, st.as_u16(), &ctx.path_and_query);
    update_buffered_usage(&ctx, &state, &resp_bytes).await;

    let duration_ms = ctx.started_at.elapsed().as_millis() as u64;
    let (input_tokens, output_tokens) = extract_usage_tokens_from_json_bytes(&resp_bytes);
    let err = upstream_error_summary(st.as_u16(), &resp_bytes);
    schedule_proxy_activity(
        &state,
        &ctx,
        st.as_u16(),
        duration_ms,
        input_tokens,
        output_tokens,
        err,
    );

    Ok(build_upstream_response(
        st,
        upstream_headers,
        &ct,
        body_full(resp_bytes),
    ))
}

// ---------------------------------------------------------------------------
// Usage helpers
// ---------------------------------------------------------------------------

/// Gdy upstream zwrócił gzip mimo braku dekompresji po stronie klienta — spróbuj rozpakować (nagłówek gzip).
fn try_gunzip_error_body(body: &[u8]) -> Option<Vec<u8>> {
    if body.len() < 12 || body[0] != 0x1f || body[1] != 0x8b {
        return None;
    }
    use flate2::read::GzDecoder;
    use std::io::Read;
    let mut dec = GzDecoder::new(body);
    let mut out = Vec::new();
    dec.read_to_end(&mut out).ok()?;
    (!out.is_empty()).then_some(out)
}

/// Zbyt dużo znaków zastępczych UTF-8 ⇒ traktuj jako binaria / śmieci, nie pokazuj w logu.
fn utf8_lossy_is_garbled(s: &str) -> bool {
    let rep = s.chars().filter(|&c| c == '\u{FFFD}').count();
    let len = s.chars().count().max(1);
    rep > 3 && rep * 8 > len
}

/// Wyciąga liczby tokenów z JSON odpowiedzi (Anthropic / OpenAI / LiteLLM).
/// Krótki opis przyczyny dla odpowiedzi spoza zakresu 2xx (JSON `error.message` / `message` albo skrót body).
fn upstream_error_summary(status: u16, body: &[u8]) -> Option<String> {
    if (200..300).contains(&status) {
        return None;
    }
    if body.is_empty() {
        return Some(format!("HTTP {status}"));
    }
    let owned = try_gunzip_error_body(body);
    let body_ref: &[u8] = owned.as_deref().unwrap_or(body);

    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(body_ref) {
        let msg = v
            .pointer("/error/message")
            .and_then(|x| x.as_str())
            .or_else(|| v.get("message").and_then(|x| x.as_str()))
            .or_else(|| {
                v.get("error").and_then(|e| {
                    if e.is_string() {
                        e.as_str()
                    } else {
                        e.get("message").and_then(|m| m.as_str())
                    }
                })
            })
            .map(|s| s.trim());
        if let Some(m) = msg.filter(|s| !s.is_empty()) {
            return Some(truncate_activity_error_line(m, 320));
        }
    }
    let s = String::from_utf8_lossy(body_ref);
    if utf8_lossy_is_garbled(&s) {
        return Some(format!("HTTP {status} (compressed or non-text body)"));
    }
    let one: String = s
        .chars()
        .map(|c| if c.is_control() && c != ' ' { ' ' } else { c })
        .collect();
    let one = one.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.is_empty() {
        return Some(format!("HTTP {status}"));
    }
    Some(truncate_activity_error_line(&one, 320))
}

fn truncate_activity_error_line(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let t: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

fn extract_usage_tokens_from_json_bytes(resp_bytes: &[u8]) -> (u64, u64) {
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(resp_bytes) else {
        return (0, 0);
    };
    let input = json
        .pointer("/usage/input_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| json.pointer("/usage/prompt_tokens").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    let output = json
        .pointer("/usage/output_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| json.pointer("/usage/completion_tokens").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    (input, output)
}

struct ActivityEmitCtx {
    started_at: Instant,
    provider: String,
    method: String,
    path: String,
    status: u16,
    model: Option<String>,
}

fn activity_ctx_from(ctx: &RequestContext, status: u16) -> ActivityEmitCtx {
    ActivityEmitCtx {
        started_at: ctx.started_at,
        provider: ctx.provider_name.clone(),
        method: ctx.method.to_string(),
        path: ctx.uri.path().to_string(),
        status,
        model: ctx.effective_model.clone(),
    }
}

fn spawn_usage_updater(
    usage_rx: tokio::sync::oneshot::Receiver<ExtractedUsage>,
    state: Arc<AppState>,
    backend_id: &str,
    effective_model: Option<String>,
    is_messages: bool,
    activity: Option<ActivityEmitCtx>,
) {
    let backend_id = backend_id.to_string();
    tokio::spawn(async move {
        let extracted = match usage_rx.await {
            Ok(u) => u,
            Err(_) => {
                if let Some(a) = activity {
                    let duration_ms = a.started_at.elapsed().as_millis() as u64;
                    schedule_proxy_activity_raw(
                        &state,
                        a.provider,
                        a.method,
                        a.path,
                        a.status,
                        a.model,
                        duration_ms,
                        0,
                        0,
                        None,
                    );
                }
                return;
            }
        };

        if let Some(a) = activity {
            let duration_ms = a.started_at.elapsed().as_millis() as u64;
            schedule_proxy_activity_raw(
                &state,
                a.provider,
                a.method,
                a.path,
                a.status,
                a.model,
                duration_ms,
                extracted.input_tokens,
                extracted.output_tokens,
                None,
            );
        }

        if !is_messages && extracted.input_tokens == 0 && extracted.output_tokens == 0 {
            return;
        }
        {
            let mut u = state.usage.write().await;
            let pu = u.by_provider.entry(backend_id).or_default();
            pu.input_tokens += extracted.input_tokens;
            pu.output_tokens += extracted.output_tokens;
            if is_messages {
                pu.requests += 1;
            }
            let key = effective_model.unwrap_or_else(|| "unknown".into());
            let m = pu.per_model.entry(key).or_default();
            m.input_tokens += extracted.input_tokens;
            m.output_tokens += extracted.output_tokens;
            if is_messages {
                m.requests += 1;
            }
        }
        state.notify_usage_and_persist().await;
    });
}

async fn update_buffered_usage(ctx: &RequestContext, state: &AppState, resp_bytes: &[u8]) {
    {
        let mut u = state.usage.write().await;
        ctx.backend.apply_usage_from_buffered_response(
            &mut u,
            ctx.is_messages,
            &ctx.effective_model,
            resp_bytes,
        );
    }
    state.notify_usage_and_persist().await;
}

fn schedule_proxy_activity(
    state: &Arc<AppState>,
    ctx: &RequestContext,
    status: u16,
    duration_ms: u64,
    input_tokens: u64,
    output_tokens: u64,
    error_detail: Option<String>,
) {
    schedule_proxy_activity_raw(
        state,
        ctx.provider_name.clone(),
        ctx.method.to_string(),
        ctx.uri.path().to_string(),
        status,
        ctx.effective_model.clone(),
        duration_ms,
        input_tokens,
        output_tokens,
        error_detail,
    );
}

fn schedule_proxy_activity_raw(
    state: &Arc<AppState>,
    provider: String,
    method: String,
    path: String,
    status: u16,
    model: Option<String>,
    duration_ms: u64,
    input_tokens: u64,
    output_tokens: u64,
    error_detail: Option<String>,
) {
    let state = state.clone();
    tokio::spawn(async move {
        let entry = ProxyActivityEntry {
            ts_ms: chrono::Utc::now().timestamp_millis(),
            provider,
            method,
            path,
            status,
            model,
            duration_ms,
            input_tokens,
            output_tokens,
            error_detail,
        };
        {
            let mut log = state.activity_log.write().await;
            while log.len() >= 100 {
                log.pop_front();
            }
            log.push_back(entry.clone());
        }
        if let Some(app) = state.app_handle.read().await.as_ref() {
            let _ = app.emit("proxy-activity", &entry);
        }
    });
}

async fn store_claude_auth(state: &AppState, auth: &ResolvedAuth) {
    let has_key = auth.key.as_ref().map_or(false, |k| !k.is_empty());
    let has_bearer = auth.bearer.as_ref().map_or(false, |b| !b.is_empty());
    if has_key || has_bearer {
        let mut stored = state.claude_auth.write().await;
        if has_key {
            stored.api_key = auth.key.clone();
        }
        if has_bearer {
            stored.bearer = auth.bearer.clone();
        }
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn body_full(bytes: Bytes) -> ProxyBody {
    let s =
        futures_util::stream::iter(std::iter::once(Ok::<_, std::io::Error>(Frame::data(bytes))));
    UnsyncBoxBody::new(StreamBody::new(s))
}

fn is_event_stream(content_type: &str) -> bool {
    let ct = content_type.to_lowercase();
    ct.contains("text/event-stream") || ct.contains("event-stream")
}

fn build_upstream_response(
    status: reqwest::StatusCode,
    upstream_headers: &reqwest::header::HeaderMap,
    content_type: &str,
    body: ProxyBody,
) -> Response<ProxyBody> {
    let mut builder = Response::builder()
        .status(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK));
    for (name, value) in upstream_headers.iter() {
        let n = name.as_str();
        if n.eq_ignore_ascii_case("content-length")
            || n.eq_ignore_ascii_case("transfer-encoding")
            || n.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder
        .header("content-type", content_type)
        .body(body)
        .unwrap()
}
