//! The steering circle. macOS does not let a background app change the system cursor, so the
//! circle is a small click-through window that follows the pointer.

use std::cell::RefCell;
use std::io;
use std::path::Path;
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::{MainThreadMarker, MainThreadOnly};
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSScreenSaverWindowLevel, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CFBoolean, CFString, CGPoint, CGRect, CGSize, kCFBooleanTrue};
use objc2_core_graphics::{CGDisplayHideCursor, CGDisplayShowCursor, CGMainDisplayID};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_quartz_core::CALayer;

/// Whether the circle should be showing. Written by the worker thread, read by the main thread.
pub static STEERING: AtomicBool = AtomicBool::new(false);
static SCALE: Mutex<f32> = Mutex::new(1.0);

/// Same proportions as the Linux cursor theme: a white ring and centre dot with dark outlines.
const NOMINAL_SIZE: f64 = 24.0;
const RING_RADIUS: f64 = 0.40;
const DOT_RADIUS: f64 = 0.07;
const STROKE: f64 = 0.09;
const OUTLINE: f64 = 1.0;

thread_local! {
    static OVERLAY: RefCell<Option<(Retained<NSWindow>, f64)>> = const { RefCell::new(None) };
}

// Private WindowServer calls. Without them macOS only lets the frontmost app hide the pointer,
// and Stickmouse is never frontmost. WindowServer shows the pointer again if Stickmouse exits.
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn _CGSDefaultConnection() -> i32;
    fn CGSSetConnectionProperty(
        connection: i32,
        target: i32,
        key: &CFString,
        value: &CFBoolean,
    ) -> i32;
}

fn allow_hiding_in_background() {
    static DONE: Once = Once::new();
    DONE.call_once(|| {
        // SAFETY: plain calls into WindowServer with valid CF objects.
        unsafe {
            let connection = _CGSDefaultConnection();
            let key = CFString::from_static_str("SetsCursorInBackground");
            if let Some(yes) = kCFBooleanTrue {
                CGSSetConnectionProperty(connection, connection, &key, yes);
            }
        }
    });
}

/// Changes the circle size; the window is rebuilt the next time it shows.
pub fn set_scale(scale: f32) {
    let mut current = SCALE.lock().unwrap();
    if *current != scale {
        *current = scale;
        OVERLAY.with_borrow_mut(|overlay| {
            if let Some((window, _)) = overlay.take() {
                window.orderOut(None);
            }
        });
    }
}

/// How often the arrow is hidden again while steering. Apps under a resting pointer can bring it
/// back, for example when they change the cursor shape, and macOS offers no reliable way to ask
/// whether it is visible (CGCursorIsVisible always answers false).
const REHIDE_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Default)]
struct PointerState {
    /// Hides sent since the arrow was last shown. WindowServer counts them, so the same number
    /// of shows brings the arrow back; shows beyond that are ignored.
    hides: u32,
    last_hide: Option<Instant>,
}

thread_local! {
    static POINTER: RefCell<PointerState> = RefCell::default();
}

fn set_pointer_hidden(hidden: bool) {
    POINTER.with_borrow_mut(|pointer| {
        if hidden {
            if pointer.last_hide.is_some_and(|at| at.elapsed() < REHIDE_INTERVAL) {
                return;
            }
            allow_hiding_in_background();
            CGDisplayHideCursor(CGMainDisplayID());
            pointer.hides += 1;
            pointer.last_hide = Some(Instant::now());
        } else {
            for _ in 0..std::mem::take(&mut pointer.hides) {
                CGDisplayShowCursor(CGMainDisplayID());
            }
            pointer.last_hide = None;
        }
    });
}

pub fn install(_icons_dir: &Path, scale: f32) -> io::Result<()> {
    *SCALE.lock().unwrap() = scale;
    Ok(())
}

/// Called on the main thread every few milliseconds.
pub fn refresh(mtm: MainThreadMarker) {
    let steering = STEERING.load(Ordering::Relaxed);
    OVERLAY.with_borrow_mut(|overlay| {
        if !steering {
            if let Some((window, _)) = overlay
                && window.isVisible()
            {
                window.orderOut(None);
            }
            set_pointer_hidden(false);
            return;
        }
        let (window, size) = overlay.get_or_insert_with(|| make_window(mtm));
        let pointer = NSEvent::mouseLocation();
        window.setFrameOrigin(NSPoint::new(pointer.x - *size / 2.0, pointer.y - *size / 2.0));
        if !window.isVisible() {
            window.orderFrontRegardless();
        }
        // The circle's centre dot takes the place of the arrow, as on Linux.
        set_pointer_hidden(true);
    });
}

fn make_window(mtm: MainThreadMarker) -> (Retained<NSWindow>, f64) {
    let size = (NOMINAL_SIZE * f64::from(*SCALE.lock().unwrap())).round().max(8.0);
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size, size)),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is kept in OVERLAY, so AppKit must not free it on close.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setHasShadow(false);
    window.setIgnoresMouseEvents(true);
    window.setLevel(NSScreenSaverWindowLevel);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );

    if let Some(view) = window.contentView() {
        view.setWantsLayer(true);
        if let Some(root) = view.layer() {
            let centre = size / 2.0;
            let ring = size * RING_RADIUS;
            let stroke = (size * STROKE).max(2.0);
            let dot = (size * DOT_RADIUS).max(1.5);
            let black = NSColor::blackColor();
            let white = NSColor::whiteColor();
            // Each layer draws its border inwards, so the outer edge sets the radius.
            root.addSublayer(&circle(centre, ring + stroke / 2.0 + OUTLINE, stroke + 2.0 * OUTLINE, &black, None));
            root.addSublayer(&circle(centre, ring + stroke / 2.0, stroke, &white, None));
            root.addSublayer(&circle(centre, dot + OUTLINE, 0.0, &black, Some(&black)));
            root.addSublayer(&circle(centre, dot, 0.0, &white, Some(&white)));
        }
    }
    (window, size)
}

fn circle(
    centre: f64,
    radius: f64,
    border: f64,
    border_color: &NSColor,
    fill: Option<&NSColor>,
) -> Retained<CALayer> {
    let layer = CALayer::new();
    layer.setFrame(CGRect::new(
        CGPoint::new(centre - radius, centre - radius),
        CGSize::new(radius * 2.0, radius * 2.0),
    ));
    layer.setCornerRadius(radius);
    layer.setBorderWidth(border);
    layer.setBorderColor(Some(&border_color.CGColor()));
    if let Some(fill) = fill {
        layer.setBackgroundColor(Some(&fill.CGColor()));
    }
    layer
}
