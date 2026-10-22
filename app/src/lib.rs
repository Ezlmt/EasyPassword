//! EasyPassword desktop app (Tauri 2).
//!
//! Press the global shortcut -> a small popup asks for the site -> Enter types
//! the derived password into the window you came from (or copies it).
//! No global keyboard monitoring is involved.

mod commands;
mod keystore;
mod output;
mod platform;
mod popup;
mod shortcut;
mod state;
mod tray;

use easypassword_core::Settings;
use tauri::{Manager, WindowEvent};

use crate::state::AppState;

fn init_logging() {
    let mut builder =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
    // enigo / arboard log the text they type or copy at debug level. Never
    // let that through, whatever RUST_LOG says.
    builder
        .filter_module("enigo", log::LevelFilter::Warn)
        .filter_module("arboard", log::LevelFilter::Warn);
    let _ = builder.try_init();
}

pub fn run() {
    init_logging();

    let app = tauri::Builder::default()
        // Must be the first plugin: a second launch just opens the popup of
        // the running instance instead of starting a duplicate.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            popup::show(app);
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::setup_master_key,
            commands::forget_master_key,
            commands::set_concat_prefix,
            commands::forget_concat_prefix,
            commands::suggest,
            commands::site_params,
            commands::submit,
            commands::hide_popup,
            commands::open_settings,
            commands::get_settings,
            commands::save_settings,
            commands::set_autostart,
            commands::forget_history,
            commands::delete_site,
            commands::check_input_permission,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let config_dir = app.path().app_config_dir()?;
            let settings_path = config_dir.join("settings.toml");
            let settings = Settings::load(&settings_path).unwrap_or_else(|e| {
                log::error!(
                    "failed to load {}: {e}; using defaults",
                    settings_path.display()
                );
                Settings::default()
            });
            let first_run = !settings_path.exists();
            let shortcut = settings.shortcut.clone();
            app.manage(AppState::new(settings_path, settings));

            let handle = app.handle().clone();
            if let Err(e) = shortcut::register(&handle, &shortcut) {
                log::error!("{e}");
            }
            tray::create(&handle)?;

            // Show the popup on first run (master key setup) and when the user
            // launches the app manually; stay in the tray when auto-started.
            let autostarted = std::env::args().any(|a| a == "--autostart");
            if first_run || !autostarted {
                popup::show(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                // Windows are reused; closing just hides them.
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::Focused(false) if window.label() == popup::POPUP => {
                let _ = window.hide();
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("failed to build EasyPassword");

    app.run(|_app, event| {
        // Keep running in the tray when all windows are hidden/closed.
        if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
