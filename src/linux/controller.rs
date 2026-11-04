//! Finds the Xbox controller, reads its events and handles grabbing and rumble.

use std::collections::HashMap;
use std::io;
use std::os::fd::AsRawFd;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

use evdev::{
    AbsInfo, AbsoluteAxisCode, Device, EventSummary, FFEffectData, FFEffectKind, FFReplay,
    FFTrigger, KeyCode,
};

use crate::Message;
use crate::engine::{Axis, Button, Input};

const MICROSOFT_VENDOR: u16 = 0x045E;
const RESCAN_INTERVAL: Duration = Duration::from_secs(2);
const POLL_TIMEOUT_MS: i32 = 20;
const RUMBLE_LENGTH_MS: u16 = 150;
// The Linux names for these are confusing: BTN_X is BTN_NORTH and BTN_Y is BTN_WEST, and
// both xpad and xpadneo report the Xbox X and Y buttons as BTN_X and BTN_Y.
const BTN_X: KeyCode = KeyCode::BTN_NORTH;
const BTN_Y: KeyCode = KeyCode::BTN_WEST;

pub enum Command {
    Grab(bool),
    Rumble,
}

pub fn spawn(messages: Sender<Message>, grab: bool) -> Sender<Command> {
    let (commands_tx, commands_rx) = channel();
    thread::spawn(move || run(messages, commands_rx, grab));
    commands_tx
}

fn run(messages: Sender<Message>, commands: Receiver<Command>, mut grab: bool) {
    loop {
        for command in commands.try_iter() {
            if let Command::Grab(value) = command {
                grab = value;
            }
        }
        let Some(device) = find_controller() else {
            thread::sleep(RESCAN_INTERVAL);
            continue;
        };
        if let Err(error) = serve(device, &messages, &commands, &mut grab) {
            eprintln!("padcursor: controller disconnected: {error}");
        }
        if messages.send(Message::Connected(false)).is_err()
            || messages.send(Message::Input(Input::Disconnected)).is_err()
        {
            return;
        }
        thread::sleep(RESCAN_INTERVAL);
    }
}

fn find_controller() -> Option<Device> {
    evdev::enumerate().map(|(_, device)| device).find(is_controller)
}

fn is_controller(device: &Device) -> bool {
    let has_keys = device
        .supported_keys()
        .is_some_and(|keys| keys.contains(KeyCode::BTN_SOUTH) && keys.contains(KeyCode::BTN_MODE));
    let has_stick = device
        .supported_absolute_axes()
        .is_some_and(|axes| axes.contains(AbsoluteAxisCode::ABS_X));
    device.input_id().vendor() == MICROSOFT_VENDOR && has_keys && has_stick
}

fn serve(
    mut device: Device,
    messages: &Sender<Message>,
    commands: &Receiver<Command>,
    grab: &mut bool,
) -> io::Result<()> {
    device.set_nonblocking(true)?;
    if *grab {
        device.grab()?;
    }
    let ranges: HashMap<u16, AbsInfo> = device
        .get_absinfo()?
        .map(|(code, info)| (code.0, info))
        .collect();
    eprintln!("padcursor: using controller {:?}", device.name().unwrap_or("unknown"));
    let _ = messages.send(Message::Connected(true));

    loop {
        for command in commands.try_iter() {
            match command {
                Command::Grab(true) if !*grab => {
                    device.grab()?;
                    *grab = true;
                }
                Command::Grab(false) if *grab => {
                    device.ungrab()?;
                    *grab = false;
                }
                Command::Grab(_) => {}
                Command::Rumble => {
                    if let Err(error) = rumble(&mut device) {
                        eprintln!("padcursor: rumble failed: {error}");
                    }
                }
            }
        }

        let mut poll = libc::pollfd { fd: device.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: `poll` points to one valid pollfd for the duration of the call.
        if unsafe { libc::poll(&mut poll, 1, POLL_TIMEOUT_MS) } < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
            continue;
        }

        match device.fetch_events() {
            Ok(events) => {
                for event in events {
                    if let Some(input) = translate(event.destructure(), &ranges)
                        && messages.send(Message::Input(input)).is_err()
                    {
                        return Ok(());
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(error),
        }
    }
}

fn rumble(device: &mut Device) -> io::Result<()> {
    let mut effect = device.upload_ff_effect(FFEffectData {
        direction: 0,
        trigger: FFTrigger::default(),
        replay: FFReplay { length: RUMBLE_LENGTH_MS, delay: 0 },
        kind: FFEffectKind::Rumble { strong_magnitude: 0x8000, weak_magnitude: 0x8000 },
    })?;
    effect.play(1)?;
    // Dropping the effect removes it from the controller, which would cut the rumble short.
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(RUMBLE_LENGTH_MS.into()) * 2);
        drop(effect);
    });
    Ok(())
}

fn translate(event: EventSummary, ranges: &HashMap<u16, AbsInfo>) -> Option<Input> {
    match event {
        EventSummary::Key(_, code, value) if value != 2 => {
            map_button(code).map(|button| Input::Button(button, value == 1))
        }
        EventSummary::AbsoluteAxis(_, code, value) => {
            let axis = map_axis(code)?;
            let info = ranges.get(&code.0)?;
            Some(Input::Axis(axis, normalize(axis, value, info.minimum(), info.maximum())))
        }
        _ => None,
    }
}

fn map_button(code: KeyCode) -> Option<Button> {
    Some(match code {
        KeyCode::BTN_SOUTH => Button::A,
        KeyCode::BTN_EAST => Button::B,
        BTN_X => Button::X,
        BTN_Y => Button::Y,
        KeyCode::BTN_TL => Button::LeftBumper,
        KeyCode::BTN_TR => Button::RightBumper,
        KeyCode::BTN_SELECT => Button::View,
        KeyCode::BTN_START => Button::Menu,
        KeyCode::BTN_MODE => Button::Xbox,
        KeyCode::BTN_THUMBL => Button::LeftStickClick,
        KeyCode::BTN_THUMBR => Button::RightStickClick,
        _ => return None,
    })
}

fn map_axis(code: AbsoluteAxisCode) -> Option<Axis> {
    Some(match code {
        AbsoluteAxisCode::ABS_X => Axis::LeftX,
        AbsoluteAxisCode::ABS_Y => Axis::LeftY,
        AbsoluteAxisCode::ABS_RX => Axis::RightX,
        AbsoluteAxisCode::ABS_RY => Axis::RightY,
        AbsoluteAxisCode::ABS_Z => Axis::LeftTrigger,
        AbsoluteAxisCode::ABS_RZ => Axis::RightTrigger,
        AbsoluteAxisCode::ABS_HAT0X => Axis::DpadX,
        AbsoluteAxisCode::ABS_HAT0Y => Axis::DpadY,
        _ => return None,
    })
}

fn normalize(axis: Axis, value: i32, minimum: i32, maximum: i32) -> f32 {
    let span = (maximum - minimum).max(1) as f32;
    let unit = ((value - minimum) as f32 / span).clamp(0.0, 1.0);
    match axis {
        Axis::LeftTrigger | Axis::RightTrigger => unit,
        _ => unit * 2.0 - 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xbox_buttons_use_the_codes_xpadneo_reports() {
        // Values from the kernel's input-event-codes.h, as used by xpadneo's mappings.c.
        let cases = [
            (0x130, Button::A),
            (0x131, Button::B),
            (0x133, Button::X),
            (0x134, Button::Y),
            (0x136, Button::LeftBumper),
            (0x137, Button::RightBumper),
            (0x13a, Button::View),
            (0x13b, Button::Menu),
            (0x13c, Button::Xbox),
            (0x13d, Button::LeftStickClick),
            (0x13e, Button::RightStickClick),
        ];
        for (code, button) in cases {
            assert_eq!(map_button(KeyCode(code)), Some(button));
        }
    }

    #[test]
    fn sticks_map_to_minus_one_through_one() {
        assert_eq!(normalize(Axis::LeftX, 0, 0, 65535), -1.0);
        assert_eq!(normalize(Axis::LeftX, 65535, 0, 65535), 1.0);
        assert!(normalize(Axis::LeftX, 32768, 0, 65535).abs() < 0.001);
    }

    #[test]
    fn triggers_map_to_zero_through_one() {
        assert_eq!(normalize(Axis::RightTrigger, 0, 0, 1023), 0.0);
        assert_eq!(normalize(Axis::RightTrigger, 1023, 0, 1023), 1.0);
    }

    #[test]
    fn dpad_keeps_its_direction() {
        assert_eq!(normalize(Axis::DpadY, -1, -1, 1), -1.0);
        assert_eq!(normalize(Axis::DpadY, 0, -1, 1), 0.0);
        assert_eq!(normalize(Axis::DpadY, 1, -1, 1), 1.0);
    }
}
