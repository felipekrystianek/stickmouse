//! The Customize Buttons window: what each stick, button, trigger and D-pad direction does.

use std::cell::RefCell;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSEvent, NSEventMask, NSEventModifierFlags, NSMenu, NSMenuItem, NSPopUpButton, NSTextField,
    NSView, NSWindow,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use super::{config_store, keys, ui};
use crate::bindings::{Binding, Command, Control, Key, NAMED_COMMANDS, Shortcut, StickRole};

const LABEL_WIDTH: f64 = 150.0;
const POPUP_WIDTH: f64 = 230.0;
const STICK_TAG: isize = 1000;
/// Single keys offered in every list, besides recording any key or shortcut.
const COMMON_KEYS: [Key; 9] =
    [Key::Esc, Key::Enter, Key::Space, Key::Tab, Key::Backspace, Key::Up, Key::Down, Key::Left, Key::Right];

#[derive(Clone)]
enum Choice {
    Command(Command),
    Record,
    Separator,
}

struct Popup {
    view: Retained<NSPopUpButton>,
    choices: Vec<Choice>,
}

struct Window {
    window: Retained<NSWindow>,
    /// Two per control, press then hold; the Xbox button has no hold list.
    popups: Vec<Option<Popup>>,
    sticks: [Retained<NSPopUpButton>; 2],
    status: Retained<NSTextField>,
    /// The list waiting for a key press, and the key monitor catching it.
    recording: Option<(Control, bool, Retained<AnyObject>)>,
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
}

pub fn show(mtm: MainThreadMarker) {
    WINDOW.with_borrow_mut(|slot| {
        let window = slot.get_or_insert_with(|| build(mtm));
        config_store::reload();
        refill(mtm, window);
        ui::present(mtm, &window.window);
    });
}

fn tag(control: Control, hold: bool) -> isize {
    (control.index() * 2 + usize::from(hold)) as isize
}

fn popup(mtm: MainThreadMarker, tag: isize, action: objc2::runtime::Sel) -> Retained<NSPopUpButton> {
    let view = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(POPUP_WIDTH, 26.0)),
        false,
    );
    view.setTag(tag);
    let target = ui::actions(mtm);
    // SAFETY: the selectors exist on ui::Actions, which lives for the whole process.
    unsafe {
        view.setTarget(Some(&target));
        view.setAction(Some(action));
    }
    ui::fixed_width(&view, POPUP_WIDTH);
    view
}

fn build(mtm: MainThreadMarker) -> Window {
    let mut rows: Vec<Retained<NSView>> = Vec::new();
    rows.push(
        ui::secondary(
            mtm,
            "Choose what each part of the controller does. A hold action happens instead when a button is held; buttons with one react when you let go, so they cannot drag.",
            LABEL_WIDTH + 2.0 * POPUP_WIDTH + 16.0,
        )
        .into_super()
        .into_super(),
    );

    let sticks = [STICK_TAG, STICK_TAG + 1].map(|tag| popup(mtm, tag, sel!(stickChanged:)));
    for (name, view) in ["Left stick", "Right stick"].iter().zip(&sticks) {
        let label = ui::label(mtm, name);
        ui::fixed_width(&label, LABEL_WIDTH);
        rows.push(ui::hstack(mtm, &[&label, view], 8.0).into_super());
    }
    rows.push(ui::separator(mtm, LABEL_WIDTH + 2.0 * POPUP_WIDTH + 16.0).into_super());

    let header = |text| {
        let label = ui::bold(mtm, text, 12.0);
        ui::fixed_width(&label, POPUP_WIDTH);
        label
    };
    let spacer = ui::label(mtm, "");
    ui::fixed_width(&spacer, LABEL_WIDTH);
    rows.push(ui::hstack(mtm, &[&spacer, &header("Press"), &header("Hold")], 8.0).into_super());

    let mut popups = Vec::new();
    for control in Control::ALL {
        let label = ui::label(mtm, control.label());
        ui::fixed_width(&label, LABEL_WIDTH);
        let press = popup(mtm, tag(control, false), sel!(bindingChanged:));
        let row = if control == Control::Xbox {
            let fixed = ui::secondary(mtm, "Turns Stickmouse on or off", POPUP_WIDTH);
            popups.push(Some(Popup { view: press.clone(), choices: Vec::new() }));
            popups.push(None);
            ui::hstack(mtm, &[&label, &press, &fixed], 8.0)
        } else {
            let hold = popup(mtm, tag(control, true), sel!(bindingChanged:));
            popups.push(Some(Popup { view: press.clone(), choices: Vec::new() }));
            popups.push(Some(Popup { view: hold.clone(), choices: Vec::new() }));
            ui::hstack(mtm, &[&label, &press, &hold], 8.0)
        };
        rows.push(row.into_super());
    }

    let status = ui::label(mtm, "");
    status.setHidden(true);
    rows.push(status.clone().into_super().into_super());
    rows.push(ui::button(mtm, "Restore Default Buttons", sel!(restoreButtons:)).into_super().into_super());

    let views: Vec<&NSView> = rows.iter().map(|view| &**view).collect();
    let content = ui::vstack(mtm, &views, 8.0);
    let window = ui::window(mtm, "Customize Buttons", &content);
    Window { window, popups, sticks, status, recording: None }
}

fn choices(current: &Command) -> Vec<Choice> {
    let mut list = Vec::new();
    for (name, _) in NAMED_COMMANDS {
        // macOS does not let other apps show its on-screen keyboard.
        if name == "on-screen-keyboard" {
            continue;
        }
        let command = Command::parse(name).unwrap();
        list.push(Choice::Command(command));
    }
    list.push(Choice::Separator);
    for key in COMMON_KEYS {
        list.push(Choice::Command(Command::Keys(Shortcut::key(key))));
    }
    if !list.iter().any(|choice| matches!(choice, Choice::Command(c) if c == current)) {
        list.push(Choice::Command(current.clone()));
    }
    list.push(Choice::Separator);
    list.push(Choice::Record);
    list
}

fn fill(mtm: MainThreadMarker, popup: &mut Popup, current: &Command) {
    popup.choices = choices(current);
    let menu = NSMenu::new(mtm);
    let mut selected = 0;
    for (index, choice) in popup.choices.iter().enumerate() {
        let item = match choice {
            Choice::Separator => NSMenuItem::separatorItem(mtm),
            Choice::Record => menu_item(mtm, "Other key or shortcut…"),
            Choice::Command(command) => {
                if command == current {
                    selected = index;
                }
                menu_item(mtm, &command.label())
            }
        };
        menu.addItem(&item);
    }
    popup.view.setMenu(Some(&menu));
    popup.view.selectItemAtIndex(selected as isize);
}

fn menu_item(mtm: MainThreadMarker, title: &str) -> Retained<NSMenuItem> {
    // SAFETY: an item without an action; the pop-up button reports the selection itself.
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            None,
            &NSString::new(),
        )
    }
}

fn refill(mtm: MainThreadMarker, window: &mut Window) {
    let config = config_store::get();
    for (view, role) in window.sticks.iter().zip([config.left_stick, config.right_stick]) {
        let menu = NSMenu::new(mtm);
        for option in StickRole::ALL {
            menu.addItem(&menu_item(mtm, option.label()));
        }
        view.setMenu(Some(&menu));
        let index = StickRole::ALL.iter().position(|r| *r == role).unwrap_or(0);
        view.selectItemAtIndex(index as isize);
    }
    for control in Control::ALL {
        let binding = config.buttons.get(control);
        for (hold, command) in [(false, &binding.press), (true, &binding.hold)] {
            if let Some(popup) = window.popups[tag(control, hold) as usize].as_mut() {
                fill(mtm, popup, command);
            }
        }
    }
}

pub fn stick_changed(view: &NSPopUpButton) {
    let role = StickRole::ALL.get(view.indexOfSelectedItem().max(0) as usize).copied();
    let Some(role) = role else {
        return;
    };
    let left = view.tag() == STICK_TAG;
    config_store::update(|config| {
        if left {
            config.left_stick = role;
        } else {
            config.right_stick = role;
        }
    });
}

pub fn binding_changed(mtm: MainThreadMarker, view: &NSPopUpButton) {
    let tag = view.tag() as usize;
    let (control, hold) = (Control::ALL[tag / 2], tag % 2 == 1);
    let choice = WINDOW.with_borrow(|slot| {
        let popup = slot.as_ref()?.popups.get(tag)?.as_ref()?;
        popup.choices.get(view.indexOfSelectedItem().max(0) as usize).cloned()
    });
    match choice {
        Some(Choice::Command(command)) => set_binding(mtm, control, hold, command),
        Some(Choice::Record) => start_recording(mtm, control, hold),
        _ => {}
    }
}

fn set_binding(mtm: MainThreadMarker, control: Control, hold: bool, command: Command) {
    config_store::update(|config| {
        let mut binding: Binding = config.buttons.get(control).clone();
        if hold {
            binding.hold = command;
        } else {
            binding.press = command;
        }
        config.buttons.set(control, binding);
    });
    WINDOW.with_borrow_mut(|slot| {
        if let Some(window) = slot.as_mut() {
            refill(mtm, window);
        }
    });
}

pub fn restore_defaults(mtm: MainThreadMarker) {
    config_store::update(|config| {
        let defaults = crate::config::Config::default();
        config.buttons = defaults.buttons;
        config.left_stick = defaults.left_stick;
        config.right_stick = defaults.right_stick;
    });
    WINDOW.with_borrow_mut(|slot| {
        if let Some(window) = slot.as_mut() {
            refill(mtm, window);
        }
    });
}

/// Waits for the next key press in this window and uses it, with its modifiers, as a shortcut.
fn start_recording(mtm: MainThreadMarker, control: Control, hold: bool) {
    stop_recording();
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit passes a valid event for the duration of the call.
        let event = unsafe { event.as_ref() };
        let mtm = MainThreadMarker::new().expect("local monitors run on the main thread");
        recorded(mtm, event.keyCode(), event.modifierFlags());
        std::ptr::null_mut()
    });
    // SAFETY: the block only runs on the main thread while the monitor is installed.
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block) };
    WINDOW.with_borrow_mut(|slot| {
        if let (Some(window), Some(monitor)) = (slot.as_mut(), monitor) {
            window.status.setStringValue(&NSString::from_str(&format!(
                "Press the key or shortcut for {} ({}). Esc cancels.",
                control.label(),
                if hold { "hold" } else { "press" }
            )));
            window.status.setHidden(false);
            window.recording = Some((control, hold, monitor));
        }
    });
    let _ = mtm;
}

fn recorded(mtm: MainThreadMarker, code: u16, flags: NSEventModifierFlags) {
    let Some((control, hold)) = WINDOW.with_borrow(|slot| {
        slot.as_ref()?.recording.as_ref().map(|(control, hold, _)| (*control, *hold))
    }) else {
        return;
    };
    let Some(key) = keys::key(code) else {
        WINDOW.with_borrow(|slot| {
            if let Some(window) = slot {
                window.status.setStringValue(&NSString::from_str(
                    "That key cannot be used yet. Try another one, or press Esc to cancel.",
                ));
            }
        });
        return;
    };
    let mut modifiers = Vec::new();
    for (flag, modifier) in [
        (NSEventModifierFlags::Control, Key::Ctrl),
        (NSEventModifierFlags::Option, Key::Alt),
        (NSEventModifierFlags::Shift, Key::Shift),
        (NSEventModifierFlags::Command, Key::Meta),
    ] {
        if flags.contains(flag) {
            modifiers.push(modifier);
        }
    }
    stop_recording();
    if key == Key::Esc && modifiers.is_empty() {
        // Cancelled: show the old choice again.
        WINDOW.with_borrow_mut(|slot| {
            if let Some(window) = slot.as_mut() {
                refill(mtm, window);
            }
        });
        return;
    }
    set_binding(mtm, control, hold, Command::Keys(Shortcut::new(modifiers, key)));
}

fn stop_recording() {
    WINDOW.with_borrow_mut(|slot| {
        if let Some(window) = slot.as_mut() {
            if let Some((_, _, monitor)) = window.recording.take() {
                // SAFETY: the monitor came from addLocalMonitorForEventsMatchingMask.
                unsafe { NSEvent::removeMonitor(&monitor) };
            }
            window.status.setHidden(true);
        }
    });
}
