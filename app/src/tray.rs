//! System tray icon and menu.

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt as _;

use crate::state::{lock, AppState};

const TRAY_ID: &str = "main";

/// Menu items whose state we update later.
pub struct TrayItems {
    open: MenuItem<Wry>,
    autostart: CheckMenuItem<Wry>,
}

fn open_label(shortcut: &str) -> String {
    let pretty = shortcut
        .replace("CommandOrControl", if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" })
        .replace("CmdOrCtrl", if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" });
    format!("生成密码…    {pretty}")
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let shortcut = lock(&app.state::<AppState>().settings).shortcut.clone();
    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);

    let open = MenuItem::with_id(app, "open", open_label(&shortcut), true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
    let autostart =
        CheckMenuItem::with_id(app, "autostart", "开机自动启动", true, autostart_enabled, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 EasyPassword", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &autostart,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .tooltip("EasyPassword")
        .menu(&menu)
        // macOS convention: left click opens the menu. Windows: left click
        // opens the generator, right click the menu.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => crate::popup::show(app),
            "settings" => crate::popup::open_settings(app),
            "autostart" => {
                let enabled = app.autolaunch().is_enabled().unwrap_or(false);
                if let Err(e) = set_autostart(app, !enabled) {
                    log::error!("{e}");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if cfg!(target_os = "macos") {
                return;
            }
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::popup::toggle(tray.app_handle());
            }
        })
        .build(app)?;

    app.manage(TrayItems { open, autostart });
    Ok(())
}

/// Enables/disables start-on-login and syncs the tray checkbox.
/// Returns the resulting state.
pub fn set_autostart(app: &AppHandle, enabled: bool) -> Result<bool, String> {
    let launcher = app.autolaunch();
    let result = if enabled { launcher.enable() } else { launcher.disable() };
    let actual = launcher.is_enabled().unwrap_or(false);
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.autostart.set_checked(actual);
    }
    result.map_err(|e| format!("设置开机启动失败: {e}"))?;
    Ok(actual)
}

/// Updates labels after settings changed.
pub fn refresh(app: &AppHandle) {
    let shortcut = lock(&app.state::<AppState>().settings).shortcut.clone();
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.open.set_text(open_label(&shortcut));
    }
}
