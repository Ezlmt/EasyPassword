//! IPC commands called from the web UI.

use easypassword_core::{generate, MasterSeed, OutputMode, Settings, SiteParams};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt as _;
use zeroize::Zeroize;

use crate::output::{self, Action};
use crate::popup;
use crate::state::{lock, now_secs, AppState};

#[derive(Serialize)]
pub struct Status {
    has_seed: bool,
    fingerprint: Option<String>,
    identity: String,
    output: OutputMode,
    shortcut: String,
    platform: &'static str,
}

const PLATFORM: &str = if cfg!(target_os = "macos") {
    "macos"
} else if cfg!(target_os = "windows") {
    "windows"
} else {
    "other"
};

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> Result<Status, String> {
    let seed = state.seed()?;
    let settings = lock(&state.settings).clone();
    Ok(Status {
        has_seed: seed.is_some(),
        fingerprint: seed.map(|s| s.fingerprint()),
        identity: settings.identity,
        output: settings.output,
        shortcut: settings.shortcut,
        platform: PLATFORM,
    })
}

/// Derives the seed (slow, memory-hard) and stores it in the keychain.
/// Returns the fingerprint for the user to verify.
#[tauri::command]
pub async fn setup_master_key(
    state: State<'_, AppState>,
    mut master_key: String,
    identity: String,
) -> Result<String, String> {
    let identity = identity.trim().to_string();
    let id = identity.clone();
    let key = std::mem::take(&mut master_key);
    let seed = tauri::async_runtime::spawn_blocking(move || {
        let mut key = key;
        let seed = MasterSeed::derive(&key, &id);
        key.zeroize();
        seed
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    crate::keystore::store(&seed)?;
    let fingerprint = seed.fingerprint();
    state.set_seed(Some(seed));

    let mut settings = lock(&state.settings);
    settings.identity = identity;
    state.save_settings(&settings)?;
    Ok(fingerprint)
}

#[tauri::command]
pub fn forget_master_key(state: State<'_, AppState>) -> Result<(), String> {
    crate::keystore::delete()?;
    state.set_seed(None);
    Ok(())
}

#[tauri::command]
pub fn suggest(state: State<'_, AppState>, query: String) -> Vec<String> {
    lock(&state.settings).suggestions(&query, 8)
}

#[tauri::command]
pub fn site_params(state: State<'_, AppState>, site: String) -> SiteParams {
    lock(&state.settings).params_for(&site)
}

#[derive(Serialize)]
pub struct SubmitResult {
    /// The popup was hidden (delivery is in progress).
    hidden: bool,
    /// Shown in the popup when `hidden` is false.
    message: Option<String>,
}

/// Generates the password and delivers it. `alternate` = Ctrl/Cmd+Enter,
/// which swaps "type" and "copy".
#[tauri::command]
pub fn submit(
    app: AppHandle,
    state: State<'_, AppState>,
    params: SiteParams,
    alternate: bool,
) -> Result<SubmitResult, String> {
    let seed = state.seed()?.ok_or("尚未设置主密钥")?;
    let password = generate(&seed, &params.site, params.counter, &params.rules)
        .map_err(|e| e.to_string())?;

    let (mode, clear_secs) = {
        let mut settings = lock(&state.settings);
        let now = now_secs();
        let stored = settings.params_for(&params.site);
        let mut changed = settings.touch(&params.site, now);
        if stored.counter != params.counter
            || stored.rules != params.rules
            || stored.username != params.username.trim()
        {
            let normalized = SiteParams {
                site: stored.site.clone(),
                ..params.clone()
            };
            settings.set_params(&normalized, now);
            changed = true;
        }
        if changed {
            if let Err(e) = state.save_settings(&settings) {
                log::error!("{e}");
            }
        }
        (settings.output, settings.clipboard_clear_seconds)
    };

    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut action = match (mode, alternate) {
        (OutputMode::Type, false) | (OutputMode::Copy, true) => Action::Type,
        (OutputMode::Type, true) | (OutputMode::Copy, false) => Action::Copy,
    };

    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut message = None;
    #[cfg(target_os = "macos")]
    if action == Action::Type && !crate::platform::macos::accessibility_trusted(true) {
        action = Action::Copy;
        message = Some(format!(
            "EasyPassword 还没有「辅助功能」权限，无法自动输入。已复制到剪贴板（{clear_secs} 秒后清除）。\n\
             请在 系统设置 → 隐私与安全性 → 辅助功能 中允许 EasyPassword，然后重新打开它。"
        ));
    }

    match action {
        Action::Copy => {
            output::copy(&app, &password, clear_secs)?;
            if message.is_none() {
                popup::hide_and_restore_focus(&app);
            }
            Ok(SubmitResult {
                hidden: message.is_none(),
                message,
            })
        }
        Action::Type | Action::Paste => {
            output::deliver_to_previous_window(&app, password, action);
            Ok(SubmitResult {
                hidden: true,
                message: None,
            })
        }
    }
}

#[tauri::command]
pub fn hide_popup(app: AppHandle) {
    popup::hide(&app);
}

#[tauri::command]
pub fn open_settings(app: AppHandle) {
    popup::open_settings(&app);
}

#[derive(Serialize)]
pub struct SettingsView {
    settings: Settings,
    autostart: bool,
    has_seed: bool,
    fingerprint: Option<String>,
    platform: &'static str,
}

#[tauri::command]
pub async fn get_settings(app: AppHandle, state: State<'_, AppState>) -> Result<SettingsView, String> {
    let seed = state.seed()?;
    Ok(SettingsView {
        settings: lock(&state.settings).clone(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        has_seed: seed.is_some(),
        fingerprint: seed.map(|s| s.fingerprint()),
        platform: PLATFORM,
    })
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    mut settings: Settings,
) -> Result<(), String> {
    settings.defaults.validate().map_err(|e| format!("默认规则无效: {e}"))?;
    settings.shortcut = settings.shortcut.trim().to_string();

    let old_shortcut = lock(&state.settings).shortcut.clone();
    if settings.shortcut != old_shortcut {
        crate::shortcut::replace(&app, &old_shortcut, &settings.shortcut)?;
    }

    let mut current = lock(&state.settings);
    // The UI edits everything except identity (bound to the stored seed).
    settings.identity = current.identity.clone();
    if !settings.remember_sites {
        settings.forget_history();
    }
    state.save_settings(&settings)?;
    *current = settings;
    drop(current);
    crate::tray::refresh(&app);
    Ok(())
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    crate::tray::set_autostart(&app, enabled)
}

#[tauri::command]
pub fn forget_history(state: State<'_, AppState>) -> Result<(), String> {
    let mut settings = lock(&state.settings);
    settings.forget_history();
    state.save_settings(&settings)
}

#[tauri::command]
pub fn delete_site(state: State<'_, AppState>, site: String) -> Result<(), String> {
    let mut settings = lock(&state.settings);
    settings.sites.remove(&site);
    state.save_settings(&settings)
}

/// Used by the setup screen to tell the user what will happen.
#[tauri::command]
pub fn check_input_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::accessibility_trusted(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}
