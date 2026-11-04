//! Virtual mouse and keyboard. These are kept as two devices so the desktop classifies each correctly.

use std::io;

use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode, RelativeAxisCode};

use crate::engine::{Action, Key, MouseButton};

pub const DEVICE_PREFIX: &str = "Stickmouse";
/// The prefix used before the rename, so devices left by a still-running old copy are not mistaken for real mice.
pub const OLD_DEVICE_PREFIX: &str = "padcursor";
const WHEEL_NOTCH: i32 = 120;
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
        for key in Key::all() {
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
            Action::Tap(k) => self.tap(key_code(k)),
            Action::SwitcherHold(held) => self.keyboard.emit(&[key(KeyCode::KEY_LEFTALT, held.into())]),
            Action::SwitcherNext => self.tap(KeyCode::KEY_TAB),
            Action::Launcher => self.tap(KeyCode::KEY_LEFTMETA),
            _ => Ok(()),
        }
    }

    fn tap(&mut self, code: KeyCode) -> io::Result<()> {
        self.keyboard.emit(&[key(code, 1)])?;
        self.keyboard.emit(&[key(code, 0)])
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
        Key::Tab => KeyCode::KEY_TAB,
        Key::Enter => KeyCode::KEY_ENTER,
        Key::Space => KeyCode::KEY_SPACE,
        Key::Backspace => KeyCode::KEY_BACKSPACE,
        Key::Delete => KeyCode::KEY_DELETE,
        Key::Home => KeyCode::KEY_HOME,
        Key::End => KeyCode::KEY_END,
        Key::PageUp => KeyCode::KEY_PAGEUP,
        Key::PageDown => KeyCode::KEY_PAGEDOWN,
        Key::Letter(letter) => letter_code(letter),
        // KEY_1 through KEY_9 are 2 to 10, and KEY_0 is 11.
        Key::Digit(0) => KeyCode::KEY_0,
        Key::Digit(digit) => KeyCode(KeyCode::KEY_1.0 + u16::from(digit) - 1),
        // F1 through F10 are consecutive; F11 and F12 come later.
        Key::F(11) => KeyCode::KEY_F11,
        Key::F(12) => KeyCode::KEY_F12,
        Key::F(number) => KeyCode(KeyCode::KEY_F1.0 + u16::from(number) - 1),
        Key::Ctrl => KeyCode::KEY_LEFTCTRL,
        Key::Shift => KeyCode::KEY_LEFTSHIFT,
        Key::Alt => KeyCode::KEY_LEFTALT,
        Key::Meta => KeyCode::KEY_LEFTMETA,
        Key::PlayPause => KeyCode::KEY_PLAYPAUSE,
        Key::NextTrack => KeyCode::KEY_NEXTSONG,
        Key::PreviousTrack => KeyCode::KEY_PREVIOUSSONG,
        Key::VolumeUp => KeyCode::KEY_VOLUMEUP,
        Key::VolumeDown => KeyCode::KEY_VOLUMEDOWN,
        Key::Mute => KeyCode::KEY_MUTE,
    }
}

/// Linux key codes follow the QWERTY layout, not the alphabet.
fn letter_code(letter: u8) -> KeyCode {
    const LETTERS: [KeyCode; 26] = [
        KeyCode::KEY_A, KeyCode::KEY_B, KeyCode::KEY_C, KeyCode::KEY_D, KeyCode::KEY_E,
        KeyCode::KEY_F, KeyCode::KEY_G, KeyCode::KEY_H, KeyCode::KEY_I, KeyCode::KEY_J,
        KeyCode::KEY_K, KeyCode::KEY_L, KeyCode::KEY_M, KeyCode::KEY_N, KeyCode::KEY_O,
        KeyCode::KEY_P, KeyCode::KEY_Q, KeyCode::KEY_R, KeyCode::KEY_S, KeyCode::KEY_T,
        KeyCode::KEY_U, KeyCode::KEY_V, KeyCode::KEY_W, KeyCode::KEY_X, KeyCode::KEY_Y,
        KeyCode::KEY_Z,
    ];
    LETTERS[usize::from(letter.saturating_sub(b'a')).min(25)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_and_function_keys_use_the_right_codes() {
        assert_eq!(key_code(Key::Digit(1)), KeyCode::KEY_1);
        assert_eq!(key_code(Key::Digit(9)), KeyCode::KEY_9);
        assert_eq!(key_code(Key::Digit(0)), KeyCode::KEY_0);
        assert_eq!(key_code(Key::F(10)), KeyCode::KEY_F10);
        assert_eq!(key_code(Key::F(12)), KeyCode::KEY_F12);
        assert_eq!(key_code(Key::Letter(b'q')), KeyCode::KEY_Q);
    }
}
