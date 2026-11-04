//! Everything specific to Linux and KDE Plasma. `src/macos` provides the same modules for macOS.

pub mod controller;
pub mod cursor_theme;
pub mod desktop;
pub mod mouse_watch;
pub mod output;
pub mod tray;

use std::process::ExitCode;

/// Linux needs no app event loop, so `work` simply runs on the main thread.
pub fn run_app(work: impl FnOnce() -> ExitCode) -> ExitCode {
    work()
}
