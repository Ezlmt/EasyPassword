use crossbeam_channel::Sender;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::core::GenerationMode;
use crate::error::Result;

use super::TriggerEvent;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
mod rdev;

pub fn start_keyboard_listener(
    tx: Sender<TriggerEvent>,
    triggers: Vec<(String, GenerationMode)>,
    injection_active: Arc<AtomicBool>,
) -> Result<std::thread::JoinHandle<()>> {
    #[cfg(target_os = "macos")]
    {
        macos::start_keyboard_listener(tx, triggers, injection_active)
    }

    #[cfg(not(target_os = "macos"))]
    {
        rdev::start_keyboard_listener(tx, triggers, injection_active)
    }
}
