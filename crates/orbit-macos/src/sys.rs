//! The C surface of macOS this driver uses, declared by hand, and the few safe wrappers over it.
//!
//! Every function here is a stable, documented C entry point. Ownership follows Core Foundation's
//! rule: anything from a `Create` or `Copy` function is ours to release, and lives in a [`Cf`],
//! which releases it on drop; anything from a `Get` function is borrowed and is never wrapped.
#![allow(non_upper_case_globals, non_snake_case, clippy::upper_case_acronyms)]

use std::ffi::{c_char, c_void};

pub type CFTypeRef = *const c_void;
pub type CFIndex = isize;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

pub const UTF8: u32 = 0x0800_0100;
pub const kCFNumberSInt64Type: CFIndex = 4;
pub const kAXValueCGPointType: u32 = 1;
pub const kAXValueCGSizeType: u32 = 2;
pub const kAXValueAXErrorType: u32 = 5;
/// RGBA, alpha last and premultiplied, bytes in that order in memory.
pub const RGBA_BITMAP_INFO: u32 = 1 | (4 << 12);
pub const kCGWindowListOptionOnScreenOnly: u32 = 1;
pub const kCGWindowListExcludeDesktopElements: u32 = 1 << 4;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub static kCFBooleanTrue: CFTypeRef;
    pub static kCFTypeDictionaryKeyCallBacks: c_void;
    pub static kCFTypeDictionaryValueCallBacks: c_void;
    pub static kCFTypeArrayCallBacks: c_void;
    pub fn CFArrayCreate(a: CFTypeRef, values: *const CFTypeRef, count: CFIndex, callbacks: *const c_void) -> CFTypeRef;
    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> u8;
    pub fn CFGetTypeID(cf: CFTypeRef) -> usize;
    pub fn CFStringGetTypeID() -> usize;
    pub fn CFArrayGetTypeID() -> usize;
    pub fn CFBooleanGetTypeID() -> usize;
    pub fn CFNumberGetTypeID() -> usize;
    pub fn CFStringCreateWithBytes(a: CFTypeRef, bytes: *const u8, len: CFIndex, enc: u32, external: u8) -> CFTypeRef;
    pub fn CFStringGetLength(s: CFTypeRef) -> CFIndex;
    pub fn CFStringGetMaximumSizeForEncoding(len: CFIndex, enc: u32) -> CFIndex;
    pub fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: CFIndex, enc: u32) -> u8;
    pub fn CFArrayGetCount(a: CFTypeRef) -> CFIndex;
    pub fn CFArrayGetValueAtIndex(a: CFTypeRef, i: CFIndex) -> CFTypeRef;
    pub fn CFBooleanGetValue(b: CFTypeRef) -> u8;
    pub fn CFNumberGetValue(n: CFTypeRef, kind: CFIndex, out: *mut c_void) -> u8;
    pub fn CFDictionaryGetValue(d: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    pub fn CFDictionaryCreate(
        a: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: CFIndex,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFTypeRef;
    pub fn CFURLCreateWithBytes(a: CFTypeRef, bytes: *const u8, len: CFIndex, enc: u32, base: CFTypeRef) -> CFTypeRef;
    pub fn CFURLCreateFromFileSystemRepresentation(a: CFTypeRef, buf: *const u8, len: CFIndex, dir: u8) -> CFTypeRef;
    pub fn CFBundleCreate(a: CFTypeRef, url: CFTypeRef) -> CFTypeRef;
    pub fn CFBundleGetIdentifier(b: CFTypeRef) -> CFTypeRef;
    pub static kCFRunLoopCommonModes: CFTypeRef;
    pub fn CFMachPortCreateRunLoopSource(a: CFTypeRef, port: CFTypeRef, order: CFIndex) -> CFTypeRef;
    pub fn CFRunLoopGetCurrent() -> CFTypeRef;
    pub fn CFRunLoopAddSource(rl: CFTypeRef, source: CFTypeRef, mode: CFTypeRef);
    pub fn CFRunLoopRun();
    pub fn CFDataCreateMutable(a: CFTypeRef, capacity: CFIndex) -> CFTypeRef;
    pub fn CFDataGetLength(d: CFTypeRef) -> CFIndex;
    pub fn CFDataGetBytePtr(d: CFTypeRef) -> *const u8;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub static kCGWindowOwnerPID: CFTypeRef;
    pub static kCGWindowBounds: CFTypeRef;
    pub static kCGWindowLayer: CFTypeRef;
    pub fn CGPreflightScreenCaptureAccess() -> bool;
    pub fn CGRequestScreenCaptureAccess() -> bool;
    pub fn CGPreflightListenEventAccess() -> bool;
    pub fn CGRequestListenEventAccess() -> bool;
    pub fn CGMainDisplayID() -> u32;
    pub fn CGDisplayBounds(display: u32) -> CGRect;
    pub fn CGDisplayCreateImage(display: u32) -> CFTypeRef;
    pub fn CGImageGetWidth(img: CFTypeRef) -> usize;
    pub fn CGImageGetHeight(img: CFTypeRef) -> usize;
    pub fn CGColorSpaceCreateDeviceRGB() -> CFTypeRef;
    pub fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: CFTypeRef,
        info: u32,
    ) -> CFTypeRef;
    pub fn CGBitmapContextCreateImage(ctx: CFTypeRef) -> CFTypeRef;
    pub fn CGContextDrawImage(ctx: CFTypeRef, rect: CGRect, img: CFTypeRef);
    pub fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFTypeRef;
    pub fn CGRectMakeWithDictionaryRepresentation(dict: CFTypeRef, rect: *mut CGRect) -> bool;
    pub fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events: u64,
        callback: EventTapCallback,
        user: *mut c_void,
    ) -> CFTypeRef;
    pub fn CGEventTapEnable(tap: CFTypeRef, enable: bool);
    pub fn CGEventGetLocation(event: CFTypeRef) -> CGPoint;
    pub fn CGEventGetIntegerValueField(event: CFTypeRef, field: u32) -> i64;
}

pub type EventTapCallback = extern "C" fn(proxy: *mut c_void, kind: u32, event: CFTypeRef, user: *mut c_void) -> CFTypeRef;

pub const kCGSessionEventTap: u32 = 1;
pub const kCGHeadInsertEventTap: u32 = 0;
pub const kCGEventTapOptionListenOnly: u32 = 1;
pub const kCGEventLeftMouseDown: u32 = 1;
pub const kCGEventRightMouseDown: u32 = 3;
pub const kCGEventMouseMoved: u32 = 5;
pub const kCGEventLeftMouseDragged: u32 = 6;
pub const kCGEventRightMouseDragged: u32 = 7;
pub const kCGEventKeyDown: u32 = 10;
pub const kCGEventOtherMouseDragged: u32 = 27;
pub const kCGEventTapDisabledByTimeout: u32 = 0xFFFF_FFFE;
pub const kCGEventTapDisabledByUserInput: u32 = 0xFFFF_FFFF;
pub const kCGKeyboardEventKeycode: u32 = 9;
pub const kCGEventSourceStateID: u32 = 45;
/// The source state of events that came from hardware.
pub const kCGEventSourceStateHIDSystemState: i64 = 1;
pub const KEY_ESCAPE: i64 = 53;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub static kAXTrustedCheckOptionPrompt: CFTypeRef;
    pub fn AXIsProcessTrusted() -> u8;
    pub fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
    pub fn AXUIElementCreateSystemWide() -> CFTypeRef;
    pub fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    pub fn AXUIElementCopyAttributeValue(el: CFTypeRef, attr: CFTypeRef, out: *mut CFTypeRef) -> i32;
    pub fn AXUIElementSetMessagingTimeout(el: CFTypeRef, seconds: f32) -> i32;
    pub fn AXUIElementSetAttributeValue(el: CFTypeRef, attr: CFTypeRef, value: CFTypeRef) -> i32;
    pub fn AXUIElementGetPid(el: CFTypeRef, pid: *mut i32) -> i32;
    pub fn AXValueGetValue(v: CFTypeRef, kind: u32, out: *mut c_void) -> u8;
    pub fn AXValueGetTypeID() -> usize;
    pub fn AXValueGetType(v: CFTypeRef) -> u32;
    pub fn AXUIElementCopyMultipleAttributeValues(el: CFTypeRef, attrs: CFTypeRef, options: u32, out: *mut CFTypeRef) -> i32;
}

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    pub fn LSOpenCFURLRef(url: CFTypeRef, launched: *mut CFTypeRef) -> i32;
}

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    pub fn CGImageDestinationCreateWithData(data: CFTypeRef, kind: CFTypeRef, count: usize, options: CFTypeRef) -> CFTypeRef;
    pub fn CGImageDestinationCreateWithURL(url: CFTypeRef, kind: CFTypeRef, count: usize, options: CFTypeRef) -> CFTypeRef;
    pub fn CGImageDestinationAddImage(dest: CFTypeRef, img: CFTypeRef, props: CFTypeRef);
    pub fn CGImageDestinationFinalize(dest: CFTypeRef) -> bool;
}

unsafe extern "C" {
    /// libproc, in libSystem.
    pub fn proc_pidpath(pid: i32, buf: *mut c_void, size: u32) -> i32;
}

/// An owned Core Foundation object (Create/Copy rule). Released on drop.
pub struct Cf(CFTypeRef);

impl Cf {
    /// `None` for null, so a failed Create is an `Option` rather than a pointer to check later.
    ///
    /// # Safety
    /// `ptr` must be null or a +1 reference the caller owns.
    pub unsafe fn own(ptr: CFTypeRef) -> Option<Cf> {
        (!ptr.is_null()).then_some(Cf(ptr))
    }
    pub fn ptr(&self) -> CFTypeRef {
        self.0
    }
}

impl Drop for Cf {
    fn drop(&mut self) {
        // SAFETY: constructed only from an owned, non-null reference.
        unsafe { CFRelease(self.0) }
    }
}

/// A CFString for `s`.
pub fn cfstr(s: &str) -> Cf {
    // SAFETY: bytes and length come from a live &str; the result is a Create.
    unsafe { Cf::own(CFStringCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as CFIndex, UTF8, 0)) }
        .expect("CFString from UTF-8")
}

/// A CFURL for a string URL, or a file path when `file` is set.
pub fn cfurl(s: &str, file: bool) -> Option<Cf> {
    // SAFETY: as above.
    unsafe {
        Cf::own(if file {
            CFURLCreateFromFileSystemRepresentation(std::ptr::null(), s.as_ptr(), s.len() as CFIndex, 0)
        } else {
            CFURLCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as CFIndex, UTF8, std::ptr::null())
        })
    }
}

/// A borrowed CF value as a Rust string, if it is a CFString.
///
/// # Safety
/// `v` must be null or a live CF object.
pub unsafe fn string(v: CFTypeRef) -> Option<String> {
    unsafe {
        if v.is_null() || CFGetTypeID(v) != CFStringGetTypeID() {
            return None;
        }
        let size = CFStringGetMaximumSizeForEncoding(CFStringGetLength(v), UTF8) + 1;
        let mut buf = vec![0u8; size as usize];
        if CFStringGetCString(v, buf.as_mut_ptr().cast(), size, UTF8) == 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        buf.truncate(end);
        String::from_utf8(buf).ok()
    }
}

/// The bundle identifier of the app a process belongs to, or its executable's name if it is not
/// in a bundle. The same naming `Capture::app` and `Window::app` use, so the two compare.
pub fn app_id(pid: i32) -> Option<String> {
    let mut buf = vec![0u8; 4096];
    // SAFETY: the buffer is 4096 bytes, which is PROC_PIDPATHINFO_MAXSIZE.
    let n = unsafe { proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if n <= 0 {
        return None;
    }
    let path = std::path::PathBuf::from(String::from_utf8_lossy(&buf[..n as usize]).into_owned());
    let bundle_id = path
        .ancestors()
        .find(|a| a.extension().is_some_and(|e| e == "app"))
        .and_then(|app| {
            let url = cfurl(app.to_str()?, true)?;
            // SAFETY: CFBundleCreate is a Create; GetIdentifier is a Get on it, read before drop.
            unsafe {
                let bundle = Cf::own(CFBundleCreate(std::ptr::null(), url.ptr()))?;
                string(CFBundleGetIdentifier(bundle.ptr()))
            }
        });
    bundle_id.or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
}
