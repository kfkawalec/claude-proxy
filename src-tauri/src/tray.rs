use crate::config::save_config;
use crate::i18n::{detect_lang, tr, Lang};
use crate::state::{AppState, ProxyStatus};
use std::sync::Arc;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    AppHandle, Emitter, Listener,
};

#[cfg(target_os = "windows")]
fn apply_windows_tray_icon_for_theme(tray: &tauri::tray::TrayIcon) {
    use dark_light::Mode;
    let bytes: &'static [u8] = match dark_light::detect().unwrap_or(Mode::Unspecified) {
        Mode::Dark => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/icons/tray-icon-win-dark.png"
        )),
        Mode::Light | Mode::Unspecified => {
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/icons/tray-icon.png"))
        }
    };
    if let Ok(img) = image::load_from_memory(bytes) {
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let icon = tauri::image::Image::new_owned(rgba.into_raw(), w, h);
        let _ = tray.set_icon(Some(icon));
    }
}

fn hub_display_label(lang: Lang, configured: &str) -> String {
    let t = configured.trim();
    if !t.is_empty() {
        return t.to_string();
    }
    tr(lang, "Hub", "Hub").to_string()
}

fn proxy_status_line(lang: Lang, status: &ProxyStatus, port: u16) -> String {
    match status {
        ProxyStatus::Running => format!(
            "{} (127.0.0.1:{port})",
            tr(lang, "Serwer: działa", "Server: Running")
        ),
        ProxyStatus::Stopped => tr(lang, "Serwer: zatrzymany", "Server: Stopped").into(),
        ProxyStatus::Error(msg) => {
            let short = msg.chars().take(36).collect::<String>();
            format!("{} - {short}", tr(lang, "Serwer: błąd", "Server: Error"))
        }
    }
}

fn build_menu(
    app: &AppHandle,
    provider: &str,
    status: &ProxyStatus,
    port: u16,
    litellm_display_name: &str,
) -> Result<Menu<tauri::Wry>, tauri::Error> {
    let lang = detect_lang();
    let hub = hub_display_label(lang, litellm_display_name);
    let litellm_menu_label = format!(
        "{} {}",
        tr(lang, "Dostawca:", "Provider:"),
        hub
    );

    let status_text = proxy_status_line(lang, status, port);
    let header = MenuItem::with_id(app, "header", &status_text, false, None::<&str>)?;
    let sep0 = PredefinedMenuItem::separator(app)?;

    let claude = CheckMenuItem::with_id(
        app,
        "provider_claude",
        tr(lang, "Provider: Claude", "Provider: Claude"),
        true,
        provider == "claude",
        None::<&str>,
    )?;
    let litellm_item = CheckMenuItem::with_id(
        app,
        "provider_litellm",
        &litellm_menu_label,
        true,
        provider == "litellm",
        None::<&str>,
    )?;

    let sep1 = PredefinedMenuItem::separator(app)?;

    let open_settings = MenuItem::with_id(
        app,
        "open_settings",
        tr(lang, "Ustawienia…", "Open Settings…"),
        true,
        Some("CmdOrCtrl+S"),
    )?;

    let sep2 = PredefinedMenuItem::separator(app)?;

    let (power_label, power_id) = match status {
        ProxyStatus::Running => (tr(lang, "Zatrzymaj serwer proxy", "Stop Server"), "stop_proxy"),
        _ => (tr(lang, "Uruchom serwer proxy", "Start Server"), "start_proxy"),
    };
    let power = MenuItem::with_id(app, power_id, power_label, true, None::<&str>)?;

    let sep3 = PredefinedMenuItem::separator(app)?;

    let copy_url = MenuItem::with_id(
        app,
        "copy_proxy_url",
        tr(lang, "Kopiuj URL proxy", "Copy Proxy URL"),
        true,
        Some("CmdOrCtrl+C"),
    )?;

    let sep4 = PredefinedMenuItem::separator(app)?;

    let quit = MenuItem::with_id(
        app,
        "quit",
        tr(lang, "Zakończ", "Quit"),
        true,
        Some("CmdOrCtrl+Q"),
    )?;

    Menu::with_items(
        app,
        &[
            &header,
            &sep0,
            &claude,
            &litellm_item,
            &sep1,
            &open_settings,
            &sep2,
            &power,
            &sep3,
            &copy_url,
            &sep4,
            &quit,
        ],
    )
}

/// Odświeża menu traya. Bezpieczne z dowolnego wątku - nie używa `block_on` w runtime Tokio
/// (to powodowało SIGABRT przy `provider-changed` / przełączaniu z UI).
pub fn schedule_tray_menu_update(app: &AppHandle, state: &Arc<AppState>) {
    let app = app.clone();
    let state = state.clone();
    tauri::async_runtime::spawn(async move {
        let (provider, status, port, litellm_display) = {
            let cfg = state.config.read().await;
            let st = state.proxy_status.read().await.clone();
            (
                cfg.provider.clone(),
                st,
                cfg.listen.port,
                cfg.litellm.litellm_display_name.clone(),
            )
        };
        let app_mt = app.clone();
        let _ = app.run_on_main_thread(move || {
            let lang = detect_lang();
            let Some(tray) = app_mt.tray_by_id("main-tray") else {
                return;
            };
            if let Ok(menu) =
                build_menu(&app_mt, &provider, &status, port, &litellm_display)
            {
                let _ = tray.set_menu(Some(menu));
            }
            #[cfg(target_os = "windows")]
            apply_windows_tray_icon_for_theme(&tray);
            let route = if provider == "litellm" {
                hub_display_label(lang, &litellm_display)
            } else {
                "Claude".to_string()
            };
            let st_short = match &status {
                ProxyStatus::Running => "●",
                ProxyStatus::Stopped => "○",
                ProxyStatus::Error(_) => "!",
            };
            let _ = tray.set_tooltip(Some(format!(
                "Claude Proxy {st_short} {route} · :{port}"
            )));
        });
    });
}

pub fn setup_tray(app: &AppHandle, state: Arc<AppState>) -> Result<(), Box<dyn std::error::Error>> {
    let tray = app.tray_by_id("main-tray").expect("tray not found");
    #[cfg(target_os = "windows")]
    apply_windows_tray_icon_for_theme(&tray);

    schedule_tray_menu_update(app, &state);

    {
        let app_h = app.clone();
        let st = state.clone();
        app.listen("provider-changed", move |_| {
            schedule_tray_menu_update(&app_h, &st);
        });
    }

    {
        let app_h = app.clone();
        let st = state.clone();
        app.listen("proxy-status-changed", move |_| {
            schedule_tray_menu_update(&app_h, &st);
        });
    }

    tray.on_menu_event({
        let state = state.clone();
        move |app, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "open_settings" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = crate::commands::open_settings(app).await;
                });
            }
            "copy_proxy_url" => {
                let st = state.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = crate::commands::copy_proxy_endpoint_inner(&st).await;
                });
            }
            "stop_proxy" => {
                let app = app.clone();
                let st = state.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = crate::commands::stop_proxy_inner(&st).await;
                    schedule_tray_menu_update(&app, &st);
                });
            }
            "start_proxy" => {
                let app = app.clone();
                let st = state.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = crate::commands::start_proxy_inner(&st, app.clone()).await;
                    schedule_tray_menu_update(&app, &st);
                });
            }
            id @ ("provider_claude" | "provider_litellm") => {
                let provider = if id == "provider_claude" {
                    "claude"
                } else {
                    "litellm"
                };
                let state = state.clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let mut cfg = state.config.write().await;
                    cfg.provider = provider.to_string();
                    save_config(&cfg);
                    drop(cfg);
                    let _ = app.emit("provider-changed", provider);
                });
            }
            _ => {}
        }
    });

    Ok(())
}
