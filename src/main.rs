#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use std::fs;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

use clap::Parser;
use crossbeam_channel::{select, unbounded, Receiver, Sender};

use std::process::Command;

use easypassword::core::GenerationMode;
use easypassword::{
    generate_password, start_keyboard_listener, Config, TextInjector, TriggerEvent,
};

mod autostart;
mod tray;

#[derive(Debug, Clone)]
pub enum ControlCommand {
    ReloadConfig,
    OpenConfig,
    SetAutostart(bool),
    Exit,
}

#[derive(Debug, Clone)]
pub enum TrayUpdate {
    AutostartSetResult {
        enabled: bool,
        ok: bool,
        error: Option<String>,
    },
}

#[derive(Parser)]
#[command(name = "easypassword")]
#[command(about = "A local-only deterministic password generator")]
#[command(version)]
struct Cli {
    #[arg(short, long)]
    verbose: bool,
}

fn log_path() -> Option<PathBuf> {
    let config_path = Config::config_path().ok()?;
    let dir = config_path.parent()?;
    Some(dir.join("easypassword.log"))
}

fn init_logging(verbose: bool) {
    let mut builder = if verbose {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
    };

    if let Some(path) = log_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        if let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
    }

    builder.init();
}

fn handle_trigger(
    config: &Config,
    master_key: Option<&str>,
    injector: &mut TextInjector,
    trigger: TriggerEvent,
) {
    log::info!("[HANDLE] Received trigger: {:?}", trigger);

    let mut password_config = config.get_password_config(&trigger.site);

    if trigger.mode == GenerationMode::Concatenation {
        password_config.mode = GenerationMode::Concatenation;
    }

    let counter = config.get_counter(&trigger.site);

    let Some(master_key) = master_key else {
        log::error!(
            "master_key not set; cannot generate password (site={})",
            trigger.site
        );
        return;
    };

    log::info!("[HANDLE] Generating password for site={}", trigger.site);

    match generate_password(master_key, &trigger.site, counter, &password_config) {
        Ok(password) => {
            log::info!("[HANDLE] Password generated, injecting...");
            if let Err(e) = injector.replace_trigger(trigger.trigger_len, &password) {
                log::error!("injection failed (site={}): {}", trigger.site, e);
            } else {
                log::info!("[HANDLE] Injection successful");
            }
        }
        Err(e) => {
            log::error!("generation failed (site={}): {}", trigger.site, e);
        }
    }
}

fn open_config_file() -> anyhow::Result<()> {
    let path = Config::config_path()?;

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(Into::into)
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(Into::into)
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        Err(anyhow::anyhow!(
            "Open config is not supported on this platform"
        ))
    }
}

fn worker_loop(
    trigger_tx: Sender<TriggerEvent>,
    trigger_rx: Receiver<TriggerEvent>,
    command_rx: Receiver<ControlCommand>,
    tray_update_tx: Sender<TrayUpdate>,
) {

    // Load the config, if it fails, use the default config
    let mut config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            log::error!("failed to load config: {}", e);
            Config::default()
        }
    };

    // get the current autostart setting
    if let Err(e) = autostart::set_enabled(config.default.autostart) {
        log::error!("failed to apply autostart setting: {}", e);
    }

    let mut master_key = config
        .default
        .master_key
        .as_deref()
        .filter(|k| !k.is_empty())
        .map(str::to_string);

    // AtomicBool is a thread-safe boolean
    // Pre
    let injection_active = Arc::new(AtomicBool::new(false));

    let triggers = vec![
        (
            config.default.trigger_prefix.clone(),
            GenerationMode::Argon2id,
        ),
        (
            config.default.concat_trigger_prefix.clone(),
            GenerationMode::Concatenation,
        ),
    ];

    if let Err(e) = start_keyboard_listener(trigger_tx, triggers, injection_active.clone()) {
        log::error!("failed to start keyboard listener: {}", e);
        return;
    }

    let mut injector = match TextInjector::new(injection_active) {
        Ok(i) => i,
        Err(e) => {
            log::error!("failed to initialize injector: {}", e);
            return;
        }
    };

    loop {
        select! {
            recv(trigger_rx) -> msg => {
                match msg {
                    Ok(trigger) => {
                        handle_trigger(&config, master_key.as_deref(), &mut injector, trigger);
                    }
                    Err(e) => {
                        log::error!("trigger channel closed: {}", e);
                        break;
                    }
                }
            }
            recv(command_rx) -> msg => {
                match msg {
                    Ok(ControlCommand::ReloadConfig) => {
                        let previous_autostart = config.default.autostart;
                        match Config::load() {
                            Ok(c) => {
                                config = c;
                                master_key = config
                                    .default
                                    .master_key
                                    .as_deref()
                                    .filter(|k| !k.is_empty())
                                    .map(str::to_string);

                                let requested_autostart = config.default.autostart;
                                match autostart::set_enabled(requested_autostart) {
                                    Ok(()) => {
                                        let _ = tray_update_tx.send(TrayUpdate::AutostartSetResult {
                                            enabled: requested_autostart,
                                            ok: true,
                                            error: None,
                                        });
                                    }
                                    Err(e) => {
                                        config.default.autostart = previous_autostart;
                                        let _ = tray_update_tx.send(TrayUpdate::AutostartSetResult {
                                            enabled: previous_autostart,
                                            ok: false,
                                            error: Some(e.to_string()),
                                        });
                                        log::error!(
                                            "failed to apply autostart setting on reload: {}",
                                            e
                                        );
                                    }
                                }
                                log::info!("config reloaded");
                            }
                            Err(e) => {
                                log::error!("failed to reload config: {}", e);
                            }
                        }
                    }
                    Ok(ControlCommand::OpenConfig) => {
                        if let Err(e) = open_config_file() {
                            log::error!("failed to open config file: {}", e);
                        }
                    }
                    Ok(ControlCommand::SetAutostart(enabled)) => {
                        let previous = config.default.autostart;

                        let result = (|| -> anyhow::Result<()> {
                            autostart::set_enabled(enabled)?;
                            config.default.autostart = enabled;
                            config.save()?;
                            Ok(())
                        })();

                        match result {
                            Ok(()) => {
                                let _ = tray_update_tx.send(TrayUpdate::AutostartSetResult {
                                    enabled,
                                    ok: true,
                                    error: None,
                                });
                                log::info!("autostart set to {}", enabled);
                            }
                            Err(e) => {
                                config.default.autostart = previous;
                                let _ = tray_update_tx.send(TrayUpdate::AutostartSetResult {
                                    enabled: previous,
                                    ok: false,
                                    error: Some(e.to_string()),
                                });
                                log::error!("failed to set autostart: {}", e);
                            }
                        }
                    }
                    Ok(ControlCommand::Exit) => {
                        log::info!("exit requested");
                        std::process::exit(0);
                    }
                    Err(e) => {
                        log::error!("command channel closed: {}", e);
                        break;
                    }
                }
            }
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    init_logging(cli.verbose);

    let (trigger_tx, trigger_rx) = unbounded::<TriggerEvent>();
    let (command_tx, command_rx) = unbounded::<ControlCommand>();
    let (tray_update_tx, tray_update_rx) = unbounded::<TrayUpdate>();

    let initial_autostart = Config::load().map(|c| c.default.autostart).unwrap_or(false);

    let worker_trigger_tx = trigger_tx.clone();
    let _worker = thread::spawn(move || {
        worker_loop(worker_trigger_tx, trigger_rx, command_rx, tray_update_tx)
    });

    tray::run_tray(command_tx, tray_update_rx, initial_autostart)
}

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        log::error!("fatal error: {}", e);
        std::process::exit(1);
    }
}
