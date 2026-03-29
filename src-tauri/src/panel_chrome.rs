//! Widoczność Dock / tray zależna od stanu głównego okna.
use tauri::{AppHandle, Manager};

pub fn sync_panel_chrome_from_window(app: &AppHandle) {
    let panel_showing = app
        .get_webview_window("main")
        .map(|w| {
            let visible = w.is_visible().unwrap_or(false);
            let minimized = w.is_minimized().unwrap_or(false);
            visible && !minimized
        })
        .unwrap_or(false);

    // Tray should remain visible even when the panel window is hidden.
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_visible(true);
    }

    #[cfg(target_os = "macos")]
    {
        let policy = if panel_showing {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        let _ = app.set_activation_policy(policy);
    }
}
