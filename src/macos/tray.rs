//! The menu bar icon. The worker thread updates shared state; the main thread redraws the menu
//! when that state changes, because AppKit may only be touched from the main thread.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSCompositingOperation, NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu,
    NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use super::{license, ui, updater};
use crate::Message;

pub struct Tray {
    pub enabled: bool,
    pub connected: bool,
    pub battery: Option<String>,
}

impl Tray {
    fn status(&self) -> String {
        match (self.connected, self.enabled) {
            (false, _) => "Controller not connected".to_string(),
            (true, false) => "Off".to_string(),
            (true, true) => "On".to_string(),
        }
    }

    fn battery_text(&self) -> String {
        format!("Battery: {}", self.battery.as_deref().unwrap_or("unknown"))
    }
}

struct Shared {
    tray: Mutex<Tray>,
    version: AtomicU64,
    messages: Sender<Message>,
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

pub struct Handle(Arc<Shared>);

impl Handle {
    pub fn update(&self, change: impl FnOnce(&mut Tray)) {
        change(&mut self.0.tray.lock().unwrap());
        self.0.version.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn spawn(enabled: bool, messages: Sender<Message>) -> Option<Handle> {
    let shared = Arc::new(Shared {
        tray: Mutex::new(Tray { enabled, connected: false, battery: None }),
        version: AtomicU64::new(1),
        messages,
    });
    SHARED.set(shared.clone()).ok()?;
    Some(Handle(shared))
}

/// Sends a message to the Stickmouse loop on the worker thread.
pub fn send(message: Message) {
    if let Some(shared) = SHARED.get() {
        let _ = shared.messages.send(message);
    }
}

struct StatusBar {
    item: Retained<NSStatusItem>,
    seen: u64,
    license: Option<license::Status>,
    /// Whether Sparkle has started, so the menu gains "Check for Updates…" once it has.
    updater: bool,
}

thread_local! {
    static STATUS_BAR: RefCell<Option<StatusBar>> = const { RefCell::new(None) };
}

/// Called on the main thread every few milliseconds.
pub fn refresh(mtm: MainThreadMarker) {
    let Some(shared) = SHARED.get() else {
        return;
    };
    STATUS_BAR.with_borrow_mut(|bar| {
        let bar = bar.get_or_insert_with(|| StatusBar {
            item: NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength),
            seen: 0,
            license: None,
            updater: false,
        });
        let version = shared.version.load(Ordering::Relaxed);
        let status = license::status();
        let updater = updater::available();
        if version == bar.seen && bar.license == Some(status) && bar.updater == updater {
            return;
        }
        bar.seen = version;
        bar.license = Some(status);
        bar.updater = updater;
        let tray = shared.tray.lock().unwrap();
        redraw(mtm, bar, &tray);
    });
}

fn redraw(mtm: MainThreadMarker, bar: &StatusBar, tray: &Tray) {
    let active = tray.enabled && tray.connected;
    if let Some(button) = bar.item.button(mtm) {
        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(if active { "gamecontroller.fill" } else { "gamecontroller" }),
            Some(&NSString::from_str("Stickmouse")),
        );
        // While Stickmouse steers the pointer, the icon flips: a solid badge with the controller
        // cut out of it, so a glance at the menu bar shows that it is on.
        if let Some(image) = image.map(|image| if active { inverted(image) } else { image }) {
            image.setTemplate(true);
            button.setImage(Some(&image));
        }
        button.setToolTip(Some(&NSString::from_str(&format!(
            "stickmouse: {}\n{}",
            tray.status(),
            tray.battery_text()
        ))));
    }

    let menu = NSMenu::new(mtm);
    menu.addItem(&item(mtm, &tray.status(), None, ""));
    menu.addItem(&item(mtm, &tray.battery_text(), None, ""));
    match license::status() {
        license::Status::Licensed => {}
        license::Status::Trial { days_left } => {
            menu.addItem(&item(mtm, &format!("Trial: {days_left} days left"), None, ""));
        }
        license::Status::Expired => menu.addItem(&item(mtm, "Trial ended", None, "")),
    }
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let enabled = item(mtm, "Enabled", Some(sel!(toggle:)), "");
    enabled.setState(if tray.enabled { NSControlStateValueOn } else { NSControlStateValueOff });
    menu.addItem(&enabled);
    menu.addItem(&item(mtm, "Settings…", Some(sel!(openSettings:)), ","));
    menu.addItem(&item(mtm, "Customize Buttons…", Some(sel!(openButtons:)), ""));
    menu.addItem(&item(mtm, "Setup & Permissions…", Some(sel!(openSetup:)), ""));
    let license_title = if license::status() == license::Status::Licensed { "License…" } else { "Buy Stickmouse…" };
    menu.addItem(&item(mtm, license_title, Some(sel!(openLicense:)), ""));
    if bar.updater {
        menu.addItem(&item(mtm, "Check for Updates…", Some(sel!(checkForUpdates:)), ""));
    }
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&item(mtm, "Quit Stickmouse", Some(sel!(quit:)), "q"));
    bar.item.setMenu(Some(&menu));
}

/// `symbol` knocked out of a rounded badge. Drawn in black; as a template image the menu bar
/// recolors it for light and dark mode.
fn inverted(symbol: Retained<NSImage>) -> Retained<NSImage> {
    let block = RcBlock::new(move |rect: NSRect| -> Bool {
        NSColor::blackColor().setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, 4.0, 4.0).fill();
        let size = symbol.size();
        let glyph = NSRect::new(
            NSPoint::new(
                rect.origin.x + (rect.size.width - size.width) / 2.0,
                rect.origin.y + (rect.size.height - size.height) / 2.0,
            ),
            size,
        );
        symbol.drawInRect_fromRect_operation_fraction(glyph, NSRect::ZERO, NSCompositingOperation::DestinationOut, 1.0);
        Bool::YES
    });
    // A little wider than the controller glyph, and as tall as menu bar icons usually are.
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(26.0, 18.0), false, &block)
}

/// An item without an action shows greyed out, which suits the status lines.
fn item(mtm: MainThreadMarker, title: &str, action: Option<Sel>, key: &str) -> Retained<NSMenuItem> {
    // SAFETY: the selectors exist on ui::Actions, which lives for the whole process.
    unsafe {
        let item = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key),
        );
        if action.is_some() {
            item.setTarget(Some(&ui::actions(mtm)));
        }
        item
    }
}
