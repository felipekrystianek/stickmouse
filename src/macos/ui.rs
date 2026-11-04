//! Small AppKit building blocks shared by the menu, the settings window and the setup window,
//! plus the one Objective-C object that receives every button, slider and menu action.

use std::cell::OnceCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSBox, NSBoxType, NSButton, NSColor, NSFont, NSImage, NSImageView,
    NSLayoutAttribute, NSMenu, NSMenuItem, NSPopUpButton, NSSlider, NSStackView, NSTextAlignment, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use super::{settings, setup, tray};
use crate::Message;
use crate::engine::Input;

define_class!(
    // SAFETY: NSObject has no subclassing requirements and we add no ivars.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PadcursorActions"]
    pub struct Actions;

    impl Actions {
        #[unsafe(method(toggle:))]
        fn toggle(&self, _sender: Option<&AnyObject>) {
            tray::send(Message::Input(Input::ToggleEnabled));
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            tray::send(Message::Quit);
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            settings::show(self.mtm());
        }

        #[unsafe(method(openSetup:))]
        fn open_setup(&self, _sender: Option<&AnyObject>) {
            setup::show(self.mtm());
        }

        #[unsafe(method(sliderChanged:))]
        fn slider_changed(&self, sender: Option<&NSSlider>) {
            if let Some(slider) = sender {
                settings::slider_changed(slider);
            }
        }

        #[unsafe(method(restoreDefaults:))]
        fn restore_defaults(&self, _sender: Option<&AnyObject>) {
            settings::restore_defaults();
        }

        #[unsafe(method(openConfigFile:))]
        fn open_config_file(&self, _sender: Option<&AnyObject>) {
            settings::open_config_file();
        }

        #[unsafe(method(toggleLogin:))]
        fn toggle_login(&self, sender: Option<&NSButton>) {
            if let Some(button) = sender {
                super::permissions::set_login_item_from(button);
            }
        }

        #[unsafe(method(openAccessibility:))]
        fn open_accessibility(&self, _sender: Option<&AnyObject>) {
            super::permissions::open_accessibility_settings();
        }

        #[unsafe(method(openBluetooth:))]
        fn open_bluetooth(&self, _sender: Option<&AnyObject>) {
            super::permissions::open_bluetooth_settings();
        }

        #[unsafe(method(restart:))]
        fn restart(&self, _sender: Option<&AnyObject>) {
            super::permissions::restart();
        }

        #[unsafe(method(openButtons:))]
        fn open_buttons(&self, _sender: Option<&AnyObject>) {
            super::buttons_window::show(self.mtm());
        }

        #[unsafe(method(bindingChanged:))]
        fn binding_changed(&self, sender: Option<&NSPopUpButton>) {
            if let Some(view) = sender {
                super::buttons_window::binding_changed(self.mtm(), view);
            }
        }

        #[unsafe(method(stickChanged:))]
        fn stick_changed(&self, sender: Option<&NSPopUpButton>) {
            if let Some(view) = sender {
                super::buttons_window::stick_changed(view);
            }
        }

        #[unsafe(method(restoreButtons:))]
        fn restore_buttons(&self, _sender: Option<&AnyObject>) {
            super::buttons_window::restore_defaults(self.mtm());
        }

        #[unsafe(method(openLicense:))]
        fn open_license(&self, _sender: Option<&AnyObject>) {
            super::license_window::show(self.mtm());
        }

        #[unsafe(method(buyLicense:))]
        fn buy_license(&self, _sender: Option<&AnyObject>) {
            super::license_window::buy();
        }

        #[unsafe(method(activateLicense:))]
        fn activate_license(&self, _sender: Option<&AnyObject>) {
            super::license_window::activate();
        }

        #[unsafe(method(deactivateLicense:))]
        fn deactivate_license(&self, _sender: Option<&AnyObject>) {
            super::license::deactivate();
        }

        #[unsafe(method(openUpdate:))]
        fn open_update(&self, _sender: Option<&AnyObject>) {
            super::update::open_download();
        }

        #[unsafe(method(finishSetup:))]
        fn finish_setup(&self, _sender: Option<&AnyObject>) {
            setup::finish();
        }
    }

    unsafe impl NSObjectProtocol for Actions {}

    unsafe impl NSApplicationDelegate for Actions {
        /// Opening padcursor again while it runs (from Finder or Spotlight) shows the settings.
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_handle_reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
            settings::show(self.mtm());
            true
        }
    }
);

thread_local! {
    static ACTIONS: OnceCell<Retained<Actions>> = const { OnceCell::new() };
}

/// The shared action target. It lives for the whole process.
pub fn actions(mtm: MainThreadMarker) -> Retained<Actions> {
    ACTIONS.with(|cell| {
        cell.get_or_init(|| {
            let this = Actions::alloc(mtm).set_ivars(());
            // SAFETY: NSObject's init takes no arguments.
            unsafe { msg_send![super(this), init] }
        })
        .clone()
    })
}

/// A main menu is never visible (Stickmouse has no Dock icon), but text fields need its Edit items
/// for copy and paste shortcuts to work.
pub fn main_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let item = |title: &str, action: Sel, key: &str| {
        // SAFETY: standard responder-chain selectors, sent to the focused text field.
        unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(action),
                &NSString::from_str(key),
            )
        }
    };
    let app_menu = NSMenu::new(mtm);
    app_menu.addItem(&item("Close Window", sel!(performClose:), "w"));
    app_menu.addItem(&item("Quit Stickmouse", sel!(quit:), "q"));
    let edit_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Edit"));
    for (title, action, key) in [
        ("Cut", sel!(cut:), "x"),
        ("Copy", sel!(copy:), "c"),
        ("Paste", sel!(paste:), "v"),
        ("Select All", sel!(selectAll:), "a"),
        ("Undo", sel!(undo:), "z"),
    ] {
        edit_menu.addItem(&item(title, action, key));
    }
    let bar = NSMenu::new(mtm);
    for submenu in [app_menu, edit_menu] {
        let holder = NSMenuItem::new(mtm);
        holder.setSubmenu(Some(&submenu));
        bar.addItem(&holder);
    }
    bar
}

pub fn label(mtm: MainThreadMarker, text: &str) -> Retained<NSTextField> {
    NSTextField::labelWithString(&NSString::from_str(text), mtm)
}

pub fn bold(mtm: MainThreadMarker, text: &str, size: f64) -> Retained<NSTextField> {
    let field = label(mtm, text);
    field.setFont(Some(&NSFont::boldSystemFontOfSize(size)));
    field
}

/// Smaller grey text that wraps at `width`.
pub fn secondary(mtm: MainThreadMarker, text: &str, width: f64) -> Retained<NSTextField> {
    let field = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    field.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    field.setTextColor(Some(&NSColor::secondaryLabelColor()));
    field.setPreferredMaxLayoutWidth(width);
    fixed_width(&field, width);
    field
}

pub fn right_aligned(field: &NSTextField) {
    field.setAlignment(NSTextAlignment::Right);
}

pub fn button(mtm: MainThreadMarker, title: &str, action: Sel) -> Retained<NSButton> {
    let target = actions(mtm);
    // SAFETY: `action` is one of the selectors defined on Actions.
    unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(title), Some(&target), Some(action), mtm)
    }
}

pub fn checkbox(mtm: MainThreadMarker, title: &str, action: Sel) -> Retained<NSButton> {
    let target = actions(mtm);
    // SAFETY: as above.
    unsafe {
        NSButton::checkboxWithTitle_target_action(&NSString::from_str(title), Some(&target), Some(action), mtm)
    }
}

pub fn slider(mtm: MainThreadMarker, value: f64, min: f64, max: f64, action: Sel) -> Retained<NSSlider> {
    let target = actions(mtm);
    // SAFETY: as above.
    let slider = unsafe {
        NSSlider::sliderWithValue_minValue_maxValue_target_action(value, min, max, Some(&target), Some(action), mtm)
    };
    slider.setContinuous(true);
    slider
}

pub fn symbol(mtm: MainThreadMarker, name: &str, color: &NSColor, size: f64) -> Retained<NSImageView> {
    let view = NSImageView::new(mtm);
    set_symbol(&view, name, color);
    fixed_width(&view, size);
    fixed_height(&view, size);
    view
}

pub fn set_symbol(view: &NSImageView, name: &str, color: &NSColor) {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None);
    view.setImage(image.as_deref());
    view.setContentTintColor(Some(color));
}

pub fn app_icon(mtm: MainThreadMarker, size: f64) -> Retained<NSImageView> {
    let view = NSImageView::new(mtm);
    view.setImage(NSApplication::sharedApplication(mtm).applicationIconImage().as_deref());
    fixed_width(&view, size);
    fixed_height(&view, size);
    view
}

pub fn separator(mtm: MainThreadMarker, width: f64) -> Retained<NSBox> {
    let line = NSBox::new(mtm);
    line.setBoxType(NSBoxType::Separator);
    fixed_width(&line, width);
    line
}

pub fn vstack(mtm: MainThreadMarker, views: &[&NSView], spacing: f64) -> Retained<NSStackView> {
    stack(mtm, views, spacing, NSUserInterfaceLayoutOrientation::Vertical, NSLayoutAttribute::Leading)
}

pub fn centered_vstack(mtm: MainThreadMarker, views: &[&NSView], spacing: f64) -> Retained<NSStackView> {
    stack(mtm, views, spacing, NSUserInterfaceLayoutOrientation::Vertical, NSLayoutAttribute::CenterX)
}

pub fn hstack(mtm: MainThreadMarker, views: &[&NSView], spacing: f64) -> Retained<NSStackView> {
    stack(mtm, views, spacing, NSUserInterfaceLayoutOrientation::Horizontal, NSLayoutAttribute::CenterY)
}

fn stack(
    mtm: MainThreadMarker,
    views: &[&NSView],
    spacing: f64,
    orientation: NSUserInterfaceLayoutOrientation,
    alignment: NSLayoutAttribute,
) -> Retained<NSStackView> {
    let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(views), mtm);
    stack.setOrientation(orientation);
    stack.setAlignment(alignment);
    stack.setSpacing(spacing);
    stack
}

pub fn fixed_width(view: &NSView, width: f64) {
    view.widthAnchor().constraintEqualToConstant(width).setActive(true);
}

pub fn fixed_height(view: &NSView, height: f64) {
    view.heightAnchor().constraintEqualToConstant(height).setActive(true);
}

/// A titled, closable window sized to fit `content`.
pub fn window(mtm: MainThreadMarker, title: &str, content: &NSStackView) -> Retained<NSWindow> {
    content.setEdgeInsets(objc2_foundation::NSEdgeInsets { top: 20.0, left: 24.0, bottom: 20.0, right: 24.0 });
    // SAFETY: standard window creation on the main thread.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 300.0)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            objc2_app_kit::NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is kept by its module, so AppKit must not free it on close.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(title));
    window.setContentView(Some(content));
    window.setContentSize(content.fittingSize());
    window.center();
    window
}

/// Brings `window` to the front. padcursor has no Dock icon, so the app must activate first.
pub fn present(mtm: MainThreadMarker, window: &NSWindow) {
    NSApplication::sharedApplication(mtm).activate();
    window.makeKeyAndOrderFront(None);
    // macOS may refuse to activate an app that runs in the background, so force the window up.
    window.orderFrontRegardless();
}
