//! Files, notifications, battery and the on-screen keyboard on macOS.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::thread;

use super::controller;
use super::cursor_theme::STEERING;

const LOW_BATTERY: f32 = 0.2;
const KEYBOARD_PROCESS: &str = "Assistive Control";
const KEYBOARD_APP: &str = "/System/Library/Input Methods/Assistive Control.app";

pub fn state_dir() -> PathBuf {
    home().join("Library/Application Support/padcursor")
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

/// Shows or hides the Accessibility Keyboard.
pub fn toggle_keyboard() {
    thread::spawn(|| {
        let running = Command::new("pgrep")
            .args(["-x", KEYBOARD_PROCESS])
            .stdout(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        let result = if running {
            Command::new("pkill").args(["-x", KEYBOARD_PROCESS]).status()
        } else {
            Command::new("open").arg(KEYBOARD_APP).status()
        };
        if !result.is_ok_and(|status| status.success()) {
            notify(
                "Could not open the on-screen keyboard",
                "Turn on the Accessibility Keyboard in System Settings, Accessibility, Keyboard.",
            );
        }
    });
}

pub fn notify(title: &str, body: &str) {
    let script = format!(
        "display notification {} with title \"padcursor\" subtitle {}",
        applescript_string(body),
        applescript_string(title)
    );
    match Command::new("osascript").args(["-e", &script]).stderr(Stdio::null()).spawn() {
        Ok(mut child) => {
            thread::spawn(move || child.wait());
        }
        Err(error) => eprintln!("padcursor: {title}: {body} ({error})"),
    }
}

fn applescript_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Matches the Linux wording, where the kernel reports "Low" for a nearly empty controller.
pub fn battery_level() -> io::Result<Option<String>> {
    Ok(controller::battery().map(|level| {
        if level <= LOW_BATTERY { "Low".to_string() } else { format!("{:.0}%", level * 100.0) }
    }))
}

pub fn open_file(path: &Path) -> io::Result<()> {
    Command::new("open").arg("-t").arg(path).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applescript_strings_escape_quotes_and_backslashes() {
        assert_eq!(applescript_string(r#"say "hi" \ bye"#), r#""say \"hi\" \\ bye""#);
    }
}
