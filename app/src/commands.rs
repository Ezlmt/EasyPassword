//! IPC commands called from the web UI.

use easypassword_core::{concat, generate, MasterSeed, Mode, OutputMode, Settings, SiteParams};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt as _;
use zeroize::{Zeroize, Zeroizing};

use crate::output::{self, Action};
use crate::popup;
use crate::state::{lock, now_secs, AppState};

#[derive(Serialize)]
pub struct Status {
    has_seed: bool,
    has_concat_prefix: bool,
    default_mode: Mode,
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
    let has_concat_prefix = state.concat_prefix()?.is_some();
    let settings = lock(&state.settings).clone();
    Ok(Status {
        has_seed: seed.is_some(),
        has_concat_prefix,
        default_mode: settings.default_mode,
        fingerprint: seed.map(|s| s.fingerprint()),
        identity: settings.identity,
        output: settings.output,
        shortcut: settings.shortcut,
        platform: PLATFORM,
    })
}

/// Derives the seed (slow, memory-hard) and stores it in the keychain.
/// `concat_prefix` (optional) is the old 0.x `master_key` used for "!!"
/// passwords; when given, concat becomes the default mode for new sites.
/// Returns the fingerprint for the user to verify.
#[tauri::command]
pub async fn setup_master_key(
    state: State<'_, AppState>,
    mut master_key: String,
    identity: String,
    concat_prefix: Option<String>,
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

    let prefix = concat_prefix.map(Zeroizing::new).filter(|p| !p.is_empty());
    if let Some(prefix) = &prefix {
        crate::keystore::store_prefix(prefix)?;
    }

    let mut settings = lock(&state.settings);
    settings.identity = identity;
    if let Some(prefix) = prefix {
        state.set_concat_prefix(Some(prefix));
        settings.default_mode = Mode::Concat;
    }
    state.save_settings(&settings)?;
    Ok(fingerprint)
}

#[tauri::command]
pub fn set_concat_prefix(state: State<'_, AppState>, prefix: String) -> Result<(), String> {
    let prefix = Zeroizing::new(prefix);
    if prefix.is_empty() {
        return Err("拼接前缀不能为空".into());
    }
    crate::keystore::store_prefix(&prefix)?;
    state.set_concat_prefix(Some(prefix));
    Ok(())
}

#[tauri::command]
pub fn forget_concat_prefix(state: State<'_, AppState>) -> Result<(), String> {
    crate::keystore::delete_prefix()?;
    state.set_concat_prefix(None);
    Ok(())
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
    let password = match params.mode {
        Mode::Derived => {
            let seed = state.seed()?.ok_or("尚未设置主密钥")?;
            generate(&seed, &params.site, params.counter, &params.rules)
                .map_err(|e| e.to_string())?
        }
        Mode::Concat => {
            let prefix = state.concat_prefix()?.ok_or(
                "尚未设置拼接前缀：请在 设置 → 旧版拼接密码 中填写旧版配置里的 master_key",
            )?;
            concat(&prefix, &params.literal).map_err(|e| e.to_string())?
        }
    };

    let (mode, clear_secs) = {
        let mut settings = lock(&state.settings);
        if settings.record_use(&params, now_secs()) {
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
    has_concat_prefix: bool,
    fingerprint: Option<String>,
    platform: &'static str,
}

#[tauri::command]
pub async fn get_settings(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SettingsView, String> {
    let seed = state.seed()?;
    let has_concat_prefix = state.concat_prefix()?.is_some();
    Ok(SettingsView {
        settings: lock(&state.settings).clone(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        has_seed: seed.is_some(),
        has_concat_prefix,
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
    settings
        .defaults
        .validate()
        .map_err(|e| format!("默认规则无效: {e}"))?;
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
