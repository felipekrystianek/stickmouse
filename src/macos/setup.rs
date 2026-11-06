//! The setup window: shown on first launch, and whenever Accessibility access is missing.
//! It checks what Stickmouse needs live, links straight to the right System Settings pane,
//! and lets the user buy Stickmouse or enter a license key right away.

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{
    NSButton, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSImageView, NSStackView, NSTextField, NSWindow,
};
use objc2_foundation::NSString;

use super::license::{self, Status};
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
    license: Row,
    entry: Retained<NSStackView>,
    key: Retained<NSTextField>,
    message: Retained<NSTextField>,
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

/// Shows the window on first launch or when Stickmouse cannot control the pointer.
/// Returns whether the window was shown.
pub fn show_if_needed(mtm: MainThreadMarker) -> bool {
    let granted = permissions::accessibility_granted();
    eprintln!(
        "stickmouse: Accessibility access {}",
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

pub fn activate() {
    WINDOW.with_borrow(|slot| {
        if let Some(window) = slot {
            license::activate(window.key.stringValue().to_string());
        }
    });
}

pub fn finish() {
    if let Err(error) = fs::write(done_file(), "") {
        eprintln!("stickmouse: could not remember finished setup: {error}");
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
    let (access, access_row) = row(mtm, "Accessibility", ui::button(mtm, "Open Settings", sel!(openAccessibility:)));
    let (pad, pad_row) = row(mtm, "Controller", ui::button(mtm, "Bluetooth Settings", sel!(openBluetooth:)));
    let (license, license_row) =
        row(mtm, "License", ui::button(mtm, &format!("Buy – {}", license::PRICE), sel!(buyLicense:)));
    let key = NSTextField::textFieldWithString(&NSString::new(), mtm);
    key.setPlaceholderString(Some(&NSString::from_str("Already bought it? Paste your license key")));
    ui::fixed_width(&key, ROW_WIDTH - 100.0);
    let entry = ui::hstack(mtm, &[&key, &ui::button(mtm, "Activate", sel!(activateSetupLicense:))], 8.0);
    let message = ui::secondary(mtm, "", ROW_WIDTH);

    let login = ui::checkbox(mtm, "Start Stickmouse when I log in", sel!(toggleLogin:));
    let restart = ui::hstack(
        mtm,
        &[
            &ui::secondary(mtm, "Access is on. Stickmouse needs to restart to use it.", 300.0),
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
            &ui::bold(mtm, "Welcome to Stickmouse", 20.0),
            &ui::secondary(
                mtm,
                "Use your controller as a mouse and media remote. Give Stickmouse one permission from macOS, connect your controller, and you are ready. Try it free for 7 days or buy it now.",
                ROW_WIDTH,
            ),
            &ui::separator(mtm, ROW_WIDTH),
            &access_row,
            &pad_row,
            &license_row,
            &entry,
            &message,
            &ui::separator(mtm, ROW_WIDTH),
            &login,
            &restart,
            &done,
        ],
        14.0,
    );
    let window = ui::window(mtm, "Set Up Stickmouse", &content);
    Window { window, access, pad, license, entry, key, message, login, restart, refreshed: Instant::now() }
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
        ("arrow.clockwise.circle.fill", NSColor::systemOrangeColor(), "Allowed. Restart Stickmouse to finish.")
    } else if granted {
        (
            "checkmark.circle.fill",
            NSColor::systemGreenColor(),
            "Allowed. Stickmouse can move the pointer, click and press keys.",
        )
    } else {
        (
            "exclamationmark.triangle.fill",
            NSColor::systemOrangeColor(),
            "Needed to move the pointer, click and press keys. Click Open Settings and switch on Stickmouse.",
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

    let status = license::status();
    let (symbol, color, text) = match status {
        Status::Licensed => (
            "checkmark.circle.fill",
            NSColor::systemGreenColor(),
            format!("Licensed. Key {} is active on this Mac.", license::masked_key().unwrap_or_default()),
        ),
        Status::Trial { days_left } => (
            "clock.fill",
            NSColor::systemBlueColor(),
            format!(
                "Free trial: {days_left} {} left. Then {} once, no subscription.",
                if days_left == 1 { "day" } else { "days" },
                license::PRICE
            ),
        ),
        Status::Expired => (
            "exclamationmark.triangle.fill",
            NSColor::systemOrangeColor(),
            format!("The trial has ended. Buy Stickmouse for {} to keep using it.", license::PRICE),
        ),
    };
    let licensed = status == Status::Licensed;
    set(&window.license, symbol, &color, &text, !licensed);
    window.entry.setHidden(licensed);
    let message = license::message().unwrap_or_default();
    window.message.setStringValue(&NSString::from_str(&message));
    window.message.setHidden(message.is_empty());

    window.login.setState(if permissions::login_item_enabled() {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    window.restart.setHidden(!restart);
    ui::fit(&window.window);
}

fn set(row: &Row, symbol: &str, color: &NSColor, text: &str, show_button: bool) {
    ui::set_symbol(&row.icon, symbol, color);
    row.detail.setStringValue(&NSString::from_str(text));
    row.button.setHidden(!show_button);
}
