//! The window for buying Couchpad and entering a license key.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{NSButton, NSStackView, NSTextField, NSWindow};
use objc2_foundation::NSString;

use super::license::{self, Status};
use super::{permissions, ui};

const WIDTH: f64 = 380.0;
const REFRESH_INTERVAL: Duration = Duration::from_millis(300);
/// After the trial, pressing the controller brings the window back at most this often.
const NAG_INTERVAL: Duration = Duration::from_secs(60);

struct Window {
    window: Retained<NSWindow>,
    title: Retained<NSTextField>,
    detail: Retained<NSTextField>,
    buy: Retained<NSButton>,
    entry: Retained<NSStackView>,
    key: Retained<NSTextField>,
    message: Retained<NSTextField>,
    refreshed: Instant,
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
    static LAST_NAG: RefCell<Option<Instant>> = const { RefCell::new(None) };
}

pub fn show(mtm: MainThreadMarker) {
    WINDOW.with_borrow_mut(|slot| {
        let window = slot.get_or_insert_with(|| build(mtm));
        refresh_window(window);
        ui::present(mtm, &window.window);
    });
}

/// Shows the window when the trial is over and the controller is used, at most once a minute.
pub fn nag(mtm: MainThreadMarker) {
    let due = LAST_NAG.with_borrow_mut(|last| {
        let due = last.is_none_or(|at| at.elapsed() >= NAG_INTERVAL);
        if due {
            *last = Some(Instant::now());
        }
        due
    });
    if due {
        show(mtm);
    }
}

pub fn buy() {
    permissions::open(license::BUY_URL);
}

pub fn activate() {
    WINDOW.with_borrow(|slot| {
        if let Some(window) = slot {
            license::activate(window.key.stringValue().to_string());
        }
    });
}

fn build(mtm: MainThreadMarker) -> Window {
    let title = ui::bold(mtm, "", 18.0);
    let detail = ui::secondary(mtm, "", WIDTH);
    let buy = ui::button(mtm, &format!("Buy Couchpad – {}", license::PRICE), sel!(buyLicense:));
    buy.setKeyEquivalent(&NSString::from_str("\r"));

    let key = NSTextField::textFieldWithString(&NSString::new(), mtm);
    key.setPlaceholderString(Some(&NSString::from_str("License key from your purchase email")));
    ui::fixed_width(&key, WIDTH - 100.0);
    let entry = ui::hstack(mtm, &[&key, &ui::button(mtm, "Activate", sel!(activateLicense:))], 8.0);
    let message = ui::secondary(mtm, "", WIDTH);

    let content = ui::centered_vstack(
        mtm,
        &[&ui::app_icon(mtm, 72.0), &title, &detail, &buy, &ui::separator(mtm, WIDTH), &entry, &message],
        12.0,
    );
    let window = ui::window(mtm, "Couchpad License", &content);
    Window { window, title, detail, buy, entry, key, message, refreshed: Instant::now() }
}

/// Called on the main thread every few milliseconds; updates while an activation runs.
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
    let status = license::status();
    let (title, detail) = match status {
        Status::Licensed => (
            "Couchpad is yours".to_string(),
            format!(
                "Thank you for buying Couchpad! License {} is active on this Mac.",
                license::masked_key().unwrap_or_default()
            ),
        ),
        Status::Trial { days_left } => (
            format!("{days_left} {} left in your trial", if days_left == 1 { "day" } else { "days" }),
            format!(
                "Couchpad is free to try for 7 days. After that it costs {} once, with no subscription.",
                license::PRICE
            ),
        ),
        Status::Expired => (
            "Your trial has ended".to_string(),
            format!(
                "Buy Couchpad for {} to keep using your controller as a mouse. Already bought it? Enter your key below.",
                license::PRICE
            ),
        ),
    };
    window.title.setStringValue(&NSString::from_str(&title));
    window.detail.setStringValue(&NSString::from_str(&detail));
    let licensed = status == Status::Licensed;
    window.buy.setHidden(licensed);
    window.entry.setHidden(licensed);
    window.message.setStringValue(&NSString::from_str(&license::message().unwrap_or_default()));
}
