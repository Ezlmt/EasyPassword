//! Small OS-specific helpers.

#[cfg(target_os = "macos")]
pub mod macos {
    use std::ffi::c_void;

    use core_foundation::base::{CFRelease, TCFType};
    use core_foundation::boolean::{CFBoolean, CFBooleanRef};
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::string::{CFString, CFStringRef};

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    }

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn TISCopyCurrentKeyboardInputSource() -> *mut c_void;
        fn TISGetInputSourceProperty(source: *mut c_void, key: CFStringRef) -> *const c_void;
        static kTISPropertyInputSourceIsASCIICapable: CFStringRef;
    }

    /// Whether we may post synthetic key events (Accessibility permission).
    /// With `prompt`, macOS shows its "open System Settings" dialog if not.
    pub fn accessibility_trusted(prompt: bool) -> bool {
        let key = CFString::from_static_string("AXTrustedCheckOptionPrompt");
        let value = if prompt {
            CFBoolean::true_value()
        } else {
            CFBoolean::false_value()
        };
        let options = CFDictionary::from_CFType_pairs(&[(key, value)]);
        unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
    }

    /// False when a CJK input method (Pinyin, Rime, ...) is active. Typing
    /// via synthetic key events can be swallowed by such IMEs, so we paste
    /// instead. Must be called on the main thread.
    pub fn input_source_is_ascii() -> bool {
        unsafe {
            let source = TISCopyCurrentKeyboardInputSource();
            if source.is_null() {
                return true;
            }
            let value = TISGetInputSourceProperty(source, kTISPropertyInputSourceIsASCIICapable);
            let ascii = if value.is_null() {
                true
            } else {
                CFBoolean::wrap_under_get_rule(value as CFBooleanRef).into()
            };
            CFRelease(source as *const c_void);
            ascii
        }
    }
}

#[cfg(target_os = "windows")]
pub mod windows {
    use std::time::{Duration, Instant};

    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

    pub fn foreground_window() -> isize {
        unsafe { GetForegroundWindow() as isize }
    }

    pub fn activate_window(hwnd: isize) {
        if hwnd != 0 {
            unsafe {
                SetForegroundWindow(hwnd as _);
            }
        }
    }

    /// Waits (up to 1 s) until Ctrl/Shift/Alt/Win are released, so they don't
    /// combine with the characters we are about to type.
    pub fn wait_for_modifiers_released() {
        let deadline = Instant::now() + Duration::from_secs(1);
        let held = || unsafe {
            [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
                .iter()
                .any(|&vk| (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0)
        };
        while held() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(15));
        }
    }
}
