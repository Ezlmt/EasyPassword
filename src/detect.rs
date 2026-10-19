use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rdev::{Event, EventType, Key};

use crate::config::Terminator;
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
    terminators: Vec<Terminator>,
    injection_active: Arc<AtomicBool>,
}

impl TriggerDetector {
    pub fn new(
        triggers: Vec<(String, GenerationMode)>,
        terminators: Vec<Terminator>,
        injection_active: Arc<AtomicBool>,
    ) -> Self {
        Self {
            state: DetectorState::Idle,
            buffer: String::new(),
            triggers,
            terminators,
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
            EventType::ButtonPress(_) => {
                // A click usually moves the caret / focus: whatever was typed
                // so far is no longer guaranteed to sit right before the caret.
                self.reset();
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

        if is_modifier(key) {
            return None;
        }

        if self.is_terminator(key) {
            log::debug!("[TERMINATOR] {:?} pressed, checking trigger...", key);
            return self.handle_terminator();
        }

        let ch = match &event.name {
            Some(name) if !name.is_empty() => name.chars().next(),
            _ => key_to_char(key),
        }
        .filter(|c| is_text_char(*c));

        match ch {
            Some(ch) => self.process_char(ch),
            None => {
                // Navigation / editing keys (arrows, Esc, Delete, Home, Enter
                // when not a terminator, Ctrl+X ...) invalidate the buffer.
                log::debug!("[KEY] non-text key {:?}, resetting", key);
                self.reset();
                None
            }
        }
    }

    fn is_terminator(&self, key: Key) -> bool {
        self.terminators.iter().any(|t| match t {
            Terminator::Space => key == Key::Space,
            Terminator::Enter => matches!(key, Key::Return | Key::KpReturn),
            Terminator::Tab => key == Key::Tab,
        })
    }

    fn process_char(&mut self, ch: char) -> Option<TriggerEvent> {
        if let DetectorState::CollectingSite(..) = self.state {
            if is_valid_site_char(ch) {
                self.buffer.push(ch);
                log::debug!("[COLLECT] buffer: \"{}\"", self.buffer);
                return None;
            }
        }

        // Idle, scanning a prefix, or an invalid site char: append and look for
        // the longest suffix that can still start a trigger, so that e.g.
        // "!;;site" and ";;;site" are recognised.
        self.buffer.push(ch);
        self.rescan();
        log::debug!("[STATE] {:?} | buffer: \"{}\"", self.state, self.buffer);
        None
    }

    /// Shrinks the buffer to its longest suffix that is (a prefix of) a trigger
    /// prefix and updates the state accordingly.
    fn rescan(&mut self) {
        loop {
            if self.buffer.is_empty() {
                self.state = DetectorState::Idle;
                return;
            }
            if let Some((mode, len)) = self.check_full_match() {
                self.state = DetectorState::CollectingSite(mode, len);
                return;
            }
            if self.check_prefixes() {
                self.state = DetectorState::ScanningPrefix;
                return;
            }
            let mut chars = self.buffer.chars();
            chars.next();
            self.buffer = chars.as_str().to_string();
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

    pub(crate) fn reset(&mut self) {
        self.state = DetectorState::Idle;
        self.buffer.clear();
    }

    fn handle_backspace(&mut self) {
        if self.buffer.is_empty() || self.state == DetectorState::Idle {
            return;
        }
        self.buffer.pop();

        if let DetectorState::CollectingSite(_, prefix_len) = self.state {
            if self.buffer.len() >= prefix_len {
                return;
            }
        }
        // Backspaced into (or within) the prefix: recompute the state.
        self.rescan();
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
        let mut detector = TriggerDetector::new(triggers, vec![Terminator::Space], injection);

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

    fn detector(terminators: Vec<Terminator>) -> TriggerDetector {
        TriggerDetector::new(
            vec![
                (";;".to_string(), GenerationMode::Argon2id),
                ("!!".to_string(), GenerationMode::Concatenation),
            ],
            terminators,
            Arc::new(AtomicBool::new(false)),
        )
    }

    fn key_event(key: Key, name: Option<&str>) -> Event {
        Event {
            time: std::time::SystemTime::now(),
            name: name.map(str::to_string),
            event_type: EventType::KeyPress(key),
        }
    }

    /// Types `text`; `\x08` = Backspace, `\n` = Enter, `\t` = Tab,
    /// `\x1b` = Escape, `\x01` = mouse click.
    fn type_text(d: &mut TriggerDetector, text: &str) -> Option<TriggerEvent> {
        let mut found = None;
        for c in text.chars() {
            let event = match c {
                ' ' => key_event(Key::Space, Some(" ")),
                '\x08' => key_event(Key::Backspace, None),
                '\n' => key_event(Key::Return, Some("\r")),
                '\t' => key_event(Key::Tab, Some("\t")),
                '\x1b' => key_event(Key::Escape, None),
                '\x01' => Event {
                    time: std::time::SystemTime::now(),
                    name: None,
                    event_type: EventType::ButtonPress(rdev::Button::Left),
                },
                c => key_event(Key::Unknown(0), Some(&c.to_string())),
            };
            if let Some(t) = d.process_event(&event) {
                found = Some(t);
            }
        }
        found
    }

    #[test]
    fn trigger_len_counts_prefix_site_and_terminator() {
        let mut d = detector(vec![Terminator::Space]);
        let t = type_text(&mut d, "hello ;;github.com ").unwrap();
        assert_eq!(t.site, "github.com");
        assert_eq!(t.trigger_len, ";;github.com ".len());
    }

    #[test]
    fn prefix_recovers_after_partial_mismatch() {
        let mut d = detector(vec![Terminator::Space]);
        assert_eq!(type_text(&mut d, "!;;site ").unwrap().site, "site");
        assert_eq!(type_text(&mut d, ";;;site ").unwrap().site, "site");
        assert_eq!(type_text(&mut d, "a;;x;;site ").unwrap().site, "site");
    }

    #[test]
    fn enter_and_tab_are_not_terminators_by_default() {
        let mut d = detector(vec![Terminator::Space]);
        assert!(type_text(&mut d, ";;site\n").is_none());
        assert!(type_text(&mut d, ";;site\t").is_none());
        // ...and they invalidate the pending trigger.
        assert!(type_text(&mut d, ";;site\nx ").is_none());
    }

    #[test]
    fn enter_can_be_enabled() {
        let mut d = detector(vec![Terminator::Space, Terminator::Enter]);
        assert_eq!(type_text(&mut d, ";;site\n").unwrap().site, "site");
    }

    #[test]
    fn navigation_and_clicks_reset() {
        let mut d = detector(vec![Terminator::Space]);
        assert!(type_text(&mut d, ";;git\x1bhub ").is_none());
        assert!(type_text(&mut d, ";;git\x01hub ").is_none());
    }

    #[test]
    fn backspace_into_prefix_and_retype() {
        let mut d = detector(vec![Terminator::Space]);
        let t = type_text(&mut d, ";;ab\x08\x08\x08;site ").unwrap();
        assert_eq!(t.site, "site");
        let t = type_text(&mut d, ";;githuv\x08b ").unwrap();
        assert_eq!(t.site, "github");
    }

    #[test]
    fn modifiers_do_not_reset() {
        let mut d = detector(vec![Terminator::Space]);
        type_text(&mut d, ";;git");
        d.process_event(&key_event(Key::ShiftLeft, None));
        let t = type_text(&mut d, "Hub ").unwrap();
        assert_eq!(t.site, "gitHub");
    }
}

fn is_valid_site_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' || ch == '!' || ch == '@'
}

/// Printable characters only: excludes control chars (Enter, Tab, Esc, Ctrl+X)
/// and the macOS private-use range used for arrows / function keys.
fn is_text_char(ch: char) -> bool {
    !ch.is_control() && !('\u{F700}'..='\u{F8FF}').contains(&ch)
}

fn is_modifier(key: Key) -> bool {
    matches!(
        key,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::CapsLock
            | Key::ControlLeft
            | Key::ControlRight
            | Key::Alt
            | Key::AltGr
            | Key::MetaLeft
            | Key::MetaRight
            | Key::Function
    )
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

pub fn start_keyboard_listener(
    tx: Sender<TriggerEvent>,
    triggers: Vec<(String, GenerationMode)>,
    terminators: Vec<Terminator>,
    injection_active: Arc<AtomicBool>,
) -> Result<std::thread::JoinHandle<()>> {
    platform::start_keyboard_listener(tx, triggers, terminators, injection_active)
}
