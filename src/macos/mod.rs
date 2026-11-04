//! Everything specific to macOS. AppKit and GameController live on the main thread, so the main
//! thread runs the app and a timer, while the platform-independent loop in `main.rs` runs on a
//! worker thread and talks to it through channels and shared state.

pub mod controller;
pub mod cursor_theme;
pub mod desktop;
pub mod mouse_watch;
pub mod output;
pub mod tray;

use std::process::ExitCode;
use std::ptr::NonNull;
use std::thread;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSTimer};
use objc2_game_controller::GCController;

/// How often the main thread reads the controller and refreshes the menu bar and circle.
const POLL_INTERVAL: f64 = 0.004;

/// Runs `work` on a worker thread while the main thread runs the AppKit event loop.
/// The process exits when `work` returns.
pub fn run_app(work: impl FnOnce() -> ExitCode + Send + 'static) -> ExitCode {
    let mtm = MainThreadMarker::new().expect("padcursor must start on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    // SAFETY: a plain class property; we never bring a window to the front, so without this the
    // controller would only be read while some other app is in front.
    unsafe { GCController::setShouldMonitorBackgroundEvents(true) };

    let block = RcBlock::new(|_timer: NonNull<NSTimer>| {
        let mtm = MainThreadMarker::new().expect("timers fire on the main thread");
        controller::poll();
        tray::refresh(mtm);
        cursor_theme::refresh(mtm);
    });
    // SAFETY: the block only touches main-thread state and the timer runs on the main run loop.
    unsafe {
        let timer = NSTimer::timerWithTimeInterval_repeats_block(POLL_INTERVAL, true, &block);
        NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
    }

    thread::spawn(move || {
        let code = work();
        std::process::exit(if code == ExitCode::SUCCESS { 0 } else { 1 });
    });
    app.run();
    ExitCode::SUCCESS
}
