//! Reads the controller through Apple's GameController framework. Polled from the main thread,
//! which is where GameController keeps its list of controllers up to date.

use std::cell::RefCell;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};

use objc2::{AllocAnyThread, msg_send};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_haptics::{
    CHHapticEngine, CHHapticEvent, CHHapticEventParameter, CHHapticEventParameterIDHapticIntensity,
    CHHapticEventTypeHapticContinuous, CHHapticPattern, CHHapticPatternPlayer, CHHapticTimeImmediate,
};
use objc2_foundation::{NSArray, NSError};
use objc2_game_controller::{
    GCController, GCControllerButtonInput, GCDevice, GCExtendedGamepad, GCHapticsLocalityDefault,
    GCSystemGestureState,
};

use crate::Message;
use crate::engine::{Axis, Button, Input};

/// Polls between controller rescans: 125 × 4 ms = 0.5 s.
const RESCAN_EVERY: u32 = 125;
const RUMBLE_SECONDS: f64 = 0.15;
const BUTTONS: [Button; 11] = [
    Button::A,
    Button::B,
    Button::X,
    Button::Y,
    Button::LeftBumper,
    Button::RightBumper,
    Button::View,
    Button::Menu,
    Button::Xbox,
    Button::LeftStickClick,
    Button::RightStickClick,
];
const AXES: [Axis; 8] = [
    Axis::LeftX,
    Axis::LeftY,
    Axis::RightX,
    Axis::RightY,
    Axis::LeftTrigger,
    Axis::RightTrigger,
    Axis::DpadX,
    Axis::DpadY,
];

pub enum Command {
    /// Linux grabs the controller so other apps do not see it. macOS has no equivalent.
    Grab(#[allow(dead_code)] bool),
    Rumble,
}

struct Link {
    messages: Sender<Message>,
    commands: Receiver<Command>,
}

static LINK: OnceLock<Mutex<Link>> = OnceLock::new();
static BATTERY: Mutex<Option<f32>> = Mutex::new(None);

#[derive(Clone, Copy, Default, PartialEq)]
struct State {
    buttons: [bool; BUTTONS.len()],
    axes: [f32; AXES.len()],
}

struct Active {
    controller: Retained<GCController>,
    state: State,
    haptics: Option<Retained<CHHapticEngine>>,
    player: Option<Retained<ProtocolObject<dyn CHHapticPatternPlayer>>>,
}

#[derive(Default)]
struct Poller {
    active: Option<Active>,
    polls: u32,
}

thread_local! {
    static POLLER: RefCell<Poller> = RefCell::default();
}

pub fn spawn(messages: Sender<Message>, _grab: bool) -> Sender<Command> {
    let (commands_tx, commands) = channel();
    let _ = LINK.set(Mutex::new(Link { messages, commands }));
    commands_tx
}

/// The battery level from 0.0 to 1.0, if the controller reports one.
pub fn battery() -> Option<f32> {
    *BATTERY.lock().unwrap()
}

/// Called on the main thread every few milliseconds.
pub fn poll() {
    let Some(link) = LINK.get() else {
        return;
    };
    let link = link.lock().unwrap();
    POLLER.with_borrow_mut(|poller| {
        if poller.polls % RESCAN_EVERY == 0 {
            rescan(poller, &link.messages);
        }
        poller.polls = poller.polls.wrapping_add(1);

        for command in link.commands.try_iter() {
            if let (Command::Rumble, Some(active)) = (command, poller.active.as_mut())
                && let Err(error) = rumble(active)
            {
                eprintln!("padcursor: rumble failed: {error}");
            }
        }

        let Some(active) = poller.active.as_mut() else {
            return;
        };
        // SAFETY: GameController objects are used on the main thread only.
        let Some(pad) = (unsafe { active.controller.extendedGamepad() }) else {
            return;
        };
        let state = read(&pad);
        for (index, button) in BUTTONS.into_iter().enumerate() {
            if state.buttons[index] != active.state.buttons[index] {
                let _ = link.messages.send(Message::Input(Input::Button(button, state.buttons[index])));
            }
        }
        for (index, axis) in AXES.into_iter().enumerate() {
            if state.axes[index] != active.state.axes[index] {
                let _ = link.messages.send(Message::Input(Input::Axis(axis, state.axes[index])));
            }
        }
        active.state = state;
    });
}

fn rescan(poller: &mut Poller, messages: &Sender<Message>) {
    // SAFETY: GameController objects are used on the main thread only.
    let controllers = unsafe { GCController::controllers() };
    let still_there = poller
        .active
        .as_ref()
        .is_some_and(|active| controllers.iter().any(|c| c == active.controller));

    if poller.active.is_some() && !still_there {
        poller.active = None;
        *BATTERY.lock().unwrap() = None;
        eprintln!("padcursor: controller disconnected");
        let _ = messages.send(Message::Connected(false));
        let _ = messages.send(Message::Input(Input::Disconnected));
    }

    if poller.active.is_none()
        && let Some(controller) =
            controllers.iter().find(|c| unsafe { c.extendedGamepad() }.is_some())
    {
        // SAFETY: as above.
        unsafe {
            if let Some(home) = controller.extendedGamepad().and_then(|pad| pad.buttonHome()) {
                // The Xbox button would otherwise open the system game overlay.
                home.setPreferredSystemGestureState(GCSystemGestureState::Disabled);
            }
            let name = controller.vendorName().map_or("unknown".to_string(), |n| n.to_string());
            eprintln!("padcursor: using controller {name:?}");
        }
        poller.active = Some(Active { controller, state: State::default(), haptics: None, player: None });
        let _ = messages.send(Message::Connected(true));
    }

    if let Some(active) = &poller.active {
        // SAFETY: as above.
        let level = unsafe { active.controller.battery() }.map(|b| unsafe { b.batteryLevel() });
        *BATTERY.lock().unwrap() = level;
    }
}

fn read(pad: &GCExtendedGamepad) -> State {
    let pressed = |button: Option<Retained<GCControllerButtonInput>>| {
        button.is_some_and(|button| unsafe { button.isPressed() })
    };
    // SAFETY: plain property reads on the main thread.
    unsafe {
        let (left, right, dpad) = (pad.leftThumbstick(), pad.rightThumbstick(), pad.dpad());
        State {
            buttons: [
                pad.buttonA().isPressed(),
                pad.buttonB().isPressed(),
                pad.buttonX().isPressed(),
                pad.buttonY().isPressed(),
                pad.leftShoulder().isPressed(),
                pad.rightShoulder().isPressed(),
                pressed(pad.buttonOptions()),
                pad.buttonMenu().isPressed(),
                pressed(pad.buttonHome()),
                pressed(pad.leftThumbstickButton()),
                pressed(pad.rightThumbstickButton()),
            ],
            // GameController reports up as positive; the engine expects Linux's down-is-positive.
            axes: [
                left.xAxis().value(),
                -left.yAxis().value(),
                right.xAxis().value(),
                -right.yAxis().value(),
                pad.leftTrigger().value(),
                pad.rightTrigger().value(),
                dpad.xAxis().value(),
                -dpad.yAxis().value(),
            ],
        }
    }
}

fn rumble(active: &mut Active) -> Result<(), String> {
    // SAFETY: CoreHaptics objects are created and used on the main thread only.
    unsafe {
        if active.haptics.is_none() {
            let haptics = active.controller.haptics().ok_or("controller has no haptics")?;
            // The Rust binding only exposes this on iOS, but GameController has it on macOS 11+.
            let engine: Option<Retained<CHHapticEngine>> =
                msg_send![&*haptics, createEngineWithLocality: GCHapticsLocalityDefault];
            let engine = engine.ok_or("could not create a haptic engine")?;
            active.haptics = Some(engine);
        }
        let engine = active.haptics.as_ref().unwrap();
        engine.startAndReturnError().map_err(describe)?;

        let intensity = CHHapticEventParameter::initWithParameterID_value(
            CHHapticEventParameter::alloc(),
            CHHapticEventParameterIDHapticIntensity,
            1.0,
        );
        let event = CHHapticEvent::initWithEventType_parameters_relativeTime_duration(
            CHHapticEvent::alloc(),
            CHHapticEventTypeHapticContinuous,
            &NSArray::from_retained_slice(&[intensity]),
            0.0,
            RUMBLE_SECONDS,
        );
        let pattern = CHHapticPattern::initWithEvents_parameters_error(
            CHHapticPattern::alloc(),
            &NSArray::from_retained_slice(&[event]),
            &NSArray::new(),
        )
        .map_err(describe)?;
        let player = engine.createPlayerWithPattern_error(&pattern).map_err(describe)?;
        player.startAtTime_error(CHHapticTimeImmediate).map_err(describe)?;
        active.player = Some(player);
    }
    Ok(())
}

fn describe(error: Retained<NSError>) -> String {
    error.localizedDescription().to_string()
}
