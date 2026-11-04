//! System tray icon showing state and battery, with a menu for use without the controller.

use std::sync::mpsc::Sender;

use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{MenuItem, ToolTip};

use crate::Message;
use crate::engine::Input;

pub type Handle = ksni::blocking::Handle<Tray>;

pub struct Tray {
    pub enabled: bool,
    pub connected: bool,
    pub battery: Option<String>,
    messages: Sender<Message>,
}

impl Tray {
    fn status(&self) -> String {
        match (self.connected, self.enabled) {
            (false, _) => "Controller not connected".to_string(),
            (true, false) => "Off".to_string(),
            (true, true) => "On".to_string(),
        }
    }

    fn battery_text(&self) -> String {
        format!("Battery: {}", self.battery.as_deref().unwrap_or("unknown"))
    }

    fn send(&self, message: Message) {
        let _ = self.messages.send(message);
    }
}

impl ksni::Tray for Tray {
    const MENU_ON_ACTIVATE: bool = false;

    fn id(&self) -> String {
        "padcursor".into()
    }

    fn title(&self) -> String {
        "padcursor".into()
    }

    fn icon_name(&self) -> String {
        "input-gamepad".into()
    }

    fn overlay_icon_name(&self) -> String {
        if self.enabled && self.connected { String::new() } else { "emblem-pause".into() }
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: format!("padcursor: {}", self.status()),
            description: self.battery_text(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Message::Input(Input::ToggleEnabled));
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: self.status(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: self.battery_text(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "Enabled".into(),
                checked: self.enabled,
                activate: Box::new(|tray: &mut Self| {
                    tray.send(Message::Input(Input::ToggleEnabled));
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Open settings".into(),
                activate: Box::new(|tray: &mut Self| tray.send(Message::OpenSettings)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|tray: &mut Self| tray.send(Message::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Starts the tray. padcursor keeps working without one, so a failure here isn't fatal.
pub fn spawn(enabled: bool, messages: Sender<Message>) -> Option<Handle> {
    let tray = Tray { enabled, connected: false, battery: None, messages };
    match tray.spawn() {
        Ok(handle) => Some(handle),
        Err(error) => {
            eprintln!("padcursor: no tray icon: {error}");
            None
        }
    }
}
