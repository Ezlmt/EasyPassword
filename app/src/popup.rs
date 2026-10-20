//! The generator popup window.

use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

pub const POPUP: &str = "popup";
pub const SETTINGS: &str = "settings";

fn window(app: &AppHandle, label: &str) -> Option<WebviewWindow> {
    app.get_webview_window(label)
}

pub fn toggle(app: &AppHandle) {
    let Some(w) = window(app, POPUP) else { return };
    let visible = w.is_visible().unwrap_or(false);
    let focused = w.is_focused().unwrap_or(false);
    if visible && focused {
        hide(app);
    } else {
        show(app);
    }
}

pub fn show(app: &AppHandle) {
    let Some(w) = window(app, POPUP) else { return };

    #[cfg(target_os = "windows")]
    {
        let fg = crate::platform::windows::foreground_window();
        let ours = w.hwnd().map(|h| h.0 as isize).unwrap_or(0);
        if fg != 0 && fg != ours {
            *crate::state::lock(&app.state::<crate::state::AppState>().previous_window) = fg;
        }
    }
    #[cfg(target_os = "macos")]
    {
        // Undo a previous `app.hide()`.
        let _ = app.show();
    }

    let _ = w.center();
    let _ = w.show();
    let _ = w.set_focus();
    let _ = w.emit("popup-shown", ());
}

pub fn hide(app: &AppHandle) {
    if let Some(w) = window(app, POPUP) {
        let _ = w.hide();
    }
}

/// Hides the popup and makes sure the window the user came from gets the
/// keyboard focus back.
pub fn hide_and_restore_focus(app: &AppHandle) {
    hide(app);
    #[cfg(target_os = "macos")]
    {
        // Hiding the whole (accessory) app re-activates the previous app.
        let _ = app.hide();
    }
    #[cfg(target_os = "windows")]
    {
        let hwnd = *crate::state::lock(&app.state::<crate::state::AppState>().previous_window);
        crate::platform::windows::activate_window(hwnd);
    }
}

/// Re-opens the popup with an error message (e.g. typing failed).
pub fn show_error(app: &AppHandle, message: &str) {
    show(app);
    if let Some(w) = window(app, POPUP) {
        let _ = w.emit("popup-error", message.to_string());
    }
}

pub fn open_settings(app: &AppHandle) {
    hide(app);
    #[cfg(target_os = "macos")]
    {
        let _ = app.show();
    }
    if let Some(w) = window(app, SETTINGS) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        let _ = w.emit("settings-shown", ());
    }
}
