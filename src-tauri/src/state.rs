use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tauri::async_runtime::JoinHandle;
use tokio::sync::{watch, Mutex, RwLock};

/// Local proxy listen settings (port, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyListen {
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_port() -> u16 {
    3456
}

impl Default for ProxyListen {
    fn default() -> Self {
        Self {
            port: default_port(),
        }
    }
}

/// LiteLLM / hub upstream settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LitellmSettings {
    pub litellm_api_key: String,
    pub litellm_endpoint: String,
    #[serde(default)]
    pub litellm_display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub provider: String,
    #[serde(flatten)]
    pub listen: ProxyListen,
    #[serde(flatten)]
    pub litellm: LitellmSettings,
    #[serde(default)]
    pub model_overrides: HashMap<String, String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            provider: "claude".into(),
            listen: ProxyListen::default(),
            litellm: LitellmSettings::default(),
            model_overrides: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct UsageData {
    pub by_provider: HashMap<String, ProviderUsage>,
}

impl Serialize for UsageData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct UsageDataSer<'a> {
            by_provider: &'a HashMap<String, ProviderUsage>,
        }
        UsageDataSer {
            by_provider: &self.by_provider,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for UsageData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct UsageDataDe {
            #[serde(default)]
            by_provider: HashMap<String, ProviderUsage>,
            #[serde(default)]
            claude: Option<ProviderUsage>,
            #[serde(default)]
            litellm: Option<ProviderUsage>,
        }
        let v = UsageDataDe::deserialize(deserializer)?;
        if !v.by_provider.is_empty() || (v.claude.is_none() && v.litellm.is_none()) {
            return Ok(UsageData {
                by_provider: v.by_provider,
            });
        }
        let mut by_provider = HashMap::new();
        if let Some(c) = v.claude {
            by_provider.insert("claude".into(), c);
        }
        if let Some(l) = v.litellm {
            by_provider.insert("litellm".into(), l);
        }
        Ok(UsageData { by_provider })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub requests: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub requests: u64,
    pub per_model: HashMap<String, ModelUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProxyStatus {
    Running,
    Stopped,
    Error(String),
}

impl Default for ProxyStatus {
    fn default() -> Self {
        Self::Stopped
    }
}

/// Captured auth from Claude Code requests passing through the proxy.
#[derive(Debug, Clone, Default)]
pub struct CapturedClaudeAuth {
    pub api_key: Option<String>,
    pub bearer: Option<String>,
}

/// One row for the live activity strip in Settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyActivityEntry {
    pub ts_ms: i64,
    pub provider: String,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub model: Option<String>,
    /// Czas od rozpoczęcia obsługi żądania do zakończenia odpowiedzi (ms).
    pub duration_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Krótki opis błędu z body odpowiedzi (gdy status poza 2xx).
    #[serde(default)]
    pub error_detail: Option<String>,
}

pub struct AppState {
    pub config: RwLock<AppConfig>,
    pub usage: RwLock<UsageData>,
    pub proxy_status: RwLock<ProxyStatus>,
    pub claude_auth: RwLock<CapturedClaudeAuth>,
    pub shutdown_tx: watch::Sender<bool>,
    pub shutdown_rx: watch::Receiver<bool>,
    pub proxy_task: Mutex<Option<JoinHandle<()>>>,
    pub http_client: reqwest::Client,
    pub app_handle: RwLock<Option<tauri::AppHandle>>,
    pub activity_log: RwLock<VecDeque<ProxyActivityEntry>>,
    pub usage_db_path: Mutex<Option<PathBuf>>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Arc<Self> {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        Arc::new(Self {
            config: RwLock::new(config),
            usage: RwLock::new(UsageData::default()),
            proxy_status: RwLock::new(ProxyStatus::Stopped),
            claude_auth: RwLock::new(CapturedClaudeAuth::default()),
            shutdown_tx,
            shutdown_rx,
            proxy_task: Mutex::new(None),
            http_client: reqwest::Client::builder()
                .no_gzip()
                .no_brotli()
                .no_deflate()
                .build()
                .expect("failed to build http client"),
            app_handle: RwLock::new(None),
            activity_log: RwLock::new(VecDeque::with_capacity(100)),
            usage_db_path: Mutex::new(None),
        })
    }

    /// Emit `usage-updated` and persist usage to SQLite (if path configured).
    pub async fn notify_usage_and_persist(&self) {
        if let Some(app) = self.app_handle.read().await.as_ref() {
            let _ = app.emit("usage-updated", ());
        }
        let path = self.usage_db_path.lock().await.clone();
        if let Some(path) = path {
            let data = self.usage.read().await.clone();
            tokio::task::spawn_blocking(move || {
                let _ = crate::usage_db::save_usage(&path, &data);
            });
        }
    }
}
