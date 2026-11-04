mod bindings;
mod config;
mod engine;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;

use std::error::Error;
use std::fs::{self, File};
use std::os::fd::AsRawFd;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use config::Config;
use engine::{Action, Engine, Input};
use platform::controller::{self, Command as ControllerCommand};
use platform::desktop::{self, CursorSwitcher};
use platform::output::Output;
use platform::tray::Tray;
use platform::{cursor_theme, mouse_watch, tray};

const TICK: Duration = Duration::from_millis(8);
const IDLE_WAIT: Duration = Duration::from_millis(250);
const BATTERY_INTERVAL: Duration = Duration::from_secs(30);
const LOW_BATTERY: &str = "Low";

pub enum Message {
    Input(Input),
    Connected(bool),
    /// New settings from the settings window. Only macOS has one.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    ConfigChanged(Config),
    OpenSettings,
    Quit,
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--restore-cursor") {
        desktop::restore_cursor();
        return ExitCode::SUCCESS;
    }
    platform::run_app(start)
}

fn start() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("padcursor: {error}");
            desktop::notify("Stickmouse could not start", &error.to_string());
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    // Before anything creates the new folders, which would make the copy skip them.
    #[cfg(target_os = "linux")]
    linux::migrate::old_settings();
    fs::create_dir_all(desktop::state_dir())?;
    let _lock = lock_single_instance()?;
    let config = load_config();

    desktop::restore_cursor();
    cursor_theme::install(&desktop::icons_dir(), config.circle_scale)?;
    #[cfg(target_os = "linux")]
    linux::migrate::old_cursor_theme(&desktop::icons_dir());

    let enabled_file = desktop::state_dir().join("enabled");
    let enabled = fs::read_to_string(&enabled_file).map_or(true, |text| text.trim() != "false");

    let quit = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, quit.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, quit.clone())?;

    let (messages_tx, messages) = channel();
    let mut output = Output::new()?;
    let controller = controller::spawn(messages_tx.clone(), enabled);
    mouse_watch::spawn(messages_tx.clone());
    let tray = tray::spawn(enabled, messages_tx.clone());
    let cursor = CursorSwitcher::spawn();
    let mut engine = Engine::new(config, enabled);

    let start = Instant::now();
    let mut connected = false;
    let mut enabled = enabled;
    let mut last_tick = Duration::ZERO;
    let mut next_battery_check = Duration::ZERO;
    let mut low_battery_warned = false;

    let perform = |actions: Vec<Action>, enabled: &mut bool, output: &mut Output| {
        for action in actions {
            match action {
                Action::Steering(steering) => cursor.set_steering(steering),
                Action::ToggleKeyboard => desktop::toggle_keyboard(),
                Action::MissionControl => desktop::mission_control(),
                Action::Rumble => send(&controller, ControllerCommand::Rumble),
                Action::SetEnabled(value) => {
                    *enabled = value;
                    send(&controller, ControllerCommand::Grab(value));
                    if let Err(error) = fs::write(&enabled_file, value.to_string()) {
                        eprintln!("padcursor: could not save state: {error}");
                    }
                    update_tray(&tray, |t| t.enabled = value);
                }
                other => {
                    if let Err(error) = output.apply(other) {
                        eprintln!("padcursor: could not send input: {error}");
                    }
                }
            }
        }
    };

    while !quit.load(Ordering::Relaxed) {
        let wait = if connected && enabled { TICK } else { IDLE_WAIT };
        let message = match messages.recv_timeout(wait) {
            Ok(message) => Some(message),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let now = start.elapsed();
        match message {
            Some(Message::Input(input)) => {
                let actions = engine.handle(input, now);
                perform(actions, &mut enabled, &mut output);
            }
            Some(Message::Connected(value)) => {
                connected = value;
                update_tray(&tray, |t| {
                    t.connected = value;
                    if !value {
                        t.battery = None;
                    }
                });
                next_battery_check = now;
                low_battery_warned = false;
            }
            Some(Message::ConfigChanged(config)) => engine.set_config(config),
            Some(Message::OpenSettings) => desktop::open_settings(),
            Some(Message::Quit) => break,
            None => {}
        }

        if now.saturating_sub(last_tick) >= TICK {
            last_tick = now;
            let actions = engine.tick(now);
            perform(actions, &mut enabled, &mut output);
        }

        if connected && now >= next_battery_check {
            next_battery_check = now + BATTERY_INTERVAL;
            let level = desktop::battery_level().ok().flatten();
            let low = level.as_deref() == Some(LOW_BATTERY);
            if low && !low_battery_warned {
                desktop::notify("Controller battery is low", "Charge it or change the batteries soon.");
            }
            low_battery_warned = low;
            update_tray(&tray, |t| t.battery = level.clone());
        }
    }

    let actions = engine.shutdown();
    perform(actions, &mut enabled, &mut output);
    cursor.finish();
    Ok(())
}

fn send(controller: &Sender<ControllerCommand>, command: ControllerCommand) {
    let _ = controller.send(command);
}

fn update_tray(tray: &Option<tray::Handle>, mut change: impl FnMut(&mut Tray)) {
    if let Some(handle) = tray {
        handle.update(|t| change(t));
    }
}

fn load_config() -> Config {
    let path = desktop::config_file();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => {
            let written = path
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| fs::write(&path, Config::default().to_toml()));
            if let Err(error) = written {
                eprintln!("padcursor: could not write {}: {error}", path.display());
            }
            return Config::default();
        }
    };
    Config::parse(&text).unwrap_or_else(|error| {
        eprintln!("padcursor: {}: {error}", path.display());
        desktop::notify("Stickmouse settings have a mistake", "Using the default settings for now.");
        Config::default()
    })
}

/// Two copies would fight over the controller, so only one may run. The kernel releases the lock on exit.
fn lock_single_instance() -> Result<File, Box<dyn Error>> {
    let file = File::create(desktop::state_dir().join("lock"))?;
    // SAFETY: the file descriptor is valid for as long as `file` is alive.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Stickmouse is already running".into());
    }
    Ok(file)
}
