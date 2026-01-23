use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rdev::{Event, EventType, Key};

use crate::core::GenerationMode;
use crate::error::Result;

mod platform;

#[derive(Debug, Clone)]
pub struct TriggerEvent {
    pub site: String,
    pub trigger_len: usize,
    pub mode: GenerationMode,
}

#[derive(Debug, Clone, PartialEq)]
enum DetectorState {
    Idle,
    ScanningPrefix,
    CollectingSite(GenerationMode, usize), // Mode and prefix length
}

pub(crate) struct TriggerDetector {
    state: DetectorState,
    buffer: String,
    triggers: Vec<(String, GenerationMode)>,
    injection_active: Arc<AtomicBool>,
}

impl TriggerDetector {
    pub fn new(triggers: Vec<(String, GenerationMode)>, injection_active: Arc<AtomicBool>) -> Self {
        Self {
            state: DetectorState::Idle,
            buffer: String::new(),
            triggers,
            injection_active,
        }
    }

    pub fn process_event(&mut self, event: &Event) -> Option<TriggerEvent> {
        if self.injection_active.load(Ordering::SeqCst) {
            log::debug!("[SKIP] Injection active, ignoring event");
            return None;
        }

        match &event.event_type {
            EventType::KeyPress(key) => {
                log::debug!(
                    "[KEY] Press: {:?} | name: {:?} | state: {:?} | buffer: \"{}\"",
                    key,
                    event.name,
                    self.state,
                    self.buffer
                );
                self.handle_key_press(*key, event)
            }
            EventType::KeyRelease(key) => {
                log::debug!("[KEY] Release: {:?}", key);
                None
            }
            _ => None,
        }
    }

    fn handle_key_press(&mut self, key: Key, event: &Event) -> Option<TriggerEvent> {
        if key == Key::Backspace {
            self.handle_backspace();
            log::debug!("[BACKSPACE] buffer now: \"{}\"", self.buffer);
            return None;
        }

        if is_terminator(key) {
            log::debug!("[TERMINATOR] {:?} pressed, checking trigger...", key);
            return self.handle_terminator();
        }

        let ch = match &event.name {
            Some(name) if !name.is_empty() => {
                let c = name.chars().next();
                log::debug!("[CHAR] from event.name: {:?}", c);
                c?
            }
            _ => {
                let c = key_to_char(key);
                log::debug!("[CHAR] from key_to_char: {:?}", c);
                c?
            }
        };

        self.process_char(ch)
    }

    fn process_char(&mut self, ch: char) -> Option<TriggerEvent> {
        match self.state {
            DetectorState::Idle => {
                self.buffer.clear();
                self.buffer.push(ch);
                if self.check_prefixes() {
                    self.state = DetectorState::ScanningPrefix;
                    log::debug!(
                        "[STATE] Idle -> ScanningPrefix | buffer: \"{}\"",
                        self.buffer
                    );
                } else {
                    // Not a start of any prefix
                    self.buffer.clear();
                }
                None
            }
            DetectorState::ScanningPrefix => {
                self.buffer.push(ch);

                // Check if we matched a full prefix
                if let Some((mode, len)) = self.check_full_match() {
                    self.state = DetectorState::CollectingSite(mode, len);
                    log::debug!(
                        "[STATE] ScanningPrefix -> CollectingSite({:?}) | buffer: \"{}\"",
                        mode,
                        self.buffer
                    );
                    return None;
                }

                // Check if we are still matching a prefix
                if self.check_prefixes() {
                    log::debug!(
                        "[STATE] ScanningPrefix continue | buffer: \"{}\"",
                        self.buffer
                    );
                } else {
                    log::debug!("[STATE] ScanningPrefix -> Idle (mismatch) | resetting");
                    self.reset();
                }
                None
            }
            DetectorState::CollectingSite(_mode, _) => {
                if is_valid_site_char(ch) {
                    self.buffer.push(ch);
                    log::debug!("[COLLECT] buffer: \"{}\"", self.buffer);
                } else {
                    log::debug!(
                        "[STATE] CollectingSite -> Idle (invalid char: '{}') | resetting",
                        ch
                    );
                    self.reset();
                }
                None
            }
        }
    }

    fn check_prefixes(&self) -> bool {
        self.triggers
            .iter()
            .any(|(prefix, _)| prefix.starts_with(&self.buffer))
    }

    fn check_full_match(&self) -> Option<(GenerationMode, usize)> {
        self.triggers.iter().find_map(|(prefix, mode)| {
            if self.buffer == *prefix {
                Some((*mode, prefix.len()))
            } else {
                None
            }
        })
    }

    fn handle_terminator(&mut self) -> Option<TriggerEvent> {
        if let DetectorState::CollectingSite(mode, prefix_len) = self.state {
            if self.buffer.len() > prefix_len {
                let site = self.buffer[prefix_len..].to_string();
                let trigger_len = self.buffer.chars().count() + 1;
                log::info!(
                    "[TRIGGER] site: \"{}\" | len: {} | mode: {:?} | buffer: \"{}\"",
                    site,
                    trigger_len,
                    mode,
                    self.buffer
                );
                self.reset();
                return Some(TriggerEvent {
                    site,
                    trigger_len,
                    mode,
                });
            }
        }
        log::debug!(
            "[TERMINATOR] No trigger (state: {:?}, buffer: \"{}\")",
            self.state,
            self.buffer
        );
        self.reset();
        None
    }

    fn reset(&mut self) {
        self.state = DetectorState::Idle;
        self.buffer.clear();
    }

    fn handle_backspace(&mut self) {
        if !self.buffer.is_empty() && self.state != DetectorState::Idle {
            self.buffer.pop();

            // Re-evaluate state based on new buffer content
            if self.buffer.is_empty() {
                self.reset();
                return;
            }

            // Check if we are still collecting site or moved back to prefix scanning
            if let DetectorState::CollectingSite(_, prefix_len) = self.state {
                if self.buffer.len() < prefix_len {
                    // We backspaced into the prefix.
                    // For simplicity, just reset to avoid complex state transitions backwards.
                    self.reset();
                }
            } else if self.state == DetectorState::ScanningPrefix && !self.check_prefixes() {
                self.reset();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdev::EventType;

    #[test]
    fn test_multiple_prefixes() {
        let triggers = vec![
            (";;".to_string(), GenerationMode::Argon2id),
            ("!!".to_string(), GenerationMode::Concatenation),
        ];
        let injection = Arc::new(AtomicBool::new(false));
        let mut detector = TriggerDetector::new(triggers, injection);

        let events = vec![
            EventType::KeyPress(Key::SemiColon),
            EventType::KeyPress(Key::SemiColon),
            EventType::KeyPress(Key::KeyS),
            EventType::KeyPress(Key::KeyI),
            EventType::KeyPress(Key::KeyT),
            EventType::KeyPress(Key::KeyE),
            EventType::KeyPress(Key::Space),
        ];

        let mut found_trigger = None;
        for evt_type in events {
            let name = match evt_type {
                EventType::KeyPress(Key::SemiColon) => Some(";".to_string()),
                EventType::KeyPress(Key::KeyS) => Some("s".to_string()),
                EventType::KeyPress(Key::KeyI) => Some("i".to_string()),
                EventType::KeyPress(Key::KeyT) => Some("t".to_string()),
                EventType::KeyPress(Key::KeyE) => Some("e".to_string()),
                EventType::KeyPress(Key::Space) => Some(" ".to_string()),
                _ => None,
            };

            let event = Event {
                time: std::time::SystemTime::now(),
                name,
                event_type: evt_type,
            };

            if let Some(t) = detector.process_event(&event) {
                found_trigger = Some(t);
            }
        }

        let t = found_trigger.expect("Should have detected trigger");
        assert_eq!(t.site, "site");
        assert_eq!(t.mode, GenerationMode::Argon2id);

        let events = vec![
            EventType::KeyPress(Key::Num1),
            EventType::KeyPress(Key::Num1),
            EventType::KeyPress(Key::KeyA),
            EventType::KeyPress(Key::Space),
        ];

        let mut found_trigger = None;
        for evt_type in events {
            let name = match evt_type {
                EventType::KeyPress(Key::Num1) => Some("!".to_string()),
                EventType::KeyPress(Key::KeyA) => Some("a".to_string()),
                EventType::KeyPress(Key::Space) => Some(" ".to_string()),
                _ => None,
            };

            let event = Event {
                time: std::time::SystemTime::now(),
                name,
                event_type: evt_type,
            };

            if let Some(t) = detector.process_event(&event) {
                found_trigger = Some(t);
            }
        }

        let t = found_trigger.expect("Should have detected concat trigger");
        assert_eq!(t.site, "a");
        assert_eq!(t.mode, GenerationMode::Concatenation);
    }
}

fn is_valid_site_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' || ch == '!' || ch == '@'
}

fn key_to_char(key: Key) -> Option<char> {
    match key {
        Key::KeyA => Some('a'),
        Key::KeyB => Some('b'),
        Key::KeyC => Some('c'),
        Key::KeyD => Some('d'),
        Key::KeyE => Some('e'),
        Key::KeyF => Some('f'),
        Key::KeyG => Some('g'),
        Key::KeyH => Some('h'),
        Key::KeyI => Some('i'),
        Key::KeyJ => Some('j'),
        Key::KeyK => Some('k'),
        Key::KeyL => Some('l'),
        Key::KeyM => Some('m'),
        Key::KeyN => Some('n'),
        Key::KeyO => Some('o'),
        Key::KeyP => Some('p'),
        Key::KeyQ => Some('q'),
        Key::KeyR => Some('r'),
        Key::KeyS => Some('s'),
        Key::KeyT => Some('t'),
        Key::KeyU => Some('u'),
        Key::KeyV => Some('v'),
        Key::KeyW => Some('w'),
        Key::KeyX => Some('x'),
        Key::KeyY => Some('y'),
        Key::KeyZ => Some('z'),
        Key::Num0 => Some('0'),
        Key::Num1 => Some('1'),
        Key::Num2 => Some('2'),
        Key::Num3 => Some('3'),
        Key::Num4 => Some('4'),
        Key::Num5 => Some('5'),
        Key::Num6 => Some('6'),
        Key::Num7 => Some('7'),
        Key::Num8 => Some('8'),
        Key::Num9 => Some('9'),
        Key::Dot => Some('.'),
        Key::Minus => Some('-'),
        Key::SemiColon => Some(';'),
        Key::Equal => Some('='),
        Key::Comma => Some(','),
        Key::Slash => Some('/'),
        Key::BackSlash => Some('\\'),
        Key::LeftBracket => Some('['),
        Key::RightBracket => Some(']'),
        Key::Quote => Some('\''),
        Key::BackQuote => Some('`'),
        _ => None,
    }
}

fn is_terminator(key: Key) -> bool {
    matches!(key, Key::Space | Key::Return | Key::Tab)
}

pub fn start_keyboard_listener(
    tx: Sender<TriggerEvent>,
    triggers: Vec<(String, GenerationMode)>,
    injection_active: Arc<AtomicBool>,
) -> Result<std::thread::JoinHandle<()>> {
    platform::start_keyboard_listener(tx, triggers, injection_active)
}
