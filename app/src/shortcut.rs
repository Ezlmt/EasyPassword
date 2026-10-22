//! Global shortcut registration.

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub fn register(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                crate::popup::toggle(app);
            }
        })
        .map_err(|e| format!("无法注册快捷键「{shortcut}」: {e}（可能格式不对或已被其他程序占用）"))
}

/// Swaps the shortcut; on failure the old one is re-registered.
pub fn replace(app: &AppHandle, old: &str, new: &str) -> Result<(), String> {
    let _ = app.global_shortcut().unregister(old);
    if let Err(e) = register(app, new) {
        let _ = register(app, old);
        return Err(e);
    }
    Ok(())
}
