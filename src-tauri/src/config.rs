use crate::state::{AppConfig, AppState};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;

fn config_dir() -> PathBuf {
    let home = dirs_next().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("claude-proxy")
}

fn dirs_next() -> Option<PathBuf> {
    Some(crate::platform::home_dir())
}

fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn load_config() -> AppConfig {
    let path = config_path();
    if path.exists() {
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str::<AppConfig>(&data) {
                return config;
            }
        }
    }
    AppConfig::default()
}

pub fn save_config(config: &AppConfig) {
    let dir = config_dir();
    let _ = fs::create_dir_all(&dir);
    let path = config_path();
    if let Ok(data) = serde_json::to_string_pretty(config) {
        let _ = fs::write(&path, data);
    }
}

pub fn watch_config_file(state: Arc<AppState>, app_handle: tauri::AppHandle) {
    let path = config_path();
    let dir = config_dir();
    let _ = fs::create_dir_all(&dir);

    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[config] Failed to create watcher: {}", e);
                return;
            }
        };
        if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
            eprintln!("[config] Failed to watch {}: {}", dir.display(), e);
            return;
        }
        println!("[config] Watching {}", path.display());

        for event in rx {
            if let Ok(event) = event {
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    if event.paths.iter().any(|p| p.ends_with("config.json")) {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        let new_config = load_config();
                        let state = state.clone();
                        let app = app_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            let mut cfg = state.config.write().await;
                            *cfg = new_config.clone();
                            let _ = app.emit("provider-changed", &new_config.provider);
                        });
                    }
                }
            }
        }
    });
}
