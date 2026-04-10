use crate::config::save_config;
use crate::panel_chrome::sync_panel_chrome_from_window;
use crate::state::{AppConfig, AppState, ProxyActivityEntry, ProxyStatus, UsageData};
use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fs;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

fn normalize_hub_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

struct LitellmHttpCtx {
    base: String,
    key: String,
    bearer: String,
    client: reqwest::Client,
}

async fn litellm_http_ctx(state: &Arc<AppState>) -> Result<LitellmHttpCtx, String> {
    let config = state.config.read().await;
    let endpoint = config.litellm.litellm_endpoint.clone();
    let key = config.litellm.litellm_api_key.clone();
    drop(config);
    if endpoint.is_empty() || key.is_empty() {
        return Err("Hub endpoint or API key not configured".into());
    }
    Ok(LitellmHttpCtx {
        base: normalize_hub_base_url(&endpoint),
        bearer: to_bearer(&key),
        key,
        client: state.http_client.clone(),
    })
}

fn to_bearer(token: &str) -> String {
    let t = token.trim();
    if t.to_lowercase().starts_with("bearer ") {
        t.to_string()
    } else {
        format!("Bearer {t}")
    }
}

/// Max budget from `/budget/info` or LiteLLM-shaped JSON (`max_budget` or `budget.max_budget`).
fn json_max_budget(v: &Value) -> f64 {
    v.get("max_budget")
        .and_then(|x| x.as_f64())
        .or_else(|| {
            v.get("budget")
                .and_then(|b| b.get("max_budget"))
                .and_then(|x| x.as_f64())
        })
        .unwrap_or(0.0)
}

/// LiteLLM `GET /user/info?user_id=` → normalized budget payload for the UI.
fn litellm_user_info_to_budget_json(v: &Value) -> Value {
    let mut spend = v
        .pointer("/user_info/spend")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0);
    if spend <= 0.0 {
        if let Some(keys) = v.get("keys").and_then(|k| k.as_array()) {
            spend = keys.iter().fold(0.0_f64, |acc, k| {
                acc + k.get("spend").and_then(|x| x.as_f64()).unwrap_or(0.0)
            });
        }
    }

    let mut max_budget = v
        .pointer("/user_info/max_budget")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0);
    let mut reset_at: Option<String> = None;
    if let Some(keys) = v.get("keys").and_then(|k| k.as_array()) {
        for key in keys {
            if let Some(mb) = key.get("max_budget").and_then(|x| x.as_f64()) {
                max_budget = max_budget.max(mb);
            }
            if reset_at.is_none() {
                reset_at = key
                    .get("budget_reset_at")
                    .and_then(|x| x.as_str())
                    .map(String::from);
            }
        }
    }

    let mut m = Map::new();
    m.insert("source".into(), Value::String("litellm_user_info".into()));
    m.insert("max_budget".into(), json!(max_budget));
    m.insert("spend".into(), json!(spend));
    if let Some(r) = reset_at {
        m.insert("budget_reset_at".into(), Value::String(r));
    }
    Value::Object(m)
}

fn merge_budget_with_user_max(mut partial: Value, user_budget: &Value) -> Value {
    let max = json_max_budget(user_budget);
    if max > 0.0 {
        if let Some(obj) = partial.as_object_mut() {
            obj.insert("max_budget".into(), json!(max));
            if let Some(ra) = user_budget.get("budget_reset_at") {
                if !ra.is_null() {
                    obj.insert("budget_reset_at".into(), ra.clone());
                }
            }
            let prev = obj
                .get("source")
                .and_then(|s| s.as_str())
                .unwrap_or("budget_info");
            obj.insert(
                "source".into(),
                Value::String(format!("{prev}+litellm_user_info")),
            );
        }
    }
    partial
}

async fn infer_litellm_user_id_from_token(
    client: &reqwest::Client,
    base: &str,
    api_key: &str,
    bearer: &str,
) -> Option<String> {
    // Try common LiteLLM variants. Depending on deployment, one may be enabled.
    let endpoints = [format!("{}/key/info", base), format!("{}/v1/key/info", base)];
    for url in endpoints {
        // A) infer from auth key directly
        if let Ok(resp) = client
            .get(&url)
            .header("x-api-key", api_key)
            .header("authorization", bearer)
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(v) = resp.json::<Value>().await {
                    if let Some(uid) = v.get("user_id").and_then(|x| x.as_str()) {
                        let uid = uid.trim();
                        if !uid.is_empty() {
                            return Some(uid.to_string());
                        }
                    }
                    if let Some(uid) = v.pointer("/info/user_id").and_then(|x| x.as_str()) {
                        let uid = uid.trim();
                        if !uid.is_empty() {
                            return Some(uid.to_string());
                        }
                    }
                }
            }
        }

        // B) explicit key query variant (some deployments require it)
        if let Ok(resp) = client
            .get(&url)
            .query(&[("key", api_key)])
            .header("x-api-key", api_key)
            .header("authorization", bearer)
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(v) = resp.json::<Value>().await {
                    if let Some(uid) = v.get("user_id").and_then(|x| x.as_str()) {
                        let uid = uid.trim();
                        if !uid.is_empty() {
                            return Some(uid.to_string());
                        }
                    }
                    if let Some(uid) = v.pointer("/info/user_id").and_then(|x| x.as_str()) {
                        let uid = uid.trim();
                        if !uid.is_empty() {
                            return Some(uid.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

async fn fetch_litellm_user_info_budget(
    client: &reqwest::Client,
    base: &str,
    api_key: &str,
    bearer: &str,
) -> Option<Value> {
    let uid = infer_litellm_user_id_from_token(client, base, api_key, bearer).await?;
    let url = format!("{}/user/info", base);
    let resp = client
        .get(&url)
        .query(&[("user_id", uid.as_str())])
        .header("x-api-key", api_key)
        .header("authorization", bearer)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    Some(litellm_user_info_to_budget_json(&v))
}

#[tauri::command]
pub async fn get_config(state: State<'_, Arc<AppState>>) -> Result<AppConfig, String> {
    Ok(state.config.read().await.clone())
}

#[tauri::command]
pub async fn save_settings(
    state: State<'_, Arc<AppState>>,
    config: AppConfig,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let mut current = state.config.write().await;
    let provider_changed = current.provider != config.provider;
    // Preserve port - not editable from UI
    current.provider = config.provider.clone();
    current.litellm.litellm_endpoint = config.litellm.litellm_endpoint.trim().trim_end_matches('/').to_string();
    current.litellm.litellm_api_key = config.litellm.litellm_api_key.trim().to_string();
    current.litellm.litellm_display_name = config.litellm.litellm_display_name.trim().to_string();
    current.model_overrides = config.model_overrides;
    let cfg_to_save = current.clone();
    drop(current);

    save_config(&cfg_to_save);
    if provider_changed {
        let _ = app.emit("provider-changed", &config.provider);
    }
    Ok(())
}

#[tauri::command]
pub async fn set_provider(
    state: State<'_, Arc<AppState>>,
    provider: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let mut config = state.config.read().await.clone();
    config.provider = provider.clone();
    save_config(&config);
    *state.config.write().await = config;
    let _ = app.emit("provider-changed", &provider);
    Ok(())
}

#[tauri::command]
pub async fn get_proxy_status(state: State<'_, Arc<AppState>>) -> Result<ProxyStatus, String> {
    Ok(state.proxy_status.read().await.clone())
}

pub(crate) async fn stop_proxy_inner(state: &Arc<AppState>) -> Result<(), String> {
    state.shutdown_tx.send(true).map_err(|e| e.to_string())?;
    let mut guard = state.proxy_task.lock().await;
    if let Some(h) = guard.take() {
        let _ = h.await;
    }
    Ok(())
}

pub(crate) async fn start_proxy_inner(state: &Arc<AppState>, app: AppHandle) -> Result<(), String> {
    {
        let mut guard = state.proxy_task.lock().await;
        if let Some(h) = guard.as_ref() {
            if !h.inner().is_finished() {
                return Ok(());
            }
            if let Some(j) = guard.take() {
                let _ = j.await;
            }
        }
    }
    state.shutdown_tx.send(false).map_err(|e| e.to_string())?;
    let shutdown_rx = state.shutdown_rx.clone();
    let st = state.clone();
    let app2 = app.clone();
    let h = tauri::async_runtime::spawn(async move {
        crate::proxy::server::run_proxy(st, shutdown_rx, app2).await;
    });
    *state.proxy_task.lock().await = Some(h);
    Ok(())
}

#[tauri::command]
pub async fn stop_proxy_server(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    stop_proxy_inner(&state).await
}

#[tauri::command]
pub async fn start_proxy_server(
    state: State<'_, Arc<AppState>>,
    app: AppHandle,
) -> Result<(), String> {
    start_proxy_inner(&state, app).await
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let mut child = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(text.as_bytes())
            .map_err(|e| e.to_string())?;
        child.wait().map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(mut child) = std::process::Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            let _ = child.stdin.as_mut().unwrap().write_all(text.as_bytes());
            let _ = child.wait();
            return Ok(());
        }
        let mut child = std::process::Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(text.as_bytes())
            .map_err(|e| e.to_string())?;
        child.wait().map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        let mut ps = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &format!("Set-Clipboard -Value @'\n{text}\n'@")])
            .spawn()
            .map_err(|e| e.to_string())?;
        ps.wait().map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[allow(unreachable_code)]
    Err("Clipboard: unsupported OS".into())
}

pub(crate) async fn copy_proxy_endpoint_inner(state: &Arc<AppState>) -> Result<(), String> {
    let port = state.config.read().await.listen.port;
    let line = format!("export ANTHROPIC_BASE_URL=http://127.0.0.1:{port}");
    copy_to_clipboard(&line)
}

#[tauri::command]
pub async fn copy_proxy_endpoint(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    copy_proxy_endpoint_inner(&state).await
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ClaudeInstallState {
    previous_base_url: Option<String>,
    backup_file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClaudeInstallStatus {
    installed: bool,
    settings_path: String,
    current_base_url: Option<String>,
}

fn home_dir() -> PathBuf {
    crate::platform::home_dir()
}

fn cc_proxy_dir() -> PathBuf {
    home_dir().join(".config").join("claude-proxy")
}

fn claude_settings_path() -> PathBuf {
    home_dir().join(".claude").join("settings.json")
}

fn install_state_path() -> PathBuf {
    cc_proxy_dir().join("backups").join("claude-install-state.json")
}

fn legacy_install_state_path() -> PathBuf {
    cc_proxy_dir().join("claude-install-state.json")
}

fn load_settings_json(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_str::<Value>(&raw).map_err(|e| format!("Invalid JSON in {}: {e}", path.display()))
}

fn write_settings_json(path: &Path, val: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(val).map_err(|e| e.to_string())?;
    fs::write(path, body).map_err(|e| e.to_string())
}

fn read_install_state() -> Option<ClaudeInstallState> {
    let path = install_state_path();
    if path.exists() {
        let raw = fs::read_to_string(path).ok()?;
        return serde_json::from_str::<ClaudeInstallState>(&raw).ok();
    }

    // Backward compatibility: read old location and migrate.
    let legacy = legacy_install_state_path();
    if !legacy.exists() {
        return None;
    }
    let raw = fs::read_to_string(&legacy).ok()?;
    let state = serde_json::from_str::<ClaudeInstallState>(&raw).ok()?;
    let _ = write_install_state(&state);
    let _ = fs::remove_file(legacy);
    Some(state)
}

fn write_install_state(state: &ClaudeInstallState) -> Result<(), String> {
    let path = install_state_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    fs::write(path, body).map_err(|e| e.to_string())
}

fn clear_install_state() {
    let _ = fs::remove_file(install_state_path());
    let _ = fs::remove_file(legacy_install_state_path());
}

fn current_base_url(settings: &Value) -> Option<String> {
    settings
        .get("env")
        .and_then(|v| v.get("ANTHROPIC_BASE_URL"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
}

fn backup_settings(settings_path: &Path) -> Result<Option<String>, String> {
    const MAX_BACKUPS: usize = 3;
    if !settings_path.exists() {
        return Ok(None);
    }
    let backup_dir = cc_proxy_dir().join("backups");
    fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;
    let stamp = Local::now().format("%Y%m%d-%H%M%S");
    let backup_path = backup_dir.join(format!("claude-settings-{stamp}.json.bak"));
    fs::copy(settings_path, &backup_path).map_err(|e| e.to_string())?;

    // Keep only N newest backup files.
    if let Ok(entries) = fs::read_dir(&backup_dir) {
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .filter_map(|p| {
                fs::metadata(&p)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(|ts| (ts, p))
            })
            .collect();
        files.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, old_path) in files.into_iter().skip(MAX_BACKUPS) {
            let _ = fs::remove_file(old_path);
        }
    }

    Ok(Some(backup_path.display().to_string()))
}

#[tauri::command]
pub async fn get_claude_install_status(
    state: State<'_, Arc<AppState>>,
) -> Result<ClaudeInstallStatus, String> {
    let settings_path = claude_settings_path();
    let settings = load_settings_json(&settings_path)?;
    let current = current_base_url(&settings);
    let port = state.config.read().await.listen.port;
    let expected = format!("http://127.0.0.1:{port}");
    Ok(ClaudeInstallStatus {
        installed: current.as_deref() == Some(expected.as_str()),
        settings_path: settings_path.display().to_string(),
        current_base_url: current,
    })
}

#[tauri::command]
pub async fn install_claude_proxy_settings(
    state: State<'_, Arc<AppState>>,
) -> Result<ClaudeInstallStatus, String> {
    let settings_path = claude_settings_path();
    let mut settings = load_settings_json(&settings_path)?;
    if !settings.is_object() {
        settings = Value::Object(Map::new());
    }

    let previous = current_base_url(&settings);
    let backup_file = backup_settings(&settings_path)?;

    let port = state.config.read().await.listen.port;
    let target = format!("http://127.0.0.1:{port}");

    let root = settings.as_object_mut().unwrap();
    let env_val = root
        .entry("env".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !env_val.is_object() {
        *env_val = Value::Object(Map::new());
    }
    env_val
        .as_object_mut()
        .unwrap()
        .insert("ANTHROPIC_BASE_URL".into(), Value::String(target.clone()));

    write_settings_json(&settings_path, &settings)?;
    write_install_state(&ClaudeInstallState {
        previous_base_url: previous,
        backup_file,
    })?;

    Ok(ClaudeInstallStatus {
        installed: true,
        settings_path: settings_path.display().to_string(),
        current_base_url: Some(target),
    })
}

#[tauri::command]
pub async fn uninstall_claude_proxy_settings(
    state: State<'_, Arc<AppState>>,
) -> Result<ClaudeInstallStatus, String> {
    let settings_path = claude_settings_path();
    let mut settings = load_settings_json(&settings_path)?;
    if !settings.is_object() {
        settings = Value::Object(Map::new());
    }

    let old = read_install_state();
    let previous = old.and_then(|s| s.previous_base_url);

    let root = settings.as_object_mut().unwrap();
    let env_val = root
        .entry("env".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !env_val.is_object() {
        *env_val = Value::Object(Map::new());
    }
    let env = env_val.as_object_mut().unwrap();
    match previous {
        Some(prev) => {
            env.insert("ANTHROPIC_BASE_URL".into(), Value::String(prev));
        }
        None => {
            env.remove("ANTHROPIC_BASE_URL");
        }
    }
    if env.is_empty() {
        root.remove("env");
    }

    write_settings_json(&settings_path, &settings)?;
    clear_install_state();

    let current = current_base_url(&settings);
    let port = state.config.read().await.listen.port;
    let expected = format!("http://127.0.0.1:{port}");
    Ok(ClaudeInstallStatus {
        installed: current.as_deref() == Some(expected.as_str()),
        settings_path: settings_path.display().to_string(),
        current_base_url: current,
    })
}

/// Otwiera okno ustawień (zakładka Settings po stronie frontu).
#[tauri::command]
pub async fn open_settings(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.center();
        let _ = win.show();
        let _ = win.set_focus();
    }
    sync_panel_chrome_from_window(&app);
    let _ = app.emit("open-settings", ());
    Ok(())
}

#[tauri::command]
pub async fn get_usage(state: State<'_, Arc<AppState>>) -> Result<UsageData, String> {
    Ok(state.usage.read().await.clone())
}

#[tauri::command]
pub async fn reset_usage(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    *state.usage.write().await = UsageData::default();
    state.notify_usage_and_persist().await;
    Ok(())
}

#[tauri::command]
pub async fn get_proxy_activity(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ProxyActivityEntry>, String> {
    Ok(state.activity_log.read().await.iter().rev().cloned().collect())
}

/// Aplikacje z Docka / skrótu mają często okrojony `PATH` (bez `~/.local/bin`, Homebrew).
/// Szukamy `claude` tak jak w interaktywnym shellu.
fn resolve_claude_executable() -> PathBuf {
    let path_sep = if cfg!(windows) { ';' } else { ':' };
    let binary_name = if cfg!(windows) { "claude.exe" } else { "claude" };
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(path_sep) {
            if dir.is_empty() {
                continue;
            }
            let candidate = Path::new(dir).join(binary_name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    let home = home_dir();
    #[cfg(not(windows))]
    {
        for p in [
            home.join("bin/claude"),
            home.join(".local/bin/claude"),
            home.join(".cargo/bin/claude"),
            PathBuf::from("/opt/homebrew/bin/claude"),
            PathBuf::from("/usr/local/bin/claude"),
        ] {
            if p.is_file() {
                return p;
            }
        }
    }
    #[cfg(windows)]
    {
        for p in [
            home.join(r".local\bin\claude.exe"),
            home.join(r".cargo\bin\claude.exe"),
        ] {
            if p.is_file() {
                return p;
            }
        }
    }
    PathBuf::from(binary_name)
}

fn credential_files_present() -> bool {
    let home = home_dir();
    let mut paths = vec![
        home.join(".claude").join(".credentials.json"),
        home.join(".claude").join("credentials.json"),
        home.join(".config").join("claude").join("credentials.json"),
    ];
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !dir.is_empty() {
            paths.push(PathBuf::from(&dir).join(".credentials.json"));
        }
    }
    paths.iter().any(|p| p.exists())
}

/// Sprawdza czy Claude Code jest zalogowany.
/// Na macOS OAuth jest w Keychain - wiarygodne jest `claude auth status` (JSON).
/// Pliki ~/.claude/.credentials.json dotyczą głównie Linux/Windows (wg dokumentacji Anthropic).
#[tauri::command]
pub async fn check_claude_auth() -> Result<bool, String> {
    let exe = resolve_claude_executable();
    let output = tokio::process::Command::new(&exe)
        .args(["auth", "status"])
        .output()
        .await;

    let Ok(out) = output else {
        return Ok(credential_files_present());
    };

    let stdout = String::from_utf8_lossy(&out.stdout);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        if let Some(logged_in) = v.get("loggedIn").and_then(|x| x.as_bool()) {
            return Ok(logged_in);
        }
    }

    Ok(out.status.success() && credential_files_present())
}

/// Uruchamia oficjalny flow logowania (`claude auth login`).
/// Z GUI na macOS nie ma TTY, więc otwieramy nowe okno Terminal.app z tą komendą.
#[tauri::command]
pub async fn claude_login(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = tokio::task::spawn_blocking(|| {
            std::process::Command::new("osascript")
                .arg("-e")
                .arg(r#"tell application "Terminal" to do script "claude auth login""#)
                .status()
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("osascript: {e}"))?;

        if !status.success() {
            return Err("Nie udało się otworzyć Terminala.".into());
        }
    }
    #[cfg(target_os = "windows")]
    {
        tokio::task::spawn_blocking(|| {
            std::process::Command::new("cmd")
                .args(["/C", "start", "cmd", "/K", "claude auth login"])
                .status()
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("cmd: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        let exe = resolve_claude_executable();
        tokio::process::Command::new(&exe)
            .args(["auth", "login"])
            .spawn()
            .map_err(|e| format!("Nie można uruchomić \"claude auth login\": {e}"))?;
    }

    let _ = app.emit("auth-changed", ());
    Ok(())
}

/// Otwiera URL w domyślnej przeglądarce
#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    crate::platform::open_url(&url)
}

#[tauri::command]
pub async fn fetch_models(state: State<'_, Arc<AppState>>) -> Result<Vec<serde_json::Value>, String> {
    let ctx = litellm_http_ctx(&state).await?;
    let url = format!("{}/v1/models", ctx.base);
    let resp = ctx
        .client
        .get(&url)
        .header("x-api-key", &ctx.key)
        .header("authorization", &ctx.bearer)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
        Ok(data.clone())
    } else {
        Ok(vec![body])
    }
}

#[tauri::command]
pub async fn fetch_budget_info(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let ctx = litellm_http_ctx(&state).await?;
    let LitellmHttpCtx {
        ref base,
        ref key,
        ref bearer,
        ref client,
    } = ctx;

    let mut partial_budget: Option<Value> = None;

    // 1) `/budget/info` (common on LiteLLM deployments)
    let budget_info_url = format!("{}/budget/info", base);
    if let Ok(resp) = client
        .get(&budget_info_url)
        .header("x-api-key", key)
        .header("authorization", bearer)
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                if json_max_budget(&v) > 0.0 {
                    return Ok(v);
                }
                partial_budget = Some(v);
            }
        }
    }

    // 2) LiteLLM GET /user/info (internal user_id → max_budget on keys / user_info)
    if let Some(u) = fetch_litellm_user_info_budget(client, base, key, bearer).await {
        if json_max_budget(&u) > 0.0 {
            return Ok(match partial_budget {
                Some(p) => merge_budget_with_user_max(p, &u),
                None => u,
            });
        }
        if partial_budget.is_none() {
            partial_budget = Some(u);
        }
    }

    if let Some(p) = partial_budget {
        return Ok(p);
    }

    // 3) LiteLLM provider budgets fallback
    let provider_budgets_url = format!("{}/provider/budgets", base);
    if let Ok(resp) = client
        .get(&provider_budgets_url)
        .header("authorization", bearer.as_str())
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                if let Some(providers) = v.get("providers").and_then(|p| p.as_object()) {
                    let mut total_max_budget = 0.0_f64;
                    let mut total_spend = 0.0_f64;
                    let mut reset_at: Option<String> = None;

                    for provider in providers.values() {
                        total_max_budget += provider
                            .get("budget_limit")
                            .and_then(|x| x.as_f64())
                            .unwrap_or(0.0);
                        total_spend += provider
                            .get("spend")
                            .and_then(|x| x.as_f64())
                            .unwrap_or(0.0);
                        if reset_at.is_none() {
                            reset_at = provider
                                .get("budget_reset_at")
                                .and_then(|x| x.as_str())
                                .map(String::from);
                        }
                    }

                    return Ok(serde_json::json!({
                        "source": "provider_budgets",
                        "max_budget": total_max_budget,
                        "spend": total_spend,
                        "budget_reset_at": reset_at,
                        "providers": providers
                    }));
                }
            }
        }
    }

    // 4) LiteLLM daily metrics fallback
    let daily_metrics_url = format!("{}/daily_metrics", base);
    if let Ok(resp) = client
        .get(&daily_metrics_url)
        .header("authorization", bearer.as_str())
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                let mut total_spend = v.get("total_spend").and_then(|x| x.as_f64()).unwrap_or(0.0);
                if total_spend <= 0.0 {
                    if let Some(arr) = v.as_array() {
                        total_spend = arr
                            .iter()
                            .map(|row| row.get("daily_spend").and_then(|x| x.as_f64()).unwrap_or(0.0))
                            .sum();
                    }
                }
                return Ok(serde_json::json!({
                    "source": "daily_metrics",
                    "spend": total_spend
                }));
            }
        }
    }

    Err("Failed to fetch budget info from /budget/info, /user/info, /provider/budgets, and /daily_metrics".into())
}

#[tauri::command]
pub async fn fetch_litellm_daily_activity(
    state: State<'_, Arc<AppState>>,
    start_date: String,
    end_date: String,
) -> Result<serde_json::Value, String> {
    let ctx = litellm_http_ctx(&state).await?;
    let url = format!(
        "{}/user/daily/activity?start_date={}&end_date={}",
        ctx.base, start_date, end_date
    );
    let resp = ctx
        .client
        .get(&url)
        .header("x-api-key", &ctx.key)
        .header("authorization", &ctx.bearer)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = resp.status();
    let body = resp.bytes().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let msg = String::from_utf8_lossy(&body);
        return Err(format!(
            "Hub daily activity HTTP {}: {}",
            status.as_u16(),
            msg.chars().take(500).collect::<String>()
        ));
    }
    serde_json::from_slice(&body).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct ClaudeUsageLimits {
    pub has_auth: bool,
    /// 5-hour window utilization (0.0–1.0)
    pub five_hour_utilization: Option<f64>,
    pub five_hour_resets_at: Option<String>,
    /// 7-day window utilization (0.0–1.0)
    pub seven_day_utilization: Option<f64>,
    pub seven_day_resets_at: Option<String>,
}

/// Fetches plan usage limits from `GET https://api.anthropic.com/api/oauth/usage`.
/// Uses the OAuth bearer token captured from requests passing through the proxy.
#[tauri::command]
pub async fn fetch_claude_rate_limits(
    state: State<'_, Arc<AppState>>,
) -> Result<ClaudeUsageLimits, String> {
    let auth = state.claude_auth.read().await.clone();
    let bearer = auth.bearer.as_deref().unwrap_or_default();
    if bearer.is_empty() {
        return Ok(ClaudeUsageLimits {
            has_auth: false,
            five_hour_utilization: None,
            five_hour_resets_at: None,
            seven_day_utilization: None,
            seven_day_resets_at: None,
        });
    }

    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("authorization", bearer)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("user-agent", "claude-proxy/1.0")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = resp.status();
    let body = resp.bytes().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("oauth/usage returned {}", status.as_u16()));
    }

    let json: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| e.to_string())?;

    let five_hour = json.get("five_hour");
    let seven_day = json.get("seven_day");

    Ok(ClaudeUsageLimits {
        has_auth: true,
        five_hour_utilization: five_hour
            .and_then(|v| v.get("utilization"))
            .and_then(|v| v.as_f64()),
        five_hour_resets_at: five_hour
            .and_then(|v| v.get("resets_at"))
            .and_then(|v| v.as_str())
            .map(String::from),
        seven_day_utilization: seven_day
            .and_then(|v| v.get("utilization"))
            .and_then(|v| v.as_f64()),
        seven_day_resets_at: seven_day
            .and_then(|v| v.get("resets_at"))
            .and_then(|v| v.as_str())
            .map(String::from),
    })
}
