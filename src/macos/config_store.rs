//! The settings as the windows see them. Both the settings window and the button window change
//! the same copy, which is saved to config.toml and sent to the running engine on every change.

use std::cell::RefCell;
use std::fs;

use super::{cursor_theme, desktop, tray};
use crate::Message;
use crate::config::Config;

thread_local! {
    static CONFIG: RefCell<Option<Config>> = const { RefCell::new(None) };
}

fn load() -> Config {
    fs::read_to_string(desktop::config_file())
        .ok()
        .and_then(|text| Config::parse(&text).ok())
        .unwrap_or_default()
}

/// The current settings, read from the file the first time.
pub fn get() -> Config {
    CONFIG.with_borrow_mut(|config| config.get_or_insert_with(load).clone())
}

/// Reads the file again, for when it was edited by hand.
pub fn reload() {
    CONFIG.with_borrow_mut(|config| *config = Some(load()));
}

/// Changes the settings, saves them and applies them right away.
pub fn update(change: impl FnOnce(&mut Config)) {
    let config = CONFIG.with_borrow_mut(|config| {
        let config = config.get_or_insert_with(load);
        change(config);
        config.clone()
    });
    let path = desktop::config_file();
    if let Err(error) = fs::write(&path, config.to_toml()) {
        eprintln!("padcursor: could not write {}: {error}", path.display());
    }
    cursor_theme::set_scale(config.circle_scale);
    tray::send(Message::ConfigChanged(config));
}
