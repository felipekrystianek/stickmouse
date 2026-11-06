//! Files, notifications, battery and the on-screen keyboard on macOS.

use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use super::controller;
use super::cursor_theme::STEERING;

const LOW_BATTERY: f32 = 0.2;
const MISSION_CONTROL: &str = "/System/Applications/Mission Control.app";

pub fn state_dir() -> PathBuf {
    home().join("Library/Application Support/Stickmouse")
}

pub fn config_file() -> PathBuf {
    state_dir().join("config.toml")
}

/// The steering circle is a window on macOS, so there is no cursor theme to install.
pub fn icons_dir() -> PathBuf {
    state_dir()
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

pub fn restore_cursor() {
    STEERING.store(false, Ordering::Relaxed);
}

pub struct CursorSwitcher;

impl CursorSwitcher {
    pub fn spawn() -> Self {
        Self
    }

    pub fn set_steering(&self, steering: bool) {
        STEERING.store(steering, Ordering::Relaxed);
    }

    pub fn finish(self) {
        restore_cursor();
    }
}

/// macOS does not let other apps show its Accessibility Keyboard.
pub fn toggle_keyboard() {
    notify(
        "The on-screen keyboard is not available",
        "macOS does not allow it. Choose another action for this button in Customize Buttons.",
    );
}

/// Opens Mission Control; doing it again closes it.
pub fn mission_control() {
    if let Err(error) = Command::new("open").arg(MISSION_CONTROL).spawn() {
        eprintln!("stickmouse: could not open Mission Control: {error}");
    }
}

pub fn notify(title: &str, body: &str) {
    let script = format!(
        "display notification {} with title \"Stickmouse\" subtitle {}",
        applescript_string(body),
        applescript_string(title)
    );
    match Command::new("osascript").args(["-e", &script]).stderr(Stdio::null()).spawn() {
        Ok(mut child) => {
            thread::spawn(move || child.wait());
        }
        Err(error) => eprintln!("stickmouse: {title}: {body} ({error})"),
    }
}

fn applescript_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Matches the Linux wording, where the kernel reports "Low" for a nearly empty controller.
pub fn battery_level() -> io::Result<Option<String>> {
    Ok(controller::battery()
        .map(|level| if level <= LOW_BATTERY { "Low".to_string() } else { format!("{:.0}%", level * 100.0) }))
}

static SETTINGS_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Asks the main thread to show the settings window.
pub fn open_settings() {
    SETTINGS_REQUESTED.store(true, Ordering::Relaxed);
}

pub fn take_settings_request() -> bool {
    SETTINGS_REQUESTED.swap(false, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applescript_strings_escape_quotes_and_backslashes() {
        assert_eq!(applescript_string(r#"say "hi" \ bye"#), r#""say \"hi\" \\ bye""#);
    }
}
