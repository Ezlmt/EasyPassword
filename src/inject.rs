use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::error::{EasyPasswordError, Result};

#[cfg(target_os = "macos")]
const KEYSTROKE_DELAY_MS: u64 = 5;

#[cfg(target_os = "windows")]
const KEYSTROKE_DELAY_MS: u64 = 2;

#[cfg(target_os = "linux")]
const KEYSTROKE_DELAY_MS: u64 = 5;

// A small guard delay around injection to reduce self-triggering and to give
// the target application a moment to process backspaces before typing.
const INJECTION_GUARD_DELAY_MS: u64 = 10;

pub struct TextInjector {
    enigo: Enigo,
    injection_active: Arc<AtomicBool>,
}

impl TextInjector {
    pub fn new(injection_active: Arc<AtomicBool>) -> Result<Self> {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| EasyPasswordError::TextInjection(e.to_string()))?;

        Ok(Self {
            enigo,
            injection_active,
        })
    }

    pub fn replace_trigger(&mut self, backspace_count: usize, replacement: &str) -> Result<()> {
        self.injection_active.store(true, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(INJECTION_GUARD_DELAY_MS));

        let result = self.do_replacement(backspace_count, replacement);

        thread::sleep(Duration::from_millis(INJECTION_GUARD_DELAY_MS));
        self.injection_active.store(false, Ordering::SeqCst);

        result
    }

    fn do_replacement(&mut self, backspace_count: usize, replacement: &str) -> Result<()> {
        for _ in 0..backspace_count {
            self.enigo
                .key(Key::Backspace, Direction::Click)
                .map_err(|e| EasyPasswordError::TextInjection(e.to_string()))?;
            thread::sleep(Duration::from_millis(KEYSTROKE_DELAY_MS));
        }

        thread::sleep(Duration::from_millis(INJECTION_GUARD_DELAY_MS));

        self.enigo
            .text(replacement)
            .map_err(|e| EasyPasswordError::TextInjection(e.to_string()))?;

        Ok(())
    }

    pub fn clear_text(&mut self, char_count: usize) -> Result<()> {
        self.injection_active.store(true, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(INJECTION_GUARD_DELAY_MS));

        for _ in 0..char_count {
            self.enigo
                .key(Key::Backspace, Direction::Click)
                .map_err(|e| EasyPasswordError::TextInjection(e.to_string()))?;
            thread::sleep(Duration::from_millis(KEYSTROKE_DELAY_MS));
        }

        thread::sleep(Duration::from_millis(INJECTION_GUARD_DELAY_MS));
        self.injection_active.store(false, Ordering::SeqCst);

        Ok(())
    }
}
