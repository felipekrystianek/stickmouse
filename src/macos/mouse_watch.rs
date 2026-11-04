//! Notices when a real mouse or trackpad moves, so the steering circle can get out of the way.

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use objc2_core_foundation::{CFMachPort, CFRunLoop, kCFRunLoopCommonModes};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventMask, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType,
};

use super::output::EVENT_MARK;
use crate::Message;
use crate::engine::Input;

const RETRY_INTERVAL: Duration = Duration::from_secs(5);
const REPORT_INTERVAL: Duration = Duration::from_millis(100);

struct Watch {
    messages: Sender<Message>,
    last_report: Cell<Option<Instant>>,
    tap: Cell<*const CFMachPort>,
}

pub fn spawn(messages: Sender<Message>) {
    thread::spawn(move || run(messages));
}

fn run(messages: Sender<Message>) {
    // Lives for the whole process; the tap callback reads it from its own run loop.
    let watch: &'static Watch = Box::leak(Box::new(Watch {
        messages,
        last_report: Cell::new(None),
        tap: Cell::new(std::ptr::null()),
    }));
    let mask: CGEventMask = [
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDragged,
        CGEventType::OtherMouseDragged,
    ]
    .iter()
    .fold(0, |mask, kind| mask | 1 << kind.0);

    let tap = loop {
        // SAFETY: `callback` matches the expected signature and `watch` outlives the tap.
        let tap = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::SessionEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                mask,
                Some(callback),
                watch as *const Watch as *mut c_void,
            )
        };
        match tap {
            Some(tap) => break tap,
            None => {
                eprintln!("stickmouse: cannot watch the real mouse yet, waiting for Accessibility access");
                thread::sleep(RETRY_INTERVAL);
            }
        }
    };
    watch.tap.set(&*tap);
    let Some(source) = CFMachPort::new_run_loop_source(None, Some(&tap), 0) else {
        eprintln!("stickmouse: cannot watch the real mouse");
        return;
    };
    let Some(run_loop) = CFRunLoop::current() else {
        return;
    };
    // SAFETY: reading an immutable framework constant.
    run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
    CGEvent::tap_enable(&tap, true);
    CFRunLoop::run();
}

unsafe extern "C-unwind" fn callback(
    _proxy: CGEventTapProxy,
    kind: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: `user_info` is the leaked `Watch` and only this run loop's thread touches it.
    let watch = unsafe { &*(user_info as *const Watch) };
    if kind == CGEventType::TapDisabledByTimeout || kind == CGEventType::TapDisabledByUserInput {
        // SAFETY: the tap is kept alive by `run`, which never returns while the run loop runs.
        if let Some(tap) = unsafe { watch.tap.get().as_ref() } {
            CGEvent::tap_enable(tap, true);
        }
        return event.as_ptr();
    }
    // SAFETY: the event is valid for the duration of the callback.
    let mark = CGEvent::integer_value_field(Some(unsafe { event.as_ref() }), CGEventField::EventSourceUserData);
    let due = watch.last_report.get().is_none_or(|at| at.elapsed() >= REPORT_INTERVAL);
    if mark != EVENT_MARK && due {
        watch.last_report.set(Some(Instant::now()));
        let _ = watch.messages.send(Message::Input(Input::RealMouseMoved));
    }
    event.as_ptr()
}
