//! Talks to KDE Plasma: cursor theme, on-screen keyboard, notifications and battery status.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Sender, channel};
use std::thread::{self, JoinHandle};

use super::cursor_theme::{OLD_THEME_NAME, THEME_NAME};

/// The folder name under the XDG config and state directories.
pub const APP_DIR: &str = "stickmouse";
const FALLBACK_THEME: &str = "breeze_cursors";
const VIRTUAL_KEYBOARD: &str = "org.kde.kwin.VirtualKeyboard";

/// Where per-user state lives, such as `~/.local/state`, shared by all apps.
pub fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").map_or_else(|| home().join(".local/state"), PathBuf::from)
}

/// Where per-user settings live, such as `~/.config`, shared by all apps.
pub fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home().join(".config"), PathBuf::from)
}

pub fn state_dir() -> PathBuf {
    state_home().join(APP_DIR)
}

pub fn config_file() -> PathBuf {
    config_home().join(APP_DIR).join("config.toml")
}

pub fn icons_dir() -> PathBuf {
    // KDE's cursor settings don't list themes from ~/.local/share/icons, but do from here.
    home().join(".icons")
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

fn saved_theme_file() -> PathBuf {
    state_dir().join("original-cursor-theme")
}

pub fn current_theme() -> Option<String> {
    let output = Command::new("kreadconfig6")
        .args(["--file", "kcminputrc", "--group", "Mouse", "--key", "cursorTheme"])
        .output()
        .ok()?;
    let theme = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some(if theme.is_empty() { FALLBACK_THEME.to_string() } else { theme })
}

fn apply_theme(theme: &str) -> bool {
    let result =
        Command::new("plasma-apply-cursortheme").arg(theme).stdout(Stdio::null()).stderr(Stdio::null()).status();
    match result {
        Ok(status) if status.success() => true,
        other => {
            eprintln!("stickmouse: could not apply cursor theme {theme}: {other:?}");
            false
        }
    }
}

fn show_circle() {
    let Some(current) = current_theme() else {
        return;
    };
    if is_circle(&current) {
        return;
    }
    // Saved first, so a crash while the circle is showing can still be undone.
    if let Err(error) = fs::write(saved_theme_file(), &current) {
        eprintln!("stickmouse: not showing circle, could not save cursor theme: {error}");
        return;
    }
    apply_theme(THEME_NAME);
}

/// Whether `theme` is the circle, under its current or its pre-rename name.
fn is_circle(theme: &str) -> bool {
    theme == THEME_NAME || theme == OLD_THEME_NAME
}

/// Puts the user's own cursor back if the circle is showing. Also runs after a crash, and after
/// an upgrade from padcursor that happened while its circle was showing.
pub fn restore_cursor() {
    if !current_theme().is_some_and(|theme| is_circle(&theme)) {
        return;
    }
    let saved = fs::read_to_string(saved_theme_file()).unwrap_or_default();
    let saved = saved.trim();
    let original = if saved.is_empty() { FALLBACK_THEME } else { saved };
    if apply_theme(original) {
        let _ = fs::remove_file(saved_theme_file());
    }
}

/// Switches the cursor on its own thread, because each switch takes around 100 ms.
pub struct CursorSwitcher {
    sender: Sender<bool>,
    worker: JoinHandle<()>,
}

impl CursorSwitcher {
    pub fn spawn() -> Self {
        let (sender, receiver) = channel::<bool>();
        let worker = thread::spawn(move || {
            while let Ok(first) = receiver.recv() {
                // Only the latest request matters if several piled up during a switch.
                let steering = receiver.try_iter().last().unwrap_or(first);
                if steering {
                    show_circle();
                } else {
                    restore_cursor();
                }
            }
        });
        Self { sender, worker }
    }

    pub fn set_steering(&self, steering: bool) {
        let _ = self.sender.send(steering);
    }

    pub fn finish(self) {
        drop(self.sender);
        let _ = self.worker.join();
        restore_cursor();
    }
}

pub fn toggle_keyboard() {
    thread::spawn(|| {
        if read_keyboard_property("available").as_deref() != Some("true") {
            notify(
                "On-screen keyboard is not set up",
                "Choose it in System Settings, under Keyboard, Virtual Keyboard.",
            );
            return;
        }
        if read_keyboard_property("visible").as_deref() == Some("true") {
            call_kwin(&[&format!("{VIRTUAL_KEYBOARD}.active"), "false"]);
        } else {
            call_kwin(&[&format!("{VIRTUAL_KEYBOARD}.forceActivate")]);
        }
    });
}

fn read_keyboard_property(name: &str) -> Option<String> {
    let output = Command::new("qdbus6")
        .args(["org.kde.KWin", "/VirtualKeyboard", &format!("{VIRTUAL_KEYBOARD}.{name}")])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn call_kwin(arguments: &[&str]) {
    let result = Command::new("qdbus6")
        .args(["org.kde.KWin", "/VirtualKeyboard"])
        .args(arguments)
        .stdout(Stdio::null())
        .status();
    if let Err(error) = result {
        eprintln!("stickmouse: could not reach the on-screen keyboard: {error}");
    }
}

/// Opens KDE Plasma's window overview, like Mission Control on a Mac.
pub fn mission_control() {
    let result = Command::new("qdbus6")
        .args(["org.kde.kglobalaccel", "/component/kwin", "org.kde.kglobalaccel.Component.invokeShortcut", "Overview"])
        .stdout(Stdio::null())
        .spawn();
    if let Err(error) = result {
        eprintln!("stickmouse: could not open the overview: {error}");
    }
}

pub fn notify(title: &str, body: &str) {
    let spawned = Command::new("notify-send").args(["-a", "Stickmouse", "-i", "input-gamepad", title, body]).spawn();
    match spawned {
        Ok(mut child) => {
            thread::spawn(move || child.wait());
        }
        Err(error) => eprintln!("stickmouse: {title}: {body} ({error})"),
    }
}

/// The controller's battery level, as reported by its driver: Low, Normal, High or Full.
pub fn battery_level() -> io::Result<Option<String>> {
    for entry in fs::read_dir("/sys/class/power_supply")?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("xpadneo_battery_")
            && let Ok(level) = fs::read_to_string(entry.path().join("capacity_level"))
        {
            return Ok(Some(level.trim().to_string()));
        }
    }
    Ok(None)
}

/// Opens the settings file in the default text editor.
pub fn open_settings() {
    if let Err(error) = Command::new("xdg-open").arg(config_file()).spawn() {
        eprintln!("stickmouse: could not open settings: {error}");
    }
}
