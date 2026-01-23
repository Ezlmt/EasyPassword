use crossbeam_channel::Sender;
use rdev::listen;
use rdev::Event;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

use crate::core::GenerationMode;
use crate::error::Result;

use super::super::{TriggerDetector, TriggerEvent};

pub fn start_keyboard_listener(
    tx: Sender<TriggerEvent>,
    triggers: Vec<(String, GenerationMode)>,
    injection_active: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>> {
    let handle = thread::spawn(move || {
        let mut detector = TriggerDetector::new(triggers.clone(), injection_active);

        log::info!(
            "[LISTENER] Keyboard listener started, triggers: {:?}",
            triggers
        );

        let callback = move |event: Event| {
            if let Some(trigger) = detector.process_event(&event) {
                log::info!("[SEND] Sending trigger event: {:?}", trigger);
                if let Err(e) = tx.send(trigger) {
                    log::error!("[ERROR] Failed to send trigger: {}", e);
                }
            }
        };

        log::info!("[LISTENER] Starting rdev::listen...");
        if let Err(e) = listen(callback) {
            log::error!("[ERROR] Keyboard listener error: {:?}", e);
        }
        log::info!("[LISTENER] rdev::listen exited");
    });

    Ok(handle)
}
