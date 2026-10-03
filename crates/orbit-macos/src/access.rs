//! Permissions: check, ask, and send the operator to the right switch.
use orbit_daemon::{Access, Asked, Permission, Status};

use crate::sys::*;

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
                    let options = Cf::own(CFDictionaryCreate(
                        std::ptr::null(),
                        keys.as_ptr(),
                        values.as_ptr(),
                        1,
                        &kCFTypeDictionaryKeyCallBacks,
                        &kCFTypeDictionaryValueCallBacks,
                    ));
                    if let Some(options) = options {
                        AXIsProcessTrustedWithOptions(options.ptr());
                    }
                }
            }
        }
    }

    fn open_settings(&self, p: Permission) -> bool {
        let Some(url) = cfurl(settings_url(p), false) else { return false };
        // SAFETY: a live CFURL; the launched-URL out-parameter is optional.
        unsafe { LSOpenCFURLRef(url.ptr(), std::ptr::null_mut()) == 0 }
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
pub fn asked_path() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
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
