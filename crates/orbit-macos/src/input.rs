//! The operator's hands: a listen-only event tap that turns hardware input into [`Human`] events.
//!
//! LISTEN-ONLY. The tap sees events and passes every one through untouched; it cannot delay, change
//! or swallow the operator's input, so a bug here cannot lock anyone out of their own machine.
//!
//! HUMAN ONLY. The kill switch must not trip on the agent's own synthesised input, or every drag the
//! agent made would stop it. Hardware events carry the HID system source state; anything the agent
//! posts will come from a private event source, and is dropped here before the guard sees it.
#![allow(non_upper_case_globals)] // Apple's constant names, matched as patterns.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use orbit_daemon::Human;

use crate::sys::*;

type Sink = Box<dyn Fn(i64, Human) + Send + Sync>;

/// One tap per process: the callback is a C function with nowhere to keep a closure but here.
static SINK: OnceLock<Sink> = OnceLock::new();
static TAP: OnceLock<usize> = OnceLock::new();

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

extern "C" fn on_event(_proxy: *mut std::ffi::c_void, kind: u32, event: CFTypeRef, _user: *mut std::ffi::c_void) -> CFTypeRef {
    // The system disables a tap that is slow or that the user interrupts; turn it straight back on,
    // because a kill switch that quietly stops listening is worse than none.
    if kind == kCGEventTapDisabledByTimeout || kind == kCGEventTapDisabledByUserInput {
        if let Some(&tap) = TAP.get() {
            // SAFETY: the tap lives for the process.
            unsafe { CGEventTapEnable(tap as CFTypeRef, true) };
        }
        return event;
    }
    let Some(sink) = SINK.get() else { return event };
    // SAFETY: `event` is the live CGEvent this callback was handed.
    unsafe {
        if CGEventGetIntegerValueField(event, kCGEventSourceStateID) != kCGEventSourceStateHIDSystemState {
            return event;
        }
        let human = match kind {
            kCGEventKeyDown if CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode) == KEY_ESCAPE => Some(Human::Escape),
            kCGEventMouseMoved
            | kCGEventLeftMouseDragged
            | kCGEventRightMouseDragged
            | kCGEventOtherMouseDragged
            | kCGEventLeftMouseDown
            | kCGEventRightMouseDown => {
                let p = CGEventGetLocation(event);
                Some(Human::MouseMoved { x: p.x, y: p.y })
            }
            _ => None,
        };
        if let Some(h) = human {
            sink(now_ms(), h);
        }
    }
    event
}

/// Start listening, on a thread of its own, for the life of the process. `Err` if the tap could not
/// be created — which means Input Monitoring is off, and the agent must not start.
pub fn listen(sink: impl Fn(i64, Human) + Send + Sync + 'static) -> Result<(), String> {
    SINK.set(Box::new(sink)).map_err(|_| "already listening")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("orbit-input".into())
        .spawn(move || {
            let mask = [
                kCGEventLeftMouseDown,
                kCGEventRightMouseDown,
                kCGEventMouseMoved,
                kCGEventLeftMouseDragged,
                kCGEventRightMouseDragged,
                kCGEventKeyDown,
                kCGEventOtherMouseDragged,
            ]
            .iter()
            .fold(0u64, |m, &k| m | (1 << k));
            // SAFETY: a listen-only session tap; the port and source live as long as this thread,
            // which runs its loop forever.
            unsafe {
                let tap = CGEventTapCreate(
                    kCGSessionEventTap,
                    kCGHeadInsertEventTap,
                    kCGEventTapOptionListenOnly,
                    mask,
                    on_event,
                    std::ptr::null_mut(),
                );
                if tap.is_null() {
                    let _ = tx.send(Err("could not listen to the keyboard and mouse: is Input Monitoring switched on?".to_string()));
                    return;
                }
                let _ = TAP.set(tap as usize);
                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
                CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
                CGEventTapEnable(tap, true);
                let _ = tx.send(Ok(()));
                CFRunLoopRun();
            }
        })
        .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}
