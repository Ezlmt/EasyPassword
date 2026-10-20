use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Mutex, MutexGuard};

use easypassword_core::{MasterSeed, Settings};

/// Process-wide state, managed by Tauri.
pub struct AppState {
    pub settings_path: PathBuf,
    pub settings: Mutex<Settings>,
    /// `None` until the keychain has been read; `Some(None)` = no key set up.
    pub seed: Mutex<Option<Option<MasterSeed>>>,
    /// Bumped on every clipboard write so stale "clear" timers do nothing.
    pub clipboard_generation: AtomicU64,
    /// Window that was focused before the popup opened (Windows HWND).
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub previous_window: Mutex<isize>,
}

impl AppState {
    pub fn new(settings_path: PathBuf, settings: Settings) -> Self {
        Self {
            settings_path,
            settings: Mutex::new(settings),
            seed: Mutex::new(None),
            clipboard_generation: AtomicU64::new(0),
            previous_window: Mutex::new(0),
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        settings
            .save(&self.settings_path)
            .map_err(|e| format!("保存设置失败: {e}"))
    }

    /// Returns the cached seed, reading the keychain on first use.
    pub fn seed(&self) -> Result<Option<MasterSeed>, String> {
        let mut guard = lock(&self.seed);
        if guard.is_none() {
            *guard = Some(crate::keystore::load()?);
        }
        Ok(guard.as_ref().and_then(|s| s.clone()))
    }

    pub fn set_seed(&self, seed: Option<MasterSeed>) {
        *lock(&self.seed) = Some(seed);
    }
}

/// Locks a mutex, recovering from poisoning (a panicked holder can't leave
/// our plain-data state inconsistent in a dangerous way).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
