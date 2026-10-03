//! ORBIT ON macOS — the driver. It turns the machine into what the daemon speaks: a [`Capture`] of
//! pixels, the frontmost app's accessibility tree, every window on screen, and on-device OCR.
//!
//! The system calls are declared by hand in [`sys`] rather than through binding crates. There are a
//! few dozen, they are stable C entry points, and every line that talks to the OS should be one a
//! reviewer can read without opening a dependency. Vision, which has no C API, goes through `objc2`.
//!
//! # Whose permission it is
//!
//! macOS grants screen recording and accessibility to an *app*, identified by its code signature —
//! not to a process, and not to a binary path. Run from a terminal, a bare executable is attributed
//! to the terminal (or to VS Code, for its built-in one), and the grant lands there. Fine for
//! development and wrong for a product: the shipped daemon is a signed `.app` with a stable bundle
//! identifier and team ID, so a grant survives updates, and so a fleet can pre-approve it by MDM
//! profile instead of five hundred people clicking through System Settings.
//!
//! [`Capture`]: orbit_daemon::Capture
#![cfg(target_os = "macos")]

pub mod access;
pub mod ax;
pub mod ocr;
pub mod screen;
pub mod sys;

pub use access::{asked_path, grantee, load_asked, save_asked, settings_url, MacAccess};
