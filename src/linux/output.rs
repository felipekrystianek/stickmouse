//! Virtual mouse and keyboard. These are kept as two devices so the desktop classifies each correctly.

use std::io;

use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode, RelativeAxisCode};

use crate::engine::{Action, Key, MouseButton};

pub const DEVICE_PREFIX: &str = "padcursor";
const WHEEL_NOTCH: i32 = 120;
const ALL_KEYS: [Key; 14] = [
    Key::Up,
    Key::Down,
    Key::Left,
    Key::Right,
    Key::Esc,
    Key::Alt,
    Key::Tab,
    Key::Meta,
    Key::PlayPause,
    Key::NextTrack,
    Key::PreviousTrack,
    Key::VolumeUp,
    Key::VolumeDown,
    Key::Mute,
];

pub struct Output {
    mouse: VirtualDevice,
    keyboard: VirtualDevice,
    wheel: (i32, i32),
}

impl Output {
    pub fn new() -> io::Result<Self> {
        let mut buttons = AttributeSet::new();
        for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            buttons.insert(mouse_code(button));
        }
        let mut axes = AttributeSet::new();
        for axis in [
            RelativeAxisCode::REL_X,
            RelativeAxisCode::REL_Y,
            RelativeAxisCode::REL_WHEEL,
            RelativeAxisCode::REL_HWHEEL,
            RelativeAxisCode::REL_WHEEL_HI_RES,
            RelativeAxisCode::REL_HWHEEL_HI_RES,
        ] {
            axes.insert(axis);
        }
        let mouse = VirtualDevice::builder()?
            .name(&format!("{DEVICE_PREFIX} mouse"))
            .with_keys(&buttons)?
            .with_relative_axes(&axes)?
            .build()?;

        let mut keys = AttributeSet::new();
        for key in ALL_KEYS {
            keys.insert(key_code(key));
        }
        let keyboard = VirtualDevice::builder()?
            .name(&format!("{DEVICE_PREFIX} keyboard"))
            .with_keys(&keys)?
            .build()?;

        Ok(Self { mouse, keyboard, wheel: (0, 0) })
    }

    /// Performs an input action. Other actions are not this module's job and are ignored.
    pub fn apply(&mut self, action: Action) -> io::Result<()> {
        match action {
            Action::Move(x, y) => self.mouse.emit(&[
                relative(RelativeAxisCode::REL_X, x),
                relative(RelativeAxisCode::REL_Y, y),
            ]),
            Action::Scroll(vertical, horizontal) => self.scroll(vertical, horizontal),
            Action::MouseDown(button) => self.mouse.emit(&[key(mouse_code(button), 1)]),
            Action::MouseUp(button) => self.mouse.emit(&[key(mouse_code(button), 0)]),
            Action::Click(button) => {
                self.mouse.emit(&[key(mouse_code(button), 1)])?;
                self.mouse.emit(&[key(mouse_code(button), 0)])
            }
            Action::KeyDown(k) => self.keyboard.emit(&[key(key_code(k), 1)]),
            Action::KeyUp(k) => self.keyboard.emit(&[key(key_code(k), 0)]),
            Action::Tap(k) => {
                self.keyboard.emit(&[key(key_code(k), 1)])?;
                self.keyboard.emit(&[key(key_code(k), 0)])
            }
            _ => Ok(()),
        }
    }

    fn scroll(&mut self, vertical: i32, horizontal: i32) -> io::Result<()> {
        // Apps that ignore hi-res scrolling still need the classic event at each full notch.
        self.wheel.0 += vertical;
        self.wheel.1 += horizontal;
        let notches = (self.wheel.0 / WHEEL_NOTCH, self.wheel.1 / WHEEL_NOTCH);
        self.wheel.0 %= WHEEL_NOTCH;
        self.wheel.1 %= WHEEL_NOTCH;
        self.mouse.emit(&[
            relative(RelativeAxisCode::REL_WHEEL_HI_RES, vertical),
            relative(RelativeAxisCode::REL_HWHEEL_HI_RES, horizontal),
            relative(RelativeAxisCode::REL_WHEEL, notches.0),
            relative(RelativeAxisCode::REL_HWHEEL, notches.1),
        ])
    }
}

fn relative(axis: RelativeAxisCode, value: i32) -> InputEvent {
    InputEvent::new(EventType::RELATIVE.0, axis.0, value)
}

fn key(code: KeyCode, value: i32) -> InputEvent {
    InputEvent::new(EventType::KEY.0, code.0, value)
}

fn mouse_code(button: MouseButton) -> KeyCode {
    match button {
        MouseButton::Left => KeyCode::BTN_LEFT,
        MouseButton::Right => KeyCode::BTN_RIGHT,
        MouseButton::Middle => KeyCode::BTN_MIDDLE,
    }
}

fn key_code(key: Key) -> KeyCode {
    match key {
        Key::Up => KeyCode::KEY_UP,
        Key::Down => KeyCode::KEY_DOWN,
        Key::Left => KeyCode::KEY_LEFT,
        Key::Right => KeyCode::KEY_RIGHT,
        Key::Esc => KeyCode::KEY_ESC,
        Key::Alt => KeyCode::KEY_LEFTALT,
        Key::Tab => KeyCode::KEY_TAB,
        Key::Meta => KeyCode::KEY_LEFTMETA,
        Key::PlayPause => KeyCode::KEY_PLAYPAUSE,
        Key::NextTrack => KeyCode::KEY_NEXTSONG,
        Key::PreviousTrack => KeyCode::KEY_PREVIOUSSONG,
        Key::VolumeUp => KeyCode::KEY_VOLUMEUP,
        Key::VolumeDown => KeyCode::KEY_VOLUMEDOWN,
        Key::Mute => KeyCode::KEY_MUTE,
    }
}
