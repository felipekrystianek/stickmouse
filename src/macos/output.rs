//! Sends mouse, keyboard and media key events with Quartz Event Services.
//! Needs Accessibility access (System Settings, Privacy & Security, Accessibility).

use std::io;
use std::time::{Duration, Instant};

use objc2_core_foundation::{CFRetained, CGPoint, CGRect};
use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayBounds, CGEvent, CGEventField, CGEventFlags, CGEventSource,
    CGEventSourceStateID, CGEventTapLocation, CGEventType, CGGetActiveDisplayList, CGMouseButton,
    CGPreflightPostEventAccess, CGRequestPostEventAccess, CGScrollEventUnit,
};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::NSPoint;

use super::desktop;
use crate::engine::{Action, Key, MouseButton};

/// Written into every event we post, so the real-mouse watcher can tell them apart.
pub const EVENT_MARK: i64 = 0x7061_6463;
/// The engine counts 120 units per wheel notch; a notch on a Mac mouse is roughly 40 pixels.
const UNITS_PER_PIXEL: i32 = 3;
const DOUBLE_CLICK_DISTANCE: f64 = 4.0;
const MAX_DISPLAYS: usize = 16;

// Virtual key codes from Carbon's Events.h.
const KEY_TAB: u16 = 0x30;
const KEY_SPACE: u16 = 0x31;
const KEY_ESCAPE: u16 = 0x35;
const KEY_COMMAND: u16 = 0x37;
const KEY_LEFT: u16 = 0x7B;
const KEY_RIGHT: u16 = 0x7C;
const KEY_DOWN: u16 = 0x7D;
const KEY_UP: u16 = 0x7E;

// Media keys from IOKit's ev_keymap.h, sent as system-defined events.
const MEDIA_SOUND_UP: isize = 0;
const MEDIA_SOUND_DOWN: isize = 1;
const MEDIA_MUTE: isize = 7;
const MEDIA_PLAY: isize = 16;
const MEDIA_FAST: isize = 19;
const MEDIA_REWIND: isize = 20;
const SYSTEM_DEFINED_AUX_KEY: i16 = 8;

enum Code {
    Key(u16),
    Media(isize),
}

#[derive(Clone, Copy)]
struct Press {
    at: Instant,
    position: CGPoint,
    count: i64,
}

pub struct Output {
    source: CFRetained<CGEventSource>,
    command_held: bool,
    held: [bool; 3],
    last_press: [Option<Press>; 3],
    double_click: Duration,
    wheel: (i32, i32),
}

impl Output {
    pub fn new() -> io::Result<Self> {
        if !CGPreflightPostEventAccess() {
            CGRequestPostEventAccess();
            desktop::notify(
                "padcursor needs Accessibility access",
                "Allow it in System Settings, Privacy & Security, Accessibility, then start padcursor again.",
            );
        }
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .ok_or_else(|| io::Error::other("could not create an event source"))?;
        Ok(Self {
            source,
            command_held: false,
            held: [false; 3],
            last_press: [None; 3],
            double_click: Duration::from_secs_f64(NSEvent::doubleClickInterval()),
            wheel: (0, 0),
        })
    }

    pub fn apply(&mut self, action: Action) -> io::Result<()> {
        match action {
            Action::Move(x, y) => self.move_by(x, y),
            Action::Scroll(vertical, horizontal) => self.scroll(vertical, horizontal),
            Action::MouseDown(button) => self.mouse_button(button, true),
            Action::MouseUp(button) => self.mouse_button(button, false),
            Action::Click(button) => {
                self.mouse_button(button, true)?;
                self.mouse_button(button, false)
            }
            Action::KeyDown(key) => self.key(key, true),
            Action::KeyUp(key) => self.key(key, false),
            // A lone Command tap does nothing on a Mac, so the launcher is Spotlight instead.
            Action::Tap(Key::Meta) => {
                self.key_code(KEY_COMMAND, true, CGEventFlags::MaskCommand)?;
                self.key_code(KEY_SPACE, true, CGEventFlags::MaskCommand)?;
                self.key_code(KEY_SPACE, false, CGEventFlags::MaskCommand)?;
                self.key_code(KEY_COMMAND, false, self.flags())
            }
            Action::Tap(key) => {
                self.key(key, true)?;
                self.key(key, false)
            }
            _ => Ok(()),
        }
    }

    fn move_by(&mut self, dx: i32, dy: i32) -> io::Result<()> {
        let from = cursor_position();
        let to = keep_on_screen(from, CGPoint::new(from.x + f64::from(dx), from.y + f64::from(dy)));
        let (kind, button) = if self.held[0] {
            (CGEventType::LeftMouseDragged, CGMouseButton::Left)
        } else if self.held[1] {
            (CGEventType::RightMouseDragged, CGMouseButton::Right)
        } else if self.held[2] {
            (CGEventType::OtherMouseDragged, CGMouseButton::Center)
        } else {
            (CGEventType::MouseMoved, CGMouseButton::Left)
        };
        let event = CGEvent::new_mouse_event(Some(&self.source), kind, to, button)
            .ok_or_else(event_error)?;
        CGEvent::set_integer_value_field(Some(&event), CGEventField::MouseEventDeltaX, dx.into());
        CGEvent::set_integer_value_field(Some(&event), CGEventField::MouseEventDeltaY, dy.into());
        self.post(&event);
        Ok(())
    }

    fn scroll(&mut self, vertical: i32, horizontal: i32) -> io::Result<()> {
        self.wheel.0 += vertical;
        self.wheel.1 += horizontal;
        let pixels = (self.wheel.0 / UNITS_PER_PIXEL, self.wheel.1 / UNITS_PER_PIXEL);
        self.wheel.0 %= UNITS_PER_PIXEL;
        self.wheel.1 %= UNITS_PER_PIXEL;
        if pixels == (0, 0) {
            return Ok(());
        }
        // Quartz counts the second wheel positive to the left, the engine positive to the right.
        let event = CGEvent::new_scroll_wheel_event2(
            Some(&self.source),
            CGScrollEventUnit::Pixel,
            2,
            pixels.0,
            -pixels.1,
            0,
        )
        .ok_or_else(event_error)?;
        self.post(&event);
        Ok(())
    }

    fn mouse_button(&mut self, button: MouseButton, down: bool) -> io::Result<()> {
        let (index, cg_button, down_type, up_type) = match button {
            MouseButton::Left => {
                (0, CGMouseButton::Left, CGEventType::LeftMouseDown, CGEventType::LeftMouseUp)
            }
            MouseButton::Right => {
                (1, CGMouseButton::Right, CGEventType::RightMouseDown, CGEventType::RightMouseUp)
            }
            MouseButton::Middle => {
                (2, CGMouseButton::Center, CGEventType::OtherMouseDown, CGEventType::OtherMouseUp)
            }
        };
        let position = cursor_position();
        // Quartz does not count clicks for posted events, so double clicks are counted here.
        let count = if down {
            let now = Instant::now();
            let count = match self.last_press[index] {
                Some(last)
                    if now.duration_since(last.at) <= self.double_click
                        && (last.position.x - position.x).abs() <= DOUBLE_CLICK_DISTANCE
                        && (last.position.y - position.y).abs() <= DOUBLE_CLICK_DISTANCE =>
                {
                    last.count + 1
                }
                _ => 1,
            };
            self.last_press[index] = Some(Press { at: now, position, count });
            count
        } else {
            self.last_press[index].map_or(1, |press| press.count)
        };
        self.held[index] = down;

        let kind = if down { down_type } else { up_type };
        let event = CGEvent::new_mouse_event(Some(&self.source), kind, position, cg_button)
            .ok_or_else(event_error)?;
        CGEvent::set_integer_value_field(Some(&event), CGEventField::MouseEventClickState, count);
        self.post(&event);
        Ok(())
    }

    fn key(&mut self, key: Key, down: bool) -> io::Result<()> {
        match code(key) {
            Code::Key(code) => {
                // Alt+Tab on Linux is Command+Tab on a Mac.
                if key == Key::Alt {
                    self.command_held = down;
                }
                let mut flags = self.flags();
                if matches!(key, Key::Up | Key::Down | Key::Left | Key::Right) {
                    flags |= CGEventFlags::MaskSecondaryFn | CGEventFlags::MaskNumericPad;
                }
                self.key_code(code, down, flags)
            }
            Code::Media(code) => media_key(code, down),
        }
    }

    fn key_code(&self, code: u16, down: bool, flags: CGEventFlags) -> io::Result<()> {
        let event = CGEvent::new_keyboard_event(Some(&self.source), code, down)
            .ok_or_else(event_error)?;
        CGEvent::set_flags(Some(&event), flags);
        self.post(&event);
        Ok(())
    }

    fn flags(&self) -> CGEventFlags {
        if self.command_held { CGEventFlags::MaskCommand } else { CGEventFlags(0) }
    }

    fn post(&self, event: &CGEvent) {
        CGEvent::set_integer_value_field(Some(event), CGEventField::EventSourceUserData, EVENT_MARK);
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(event));
    }
}

fn media_key(code: isize, down: bool) -> io::Result<()> {
    let state: isize = if down { 0xA } else { 0xB };
    let event = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        NSEventType::SystemDefined,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags((state << 8) as usize),
        0.0,
        0,
        None,
        SYSTEM_DEFINED_AUX_KEY,
        (code << 16) | (state << 8),
        -1,
    )
    .and_then(|event| event.CGEvent())
    .ok_or_else(event_error)?;
    CGEvent::set_integer_value_field(Some(&event), CGEventField::EventSourceUserData, EVENT_MARK);
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
    Ok(())
}

fn code(key: Key) -> Code {
    match key {
        Key::Up => Code::Key(KEY_UP),
        Key::Down => Code::Key(KEY_DOWN),
        Key::Left => Code::Key(KEY_LEFT),
        Key::Right => Code::Key(KEY_RIGHT),
        Key::Esc => Code::Key(KEY_ESCAPE),
        Key::Alt => Code::Key(KEY_COMMAND),
        Key::Tab => Code::Key(KEY_TAB),
        Key::Meta => Code::Key(KEY_COMMAND),
        Key::PlayPause => Code::Media(MEDIA_PLAY),
        Key::NextTrack => Code::Media(MEDIA_FAST),
        Key::PreviousTrack => Code::Media(MEDIA_REWIND),
        Key::VolumeUp => Code::Media(MEDIA_SOUND_UP),
        Key::VolumeDown => Code::Media(MEDIA_SOUND_DOWN),
        Key::Mute => Code::Media(MEDIA_MUTE),
    }
}

fn event_error() -> io::Error {
    io::Error::other("could not create an input event")
}

fn cursor_position() -> CGPoint {
    let event = CGEvent::new(None);
    CGEvent::location(event.as_deref())
}

/// Lets the pointer cross onto any display, but stops it at the outer edges.
fn keep_on_screen(from: CGPoint, to: CGPoint) -> CGPoint {
    let displays = active_displays();
    if displays.iter().any(|bounds| contains(bounds, to)) {
        return to;
    }
    let Some(bounds) = displays.iter().find(|bounds| contains(bounds, from)).or(displays.first())
    else {
        return to;
    };
    CGPoint::new(
        to.x.clamp(bounds.origin.x, bounds.origin.x + bounds.size.width - 1.0),
        to.y.clamp(bounds.origin.y, bounds.origin.y + bounds.size.height - 1.0),
    )
}

fn contains(bounds: &CGRect, point: CGPoint) -> bool {
    point.x >= bounds.origin.x
        && point.y >= bounds.origin.y
        && point.x < bounds.origin.x + bounds.size.width
        && point.y < bounds.origin.y + bounds.size.height
}

fn active_displays() -> Vec<CGRect> {
    let mut ids: [CGDirectDisplayID; MAX_DISPLAYS] = [0; MAX_DISPLAYS];
    let mut count = 0;
    // SAFETY: both pointers are valid and `ids` has room for MAX_DISPLAYS entries.
    unsafe { CGGetActiveDisplayList(MAX_DISPLAYS as u32, ids.as_mut_ptr(), &mut count) };
    ids[..count as usize].iter().map(|&id| CGDisplayBounds(id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> CGRect {
        CGRect::new(CGPoint::new(x, y), objc2_core_foundation::CGSize::new(width, height))
    }

    #[test]
    fn contains_uses_half_open_bounds() {
        let bounds = rect(0.0, 0.0, 100.0, 50.0);
        assert!(contains(&bounds, CGPoint::new(0.0, 0.0)));
        assert!(contains(&bounds, CGPoint::new(99.0, 49.0)));
        assert!(!contains(&bounds, CGPoint::new(100.0, 10.0)));
    }
}
