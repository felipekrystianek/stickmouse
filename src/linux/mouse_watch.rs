//! Watches real mice so the circle can give way as soon as one is touched.

use std::io;
use std::os::fd::AsRawFd;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use evdev::{Device, EventSummary, KeyCode, RelativeAxisCode};

use super::output::DEVICE_PREFIX;
use crate::Message;
use crate::engine::Input;

const RESCAN_INTERVAL: Duration = Duration::from_secs(5);
const REPORT_INTERVAL: Duration = Duration::from_millis(100);

pub fn spawn(messages: Sender<Message>) {
    thread::spawn(move || run(messages));
}

fn run(messages: Sender<Message>) {
    let mut last_report = Instant::now() - REPORT_INTERVAL;
    loop {
        let mut mice = find_mice();
        let rescan_at = Instant::now() + RESCAN_INTERVAL;
        while Instant::now() < rescan_at {
            if wait(&mice).is_err() {
                break;
            }
            let moved = mice.iter_mut().any(drain_moved);
            if moved && last_report.elapsed() >= REPORT_INTERVAL {
                last_report = Instant::now();
                if messages.send(Message::Input(Input::RealMouseMoved)).is_err() {
                    return;
                }
            }
        }
    }
}

fn find_mice() -> Vec<Device> {
    evdev::enumerate()
        .map(|(_, device)| device)
        .filter(is_real_mouse)
        .filter(|device| device.set_nonblocking(true).is_ok())
        .collect()
}

fn is_real_mouse(device: &Device) -> bool {
    // Skips our own virtual mouse and the mouse device xpadneo creates for the controller.
    if device.name().is_some_and(|name| name.starts_with(DEVICE_PREFIX) || name.contains("Xbox")) {
        return false;
    }
    let moves = device.supported_relative_axes().is_some_and(|axes| {
        axes.contains(RelativeAxisCode::REL_X) && axes.contains(RelativeAxisCode::REL_Y)
    });
    let clicks = device.supported_keys().is_some_and(|keys| keys.contains(KeyCode::BTN_LEFT));
    moves && clicks
}

fn wait(mice: &[Device]) -> io::Result<()> {
    let mut polls: Vec<libc::pollfd> = mice
        .iter()
        .map(|mouse| libc::pollfd { fd: mouse.as_raw_fd(), events: libc::POLLIN, revents: 0 })
        .collect();
    if polls.is_empty() {
        thread::sleep(RESCAN_INTERVAL);
        return Err(io::ErrorKind::NotFound.into());
    }
    // SAFETY: the pointer and length describe the `polls` vector, which outlives the call.
    let ready = unsafe {
        libc::poll(polls.as_mut_ptr(), polls.len() as libc::nfds_t, RESCAN_INTERVAL.as_millis() as i32)
    };
    if ready < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

fn drain_moved(mouse: &mut Device) -> bool {
    let Ok(events) = mouse.fetch_events() else {
        return false;
    };
    let mut moved = false;
    for event in events {
        moved |= matches!(event.destructure(), EventSummary::RelativeAxis(..));
    }
    moved
}
