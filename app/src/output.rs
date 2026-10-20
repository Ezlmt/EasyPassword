//! Delivering the password: type it, paste it, or copy it.

use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use zeroize::Zeroizing;

use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Type,
    Paste,
    Copy,
}

/// Time for the OS to hand focus back to the target window after the popup
/// hides, before we start sending keystrokes.
const FOCUS_SETTLE: Duration = Duration::from_millis(250);
/// After Ctrl/Cmd+V, how long to wait before restoring the old clipboard.
const PASTE_RESTORE_DELAY: Duration = Duration::from_millis(600);

fn digest(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

fn enigo() -> Result<Enigo, String> {
    let settings = EnigoSettings {
        // We check / prompt for the permission ourselves.
        open_prompt_to_get_permissions: false,
        ..EnigoSettings::default()
    };
    Enigo::new(&settings).map_err(|e| format!("无法模拟键盘输入: {e}"))
}

/// Writes `text` to the clipboard, asking clipboard managers / cloud sync
/// not to record it.
fn set_secret_text(clipboard: &mut Clipboard, text: &str) -> Result<(), String> {
    let set = clipboard.set();
    #[cfg(target_os = "macos")]
    let set = {
        use arboard::SetExtApple;
        set.exclude_from_history()
    };
    #[cfg(target_os = "windows")]
    let set = {
        use arboard::SetExtWindows;
        set.exclude_from_history().exclude_from_cloud()
    };
    set.text(text).map_err(|e| format!("写入剪贴板失败: {e}"))
}

/// Copies the password and clears the clipboard after `clear_after_secs`
/// (0 = never), unless something else was copied in the meantime.
pub fn copy(app: &AppHandle, password: &str, clear_after_secs: u32) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("无法打开剪贴板: {e}"))?;
    set_secret_text(&mut clipboard, password)?;

    let state = app.state::<AppState>();
    let generation = state.clipboard_generation.fetch_add(1, Ordering::SeqCst) + 1;
    if clear_after_secs > 0 {
        let expected = digest(password);
        let app = app.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(u64::from(clear_after_secs)));
            let state = app.state::<AppState>();
            if state.clipboard_generation.load(Ordering::SeqCst) == generation {
                clear_if_unchanged(&expected, None);
            }
        });
    }
    Ok(())
}

/// Clears (or restores `previous` into) the clipboard if it still holds the
/// text whose digest is `expected`.
fn clear_if_unchanged(expected: &[u8; 32], previous: Option<Zeroizing<String>>) {
    let Ok(mut clipboard) = Clipboard::new() else {
        return;
    };
    let still_ours = clipboard
        .get_text()
        .map(|t| digest(&Zeroizing::new(t)) == *expected)
        .unwrap_or(false);
    if !still_ours {
        return;
    }
    let result = match previous {
        Some(prev) => clipboard.set_text(prev.as_str()),
        None => clipboard.clear(),
    };
    if let Err(e) = result {
        log::warn!("failed to clear clipboard: {e}");
    }
}

/// Hides the popup, waits for focus to return to the target app, then types
/// or pastes on the main thread (macOS keyboard APIs require it).
pub fn deliver_to_previous_window(app: &AppHandle, password: Zeroizing<String>, action: Action) {
    crate::popup::hide_and_restore_focus(app);
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(FOCUS_SETTLE);
        #[cfg(target_os = "windows")]
        crate::platform::windows::wait_for_modifiers_released();

        let main = app.clone();
        let result = main.run_on_main_thread(move || {
            let action = effective_action(action);
            let result = match action {
                Action::Type => type_text(&password),
                Action::Paste => paste(&password),
                Action::Copy => Ok(()),
            };
            if let Err(e) = result {
                log::error!("delivering password failed: {e}");
                crate::popup::show_error(&app, &e);
            }
        });
        if let Err(e) = result {
            log::error!("cannot dispatch to main thread: {e}");
        }
    });
}

/// On macOS, CJK input methods may swallow synthetic keystrokes: paste instead.
fn effective_action(action: Action) -> Action {
    #[cfg(target_os = "macos")]
    if action == Action::Type && !crate::platform::macos::input_source_is_ascii() {
        log::info!("non-ASCII input source active; pasting instead of typing");
        return Action::Paste;
    }
    action
}

fn type_text(password: &str) -> Result<(), String> {
    enigo()?
        .text(password)
        .map_err(|e| format!("模拟输入失败: {e}"))
}

/// Puts the password on the clipboard, sends Cmd/Ctrl+V, then restores the
/// previous clipboard text (or clears it).
fn paste(password: &str) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("无法打开剪贴板: {e}"))?;
    let previous = clipboard.get_text().ok().map(Zeroizing::new);
    set_secret_text(&mut clipboard, password)?;

    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    let mut enigo = enigo()?;
    let send = |enigo: &mut Enigo| -> enigo::InputResult<()> {
        enigo.key(modifier, Direction::Press)?;
        let r = enigo.key(Key::Unicode('v'), Direction::Click);
        enigo.key(modifier, Direction::Release)?;
        r
    };
    let result = send(&mut enigo).map_err(|e| format!("模拟粘贴失败: {e}"));

    let expected = digest(password);
    thread::spawn(move || {
        thread::sleep(PASTE_RESTORE_DELAY);
        clear_if_unchanged(&expected, previous);
    });
    result
}
