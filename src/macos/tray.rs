//! The menu bar icon. The worker thread updates shared state; the main thread redraws the menu
//! when that state changes, because AppKit may only be touched from the main thread.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu, NSMenuItem, NSStatusBar,
    NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSObject, NSString};

use crate::Message;
use crate::engine::Input;

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

fn send(message: Message) {
    if let Some(shared) = SHARED.get() {
        let _ = shared.messages.send(message);
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and we add no ivars.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PadcursorMenuTarget"]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(toggle:))]
        fn toggle(&self, _sender: Option<&AnyObject>) {
            send(Message::Input(Input::ToggleEnabled));
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            send(Message::OpenSettings);
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            send(Message::Quit);
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's init takes no arguments.
        unsafe { msg_send![super(this), init] }
    }
}

struct StatusBar {
    item: Retained<NSStatusItem>,
    target: Retained<MenuTarget>,
    seen: u64,
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
            target: MenuTarget::new(mtm),
            seen: 0,
        });
        let version = shared.version.load(Ordering::Relaxed);
        if version == bar.seen {
            return;
        }
        bar.seen = version;
        let tray = shared.tray.lock().unwrap();
        redraw(mtm, bar, &tray);
    });
}

fn redraw(mtm: MainThreadMarker, bar: &StatusBar, tray: &Tray) {
    let active = tray.enabled && tray.connected;
    if let Some(button) = bar.item.button(mtm) {
        let symbol = if active { "gamecontroller.fill" } else { "gamecontroller" };
        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(symbol),
            Some(&NSString::from_str("padcursor")),
        );
        if let Some(image) = image {
            image.setTemplate(true);
            button.setImage(Some(&image));
        }
        button.setToolTip(Some(&NSString::from_str(&format!(
            "padcursor: {}\n{}",
            tray.status(),
            tray.battery_text()
        ))));
    }

    let menu = NSMenu::new(mtm);
    menu.addItem(&item(mtm, &tray.status(), None, None));
    menu.addItem(&item(mtm, &tray.battery_text(), None, None));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let enabled = item(mtm, "Enabled", Some(sel!(toggle:)), Some(&bar.target));
    enabled.setState(if tray.enabled { NSControlStateValueOn } else { NSControlStateValueOff });
    menu.addItem(&enabled);
    menu.addItem(&item(mtm, "Open settings", Some(sel!(openSettings:)), Some(&bar.target)));
    menu.addItem(&item(mtm, "Quit", Some(sel!(quit:)), Some(&bar.target)));
    bar.item.setMenu(Some(&menu));
}

/// An item without an action shows greyed out, which suits the status lines.
fn item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    target: Option<&MenuTarget>,
) -> Retained<NSMenuItem> {
    // SAFETY: the selectors exist on MenuTarget, which outlives the menu.
    unsafe {
        let item = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::new(),
        );
        if let Some(target) = target {
            item.setTarget(Some(target));
        }
        item
    }
}
