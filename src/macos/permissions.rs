//! What Stickmouse needs from macOS: Accessibility access, and optionally a login item.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use objc2_app_kit::{NSButton, NSControlStateValueOn};
use objc2_core_graphics::CGRequestPostEventAccess;
use objc2_foundation::NSBundle;
use objc2_service_management::{SMAppService, SMAppServiceStatus};

use super::tray;
use crate::Message;

const ACCESSIBILITY_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
const BLUETOOTH_PANE: &str = "x-apple.systempreferences:com.apple.BluetoothSettings";

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
}

static GRANTED_AT_LAUNCH: OnceLock<bool> = OnceLock::new();

/// Whether Stickmouse may move the pointer and press keys. Reflects changes made while running.
pub fn accessibility_granted() -> bool {
    // SAFETY: no arguments, no side effects.
    let granted = unsafe { AXIsProcessTrusted() } != 0;
    GRANTED_AT_LAUNCH.get_or_init(|| granted);
    granted
}

/// Access granted after launch only takes effect once Stickmouse starts again.
pub fn needs_restart() -> bool {
    let granted = accessibility_granted();
    granted && GRANTED_AT_LAUNCH.get() == Some(&false)
}

/// Adds Stickmouse to the Accessibility list (switched off) and opens that list.
pub fn open_accessibility_settings() {
    CGRequestPostEventAccess();
    open(ACCESSIBILITY_PANE);
}

pub fn open_bluetooth_settings() {
    open(BLUETOOTH_PANE);
}

pub fn open(target: &str) {
    if let Err(error) = Command::new("open").arg(target).spawn() {
        eprintln!("stickmouse: could not open {target}: {error}");
    }
}

pub fn login_item_enabled() -> bool {
    // SAFETY: plain calls on the main thread.
    unsafe { SMAppService::mainAppService().status() == SMAppServiceStatus::Enabled }
}

pub fn set_login_item_from(checkbox: &NSButton) {
    let enable = checkbox.state() == NSControlStateValueOn;
    // SAFETY: plain calls on the main thread.
    let result = unsafe {
        let service = SMAppService::mainAppService();
        if enable { service.registerAndReturnError() } else { service.unregisterAndReturnError() }
    };
    if let Err(error) = result {
        eprintln!("stickmouse: could not change the login item: {}", error.localizedDescription());
        checkbox.setState(if login_item_enabled() { NSControlStateValueOn } else { 0 });
    }
}

/// Starts a new copy of Stickmouse and quits this one. The new copy waits for the old one to
/// release the single-instance lock.
pub fn restart() {
    let target = bundle_path().unwrap_or_else(|| std::env::current_exe().unwrap_or_default());
    let spawned = Command::new("/bin/sh")
        .args(["-c", "sleep 1; open \"$0\"", &target.to_string_lossy()])
        .spawn();
    match spawned {
        Ok(_) => tray::send(Message::Quit),
        Err(error) => eprintln!("stickmouse: could not restart: {error}"),
    }
}

fn bundle_path() -> Option<PathBuf> {
    let path = PathBuf::from(NSBundle::mainBundle().bundlePath().to_string());
    path.extension().is_some_and(|ext| ext == "app").then_some(path)
}
