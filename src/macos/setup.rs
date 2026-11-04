//! The setup window: shown on first launch, and whenever Accessibility access is missing.
//! It checks what padcursor needs live and links straight to the right System Settings pane.

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{
    NSButton, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSImageView, NSStackView,
    NSTextField, NSWindow,
};
use objc2_foundation::NSString;

use super::{controller, desktop, permissions, ui};

const ROW_WIDTH: f64 = 440.0;
const TEXT_WIDTH: f64 = 250.0;
const REFRESH_INTERVAL: Duration = Duration::from_millis(500);

struct Row {
    icon: Retained<NSImageView>,
    detail: Retained<NSTextField>,
    button: Retained<NSButton>,
}

struct Window {
    window: Retained<NSWindow>,
    access: Row,
    pad: Row,
    login: Retained<NSButton>,
    restart: Retained<NSStackView>,
    refreshed: Instant,
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
}

fn done_file() -> PathBuf {
    desktop::state_dir().join("setup-done")
}

/// Shows the window on first launch or when padcursor cannot control the pointer.
/// Returns whether the window was shown.
pub fn show_if_needed(mtm: MainThreadMarker) -> bool {
    let granted = permissions::accessibility_granted();
    eprintln!(
        "padcursor: Accessibility access {}",
        if granted { "granted" } else { "missing, the pointer will not move" }
    );
    let needed = !granted || !done_file().exists();
    if needed {
        show(mtm);
    }
    needed
}

pub fn show(mtm: MainThreadMarker) {
    WINDOW.with_borrow_mut(|slot| {
        let window = slot.get_or_insert_with(|| build(mtm));
        refresh_window(window);
        ui::present(mtm, &window.window);
    });
}

pub fn finish() {
    if let Err(error) = fs::write(done_file(), "") {
        eprintln!("padcursor: could not remember finished setup: {error}");
    }
    WINDOW.with_borrow(|slot| {
        if let Some(window) = slot {
            window.window.close();
        }
    });
}

fn row(mtm: MainThreadMarker, title: &str, button: Retained<NSButton>) -> (Row, Retained<NSStackView>) {
    let icon = ui::symbol(mtm, "circle.dashed", &NSColor::secondaryLabelColor(), 24.0);
    let detail = ui::secondary(mtm, "", TEXT_WIDTH);
    let text = ui::vstack(mtm, &[&ui::bold(mtm, title, 13.0), &detail], 2.0);
    let stack = ui::hstack(mtm, &[&icon, &text, &button], 12.0);
    ui::fixed_width(&stack, ROW_WIDTH);
    (Row { icon, detail, button }, stack)
}

fn build(mtm: MainThreadMarker) -> Window {
    let (access, access_row) =
        row(mtm, "Accessibility", ui::button(mtm, "Open Settings", sel!(openAccessibility:)));
    let (pad, pad_row) =
        row(mtm, "Controller", ui::button(mtm, "Bluetooth Settings", sel!(openBluetooth:)));

    let login = ui::checkbox(mtm, "Start Couchpad when I log in", sel!(toggleLogin:));
    let restart = ui::hstack(
        mtm,
        &[
            &ui::secondary(mtm, "Access is on. Couchpad needs to restart to use it.", 300.0),
            &ui::button(mtm, "Restart Now", sel!(restart:)),
        ],
        12.0,
    );
    let done = ui::button(mtm, "Done", sel!(finishSetup:));
    done.setKeyEquivalent(&NSString::from_str("\r"));

    let content = ui::centered_vstack(
        mtm,
        &[
            &ui::app_icon(mtm, 88.0),
            &ui::bold(mtm, "Welcome to Couchpad", 20.0),
            &ui::secondary(
                mtm,
                "Use your controller as a mouse and media remote. Couchpad needs one permission from macOS before it can move the pointer.",
                ROW_WIDTH,
            ),
            &ui::separator(mtm, ROW_WIDTH),
            &access_row,
            &pad_row,
            &ui::separator(mtm, ROW_WIDTH),
            &login,
            &restart,
            &done,
        ],
        14.0,
    );
    let window = ui::window(mtm, "Set Up Couchpad", &content);
    Window { window, access, pad, login, restart, refreshed: Instant::now() }
}

/// Called on the main thread every few milliseconds; checks twice a second while visible.
pub fn refresh() {
    WINDOW.with_borrow_mut(|slot| {
        if let Some(window) = slot.as_mut()
            && window.window.isVisible()
            && window.refreshed.elapsed() >= REFRESH_INTERVAL
        {
            window.refreshed = Instant::now();
            refresh_window(window);
        }
    });
}

fn refresh_window(window: &Window) {
    let granted = permissions::accessibility_granted();
    let restart = permissions::needs_restart();
    let (symbol, color, text) = if restart {
        ("arrow.clockwise.circle.fill", NSColor::systemOrangeColor(), "Allowed. Restart Couchpad to finish.")
    } else if granted {
        ("checkmark.circle.fill", NSColor::systemGreenColor(), "Allowed. Couchpad can move the pointer, click and press keys.")
    } else {
        (
            "exclamationmark.triangle.fill",
            NSColor::systemOrangeColor(),
            "Needed to move the pointer, click and press keys. Click Open Settings and switch on Couchpad.",
        )
    };
    set(&window.access, symbol, &color, text, !granted);

    match controller::name() {
        Some(name) => {
            let battery = desktop::battery_level().ok().flatten();
            let text = match battery {
                Some(level) => format!("{name} is connected. Battery: {level}."),
                None => format!("{name} is connected."),
            };
            set(&window.pad, "checkmark.circle.fill", &NSColor::systemGreenColor(), &text, false);
        }
        None => set(
            &window.pad,
            "circle.dashed",
            &NSColor::secondaryLabelColor(),
            "Not connected. Turn the controller on, or pair it in Bluetooth settings.",
            true,
        ),
    }

    window.login.setState(if permissions::login_item_enabled() {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    window.restart.setHidden(!restart);
}

fn set(row: &Row, symbol: &str, color: &NSColor, text: &str, show_button: bool) {
    ui::set_symbol(&row.icon, symbol, color);
    row.detail.setStringValue(&NSString::from_str(text));
    row.button.setHidden(!show_button);
}
