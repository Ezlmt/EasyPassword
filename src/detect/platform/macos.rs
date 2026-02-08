use crossbeam_channel::Sender;
use std::cell::RefCell;
use std::os::raw::c_ulong;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::mach_port::CFMachPortRef;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, EventField,
};
use foreign_types::ForeignType;

use crate::core::GenerationMode;
use crate::error::Result;

use super::super::{TriggerDetector, TriggerEvent};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventKeyboardGetUnicodeString(
        event: *const std::ffi::c_void,
        max_len: c_ulong,
        actual_len: *mut c_ulong,
        buf: *mut u16,
    );
}

fn get_unicode_string(event: &CGEvent) -> Option<String> {
    let mut buf = [0u16; 4];
    let mut actual: c_ulong = 0;
    unsafe {
        let event_ref = event.as_ptr() as *const std::ffi::c_void;
        CGEventKeyboardGetUnicodeString(
            event_ref,
            buf.len() as c_ulong,
            &mut actual,
            buf.as_mut_ptr(),
        );
    }
    if actual == 0 || actual as usize > buf.len() {
        return None;
    }
    String::from_utf16(&buf[..actual as usize])
        .ok()
        .filter(|s| !s.is_empty())
}

fn keycode_to_key(keycode: u16) -> Option<rdev::Key> {
    match keycode {
        0 => Some(rdev::Key::KeyA),
        1 => Some(rdev::Key::KeyS),
        2 => Some(rdev::Key::KeyD),
        3 => Some(rdev::Key::KeyF),
        4 => Some(rdev::Key::KeyH),
        5 => Some(rdev::Key::KeyG),
        6 => Some(rdev::Key::KeyZ),
        7 => Some(rdev::Key::KeyX),
        8 => Some(rdev::Key::KeyC),
        9 => Some(rdev::Key::KeyV),
        11 => Some(rdev::Key::KeyB),
        12 => Some(rdev::Key::KeyQ),
        13 => Some(rdev::Key::KeyW),
        14 => Some(rdev::Key::KeyE),
        15 => Some(rdev::Key::KeyR),
        16 => Some(rdev::Key::KeyY),
        17 => Some(rdev::Key::KeyT),
        31 => Some(rdev::Key::KeyO),
        32 => Some(rdev::Key::KeyU),
        34 => Some(rdev::Key::KeyI),
        35 => Some(rdev::Key::KeyP),
        37 => Some(rdev::Key::KeyL),
        38 => Some(rdev::Key::KeyJ),
        40 => Some(rdev::Key::KeyK),
        45 => Some(rdev::Key::KeyN),
        46 => Some(rdev::Key::KeyM),

        18 => Some(rdev::Key::Num1),
        19 => Some(rdev::Key::Num2),
        20 => Some(rdev::Key::Num3),
        21 => Some(rdev::Key::Num4),
        22 => Some(rdev::Key::Num6),
        23 => Some(rdev::Key::Num5),
        24 => Some(rdev::Key::Equal),
        25 => Some(rdev::Key::Num9),
        26 => Some(rdev::Key::Num7),
        27 => Some(rdev::Key::Minus),
        28 => Some(rdev::Key::Num8),
        29 => Some(rdev::Key::Num0),
        30 => Some(rdev::Key::RightBracket),
        33 => Some(rdev::Key::LeftBracket),
        39 => Some(rdev::Key::Quote),
        41 => Some(rdev::Key::SemiColon),
        42 => Some(rdev::Key::BackSlash),
        43 => Some(rdev::Key::Comma),
        44 => Some(rdev::Key::Slash),
        47 => Some(rdev::Key::Dot),
        50 => Some(rdev::Key::BackQuote),

        36 => Some(rdev::Key::Return),
        48 => Some(rdev::Key::Tab),
        49 => Some(rdev::Key::Space),
        51 => Some(rdev::Key::Backspace),

        _ => None,
    }
}

pub fn start_keyboard_listener(
    tx: Sender<TriggerEvent>,
    triggers: Vec<(String, GenerationMode)>,
    injection_active: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>> {
    let handle = thread::spawn(move || {
        let detector = RefCell::new(TriggerDetector::new(
            triggers.clone(),
            injection_active.clone(),
        ));

        log::info!(
            "[LISTENER-MACOS] Keyboard listener started, triggers: {:?}",
            triggers
        );

        let tx_clone = tx.clone();
        let injection_clone = injection_active.clone();

        let tap_port: Arc<std::sync::atomic::AtomicPtr<std::ffi::c_void>> =
            Arc::new(std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()));
        let tap_port_for_cb = tap_port.clone();

        let tap = CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            vec![CGEventType::KeyDown],
            move |_proxy, event_type, event: &CGEvent| {
                if matches!(
                    event_type,
                    CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
                ) {
                    let port = tap_port_for_cb.load(Ordering::SeqCst) as CFMachPortRef;
                    if !port.is_null() {
                        unsafe { CGEventTapEnable(port, true) };
                        log::warn!(
                            "[LISTENER-MACOS] CGEventTap disabled ({:?}); re-enabled",
                            event_type
                        );
                    } else {
                        log::warn!(
                            "[LISTENER-MACOS] CGEventTap disabled ({:?}); no port to re-enable",
                            event_type
                        );
                    }
                    return None;
                }

                if injection_clone.load(Ordering::SeqCst) {
                    return None;
                }

                let is_autorepeat =
                    event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0;
                if is_autorepeat {
                    return None;
                }

                let keycode =
                    event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
                let flags = event.get_flags();

                if flags.contains(CGEventFlags::CGEventFlagCommand)
                    || flags.contains(CGEventFlags::CGEventFlagControl)
                    || flags.contains(CGEventFlags::CGEventFlagAlternate)
                {
                    return None;
                }

                let name = get_unicode_string(event);
                let mapped_key = keycode_to_key(keycode);

                if let Some(key) = mapped_key {
                    let rdev_event = rdev::Event {
                        time: std::time::SystemTime::now(),
                        name,
                        event_type: rdev::EventType::KeyPress(key),
                    };

                    if let Some(trigger) = detector.borrow_mut().process_event(&rdev_event) {
                        if let Err(e) = tx_clone.send(trigger) {
                            log::error!("[LISTENER-MACOS] Failed to send trigger: {}", e);
                        }
                    }
                }

                None
            },
        );

        let tap = match tap {
            Ok(tap) => tap,
            Err(()) => {
                log::error!("[LISTENER-MACOS] Failed to create CGEventTap. Make sure the app has Input Monitoring permissions in System Settings > Privacy & Security > Input Monitoring.");
                loop {
                    thread::sleep(Duration::from_secs(5));
                    log::warn!("[LISTENER-MACOS] Waiting for Input Monitoring permission...");
                }
            }
        };

        unsafe {
            tap_port.store(
                tap.mach_port.as_concrete_TypeRef() as *mut _,
                Ordering::SeqCst,
            );

            let loop_source = match tap.mach_port.create_runloop_source(0) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("[LISTENER-MACOS] Failed to create runloop source: {:?}", e);
                    return;
                }
            };
            let run_loop = CFRunLoop::get_current();
            run_loop.add_source(&loop_source, kCFRunLoopCommonModes);
            tap.enable();
            log::info!("[LISTENER-MACOS] CGEventTap enabled, starting run loop...");
            CFRunLoop::run_current();
        }

        log::info!("[LISTENER-MACOS] Run loop exited");
    });

    Ok(handle)
}
