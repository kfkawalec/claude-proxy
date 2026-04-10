mod commands;
mod config;
mod i18n;
mod panel_chrome;
mod platform;
mod proxy;
mod state;
mod tray;
mod usage_db;

use config::{load_config, watch_config_file};
use panel_chrome::sync_panel_chrome_from_window;
use state::AppState;
#[cfg(target_os = "macos")]
use tauri::Emitter;
use tauri::{Manager, RunEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_config = load_config();
    let app_state = AppState::new(app_config);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(app_state.clone())
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_settings,
            commands::set_provider,
            commands::get_proxy_status,
            commands::get_usage,
            commands::reset_usage,
            commands::fetch_models,
            commands::fetch_budget_info,
            commands::fetch_litellm_daily_activity,
            commands::check_claude_auth,
            commands::claude_login,
            commands::open_url,
            commands::stop_proxy_server,
            commands::start_proxy_server,
            commands::copy_proxy_endpoint,
            commands::open_settings,
            commands::get_claude_install_status,
            commands::install_claude_proxy_settings,
            commands::uninstall_claude_proxy_settings,
            commands::fetch_claude_rate_limits,
            commands::get_proxy_activity,
        ])
        .setup(move |app| {
            let state = app_state.clone();
            tray::setup_tray(app.handle(), state.clone())?;

            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("app_data_dir: {e}"))?;
            std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
            let db_path = data_dir.join("usage.db");
            usage_db::init_schema(&db_path).map_err(|e| e.to_string())?;
            tauri::async_runtime::block_on(async {
                if let Ok(u) = usage_db::load_usage(&db_path) {
                    *state.usage.write().await = u;
                }
                *state.usage_db_path.lock().await = Some(db_path);
                *state.app_handle.write().await = Some(app.handle().clone());
            });

            watch_config_file(state.clone(), app.handle().clone());

            let proxy_state = state.clone();
            let shutdown_rx = state.shutdown_rx.clone();
            let app_h = app.handle().clone();
            let h = tauri::async_runtime::spawn(async move {
                proxy::server::run_proxy(proxy_state, shutdown_rx, app_h).await;
            });
            tauri::async_runtime::block_on(async {
                *state.proxy_task.lock().await = Some(h);
            });

            if let Some(win) = app.get_webview_window("main") {
                let app_h = app.handle().clone();
                win.on_window_event(move |event| {
                    match event {
                        tauri::WindowEvent::CloseRequested { api, .. } => {
                            api.prevent_close();
                            if let Some(w) = app_h.get_webview_window("main") {
                                let _ = w.hide();
                            }
                            sync_panel_chrome_from_window(&app_h);
                        }
                        tauri::WindowEvent::Focused(_) => {
                            sync_panel_chrome_from_window(&app_h);
                        }
                        _ => {}
                    }
                });
            }

            sync_panel_chrome_from_window(app.handle());

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Claude Proxy");

    app.run(|app_handle, event| {
        match event {
            RunEvent::Ready => {
                // Start hidden: tray only, no Dock icon until window is shown.
                sync_panel_chrome_from_window(app_handle);
            }
            #[cfg(target_os = "macos")]
            RunEvent::Reopen {
                has_visible_windows,
                ..
            } => {
                if !has_visible_windows {
                    if let Some(win) = app_handle.get_webview_window("main") {
                        let _ = win.unminimize();
                        let _ = win.center();
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                    let _ = app_handle.emit("focus-usage", ());
                    sync_panel_chrome_from_window(app_handle);
                }
            }
            _ => {}
        }
    });
}
