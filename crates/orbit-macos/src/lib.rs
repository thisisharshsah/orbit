//! ORBIT ON macOS — the driver, starting where every operator starts: permissions.
//!
//! The system calls are declared here directly rather than through a binding crate. There are a
//! handful, they have been stable since macOS 10.15, and every line that talks to the OS should be
//! one a reviewer can read without opening a dependency.
//!
//! # Whose permission it is
//!
//! macOS grants these to an *app*, identified by its code signature — not to a process, and not to
//! a binary path. Run from a terminal, a bare executable is attributed to the terminal (or to VS
//! Code, for its built-in one), and the grant lands there. That is fine for development and wrong
//! for a product: the shipped daemon is a signed `.app` with a stable bundle identifier and team ID,
//! so a grant survives updates, and so a fleet can pre-approve it by MDM profile instead of five
//! hundred people clicking through System Settings.
#![cfg(target_os = "macos")]

use std::ffi::c_void;
use std::path::PathBuf;

use orbit_daemon::{Access, Asked, Permission, Status};

type CFTypeRef = *const c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
    static kAXTrustedCheckOptionPrompt: CFTypeRef;
}

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn LSOpenCFURLRef(url: CFTypeRef, launched: *mut CFTypeRef) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFTypeRef;
    fn CFURLCreateWithBytes(allocator: CFTypeRef, bytes: *const u8, len: isize, encoding: u32, base: CFTypeRef) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
}

const UTF8: u32 = 0x0800_0100;

/// The real thing.
pub struct MacAccess;

impl Access for MacAccess {
    fn status(&self, p: Permission) -> Status {
        // SAFETY: argument-free queries of the calling process's own TCC state.
        let granted = unsafe {
            match p {
                Permission::ScreenRecording => CGPreflightScreenCaptureAccess(),
                Permission::Accessibility => AXIsProcessTrusted() != 0,
                Permission::InputMonitoring => CGPreflightListenEventAccess(),
            }
        };
        if granted { Status::Granted } else { Status::Missing }
    }

    fn request(&self, p: Permission) {
        // SAFETY: the dictionary is built from CF constants, passed, and released; nothing escapes.
        unsafe {
            match p {
                Permission::ScreenRecording => {
                    CGRequestScreenCaptureAccess();
                }
                Permission::InputMonitoring => {
                    CGRequestListenEventAccess();
                }
                Permission::Accessibility => {
                    let keys = [kAXTrustedCheckOptionPrompt];
                    let values = [kCFBooleanTrue];
                    let options = CFDictionaryCreate(
                        std::ptr::null(),
                        keys.as_ptr(),
                        values.as_ptr(),
                        1,
                        &kCFTypeDictionaryKeyCallBacks,
                        &kCFTypeDictionaryValueCallBacks,
                    );
                    AXIsProcessTrustedWithOptions(options);
                    CFRelease(options);
                }
            }
        }
    }

    fn open_settings(&self, p: Permission) -> bool {
        let url = settings_url(p);
        // SAFETY: the URL is created from a static string, opened, and released.
        unsafe {
            let cf = CFURLCreateWithBytes(std::ptr::null(), url.as_ptr(), url.len() as isize, UTF8, std::ptr::null());
            if cf.is_null() {
                return false;
            }
            let status = LSOpenCFURLRef(cf, std::ptr::null_mut());
            CFRelease(cf);
            status == 0
        }
    }

    fn needs_relaunch(&self, p: Permission) -> bool {
        // A running process keeps the screen-capture answer it started with.
        p == Permission::ScreenRecording
    }
}

/// The pane of System Settings → Privacy & Security where `p` is switched on.
pub fn settings_url(p: Permission) -> &'static str {
    match p {
        Permission::ScreenRecording => "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
        Permission::Accessibility => "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        Permission::InputMonitoring => "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent",
    }
}

/// The app macOS will attribute these permissions to, as best this process can tell. A bundled
/// Orbit is itself; a binary run from a terminal is the terminal.
pub fn grantee() -> String {
    if let Some(app) = bundle_name() {
        return app;
    }
    // Set by LaunchServices for everything an app starts, and inherited down the process tree —
    // the most direct answer short of asking TCC, which offers no API for it.
    if let Ok(id) = std::env::var("__CFBundleIdentifier") {
        return match id.as_str() {
            "com.microsoft.VSCode" => "Visual Studio Code".into(),
            "com.microsoft.VSCodeInsiders" => "Visual Studio Code - Insiders".into(),
            "com.apple.Terminal" => "Terminal".into(),
            "com.googlecode.iterm2" => "iTerm".into(),
            "dev.warp.Warp-Stable" => "Warp".into(),
            "com.mitchellh.ghostty" => "Ghostty".into(),
            _ => id,
        };
    }
    match std::env::var("TERM_PROGRAM").as_deref() {
        Ok("vscode") => "Visual Studio Code".into(),
        Ok("Apple_Terminal") => "Terminal".into(),
        Ok("iTerm.app") => "iTerm".into(),
        Ok("WarpTerminal") => "Warp".into(),
        Ok("ghostty") => "Ghostty".into(),
        Ok(other) => other.into(),
        Err(_) => "the app that started Orbit".into(),
    }
}

fn bundle_name() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .find(|a| a.extension().is_some_and(|e| e == "app"))
        .and_then(|a| a.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
}

/// Where the flow remembers which prompts it has already shown.
pub fn asked_path() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join("Library/Application Support/Orbit/access.json")
}

pub fn load_asked() -> Asked {
    std::fs::read(asked_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save_asked(asked: &Asked) -> std::io::Result<()> {
    let path = asked_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(asked).expect("Asked serialises"))
}
