//! Turns controller input into desktop actions. Has no platform code and takes time as an
//! argument, so the whole button layout can be tested without hardware. What each button does
//! comes from the settings (see bindings.rs).

use std::time::Duration;

pub use crate::bindings::Key;
use crate::bindings::{Command, Control, Shortcut, StickRole};
use crate::config::Config;

const MAX_TICK: Duration = Duration::from_millis(50);
const TRIGGER_PRESS: f32 = 0.5;
const TRIGGER_RELEASE: f32 = 0.3;
const REAL_MOUSE_PRIORITY: Duration = Duration::from_millis(500);
const CONTROLS: usize = Control::ALL.len();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    A,
    B,
    X,
    Y,
    LeftBumper,
    RightBumper,
    View,
    Menu,
    Xbox,
    LeftStickClick,
    RightStickClick,
}

impl Button {
    fn control(self) -> Control {
        match self {
            Button::A => Control::A,
            Button::B => Control::B,
            Button::X => Control::X,
            Button::Y => Control::Y,
            Button::LeftBumper => Control::LeftBumper,
            Button::RightBumper => Control::RightBumper,
            Button::View => Control::View,
            Button::Menu => Control::Menu,
            Button::Xbox => Control::Xbox,
            Button::LeftStickClick => Control::LeftStickClick,
            Button::RightStickClick => Control::RightStickClick,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    LeftX,
    LeftY,
    RightX,
    RightY,
    LeftTrigger,
    RightTrigger,
    DpadX,
    DpadY,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    Button(Button, bool),
    Axis(Axis, f32),
    RealMouseMoved,
    Disconnected,
    ToggleEnabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Move(i32, i32),
    /// Vertical and horizontal scroll in hi-res wheel units (120 = one notch). Positive is up/right.
    Scroll(i32, i32),
    MouseDown(MouseButton),
    MouseUp(MouseButton),
    KeyDown(Key),
    KeyUp(Key),
    Tap(Key),
    Click(MouseButton),
    /// Holds or lets go of the app switcher's modifier: Alt on Linux, Command on a Mac.
    SwitcherHold(bool),
    /// Moves the app switcher one app further (Tab while the modifier is held).
    SwitcherNext,
    /// Opens the application launcher: Meta on Linux, Spotlight on a Mac.
    Launcher,
    ToggleKeyboard,
    MissionControl,
    Steering(bool),
    SetEnabled(bool),
    Rumble,
}

pub struct Engine {
    config: Config,
    enabled: bool,
    left: (f32, f32),
    right: (f32, f32),
    move_remainder: (f32, f32),
    scroll_remainder: (f32, f32),
    precise: bool,
    steering: bool,
    last_steer: Duration,
    real_mouse_at: Option<Duration>,
    /// Which controls are down, as far as padcursor acted on them.
    pressed: [bool; CONTROLS],
    /// When a control with a hold action went down, until it is released or the hold happens.
    pending: [Option<Duration>; CONTROLS],
    /// Commands started by a press that must be finished on release, like a held mouse button.
    active: [Option<Command>; CONTROLS],
    /// When a held volume command repeats next.
    next_repeat: [Option<Duration>; CONTROLS],
    xbox_down_at: Option<Duration>,
    xbox_toggled: bool,
    dpad: (i32, i32),
    switcher_release_at: Option<Duration>,
    last_tick: Option<Duration>,
    actions: Vec<Action>,
}

impl Engine {
    pub fn new(config: Config, enabled: bool) -> Self {
        Self {
            config,
            enabled,
            left: (0.0, 0.0),
            right: (0.0, 0.0),
            move_remainder: (0.0, 0.0),
            scroll_remainder: (0.0, 0.0),
            precise: false,
            steering: false,
            last_steer: Duration::ZERO,
            real_mouse_at: None,
            pressed: [false; CONTROLS],
            pending: [None; CONTROLS],
            active: std::array::from_fn(|_| None),
            next_repeat: [None; CONTROLS],
            xbox_down_at: None,
            xbox_toggled: false,
            dpad: (0, 0),
            switcher_release_at: None,
            last_tick: None,
            actions: Vec::new(),
        }
    }

    /// Applies new settings while running. Buttons held now finish with their old command.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn set_config(&mut self, config: Config) {
        self.config = config;
    }

    pub fn handle(&mut self, input: Input, now: Duration) -> Vec<Action> {
        match input {
            Input::Button(Button::Xbox, pressed) => self.xbox(pressed, now),
            Input::Button(button, pressed) if self.enabled => self.control(button.control(), pressed, now),
            Input::Button(..) => {}
            Input::Axis(axis, value) => self.axis(axis, value, now),
            Input::RealMouseMoved => {
                self.real_mouse_at = Some(now);
                self.set_steering(false);
            }
            Input::Disconnected => {
                self.release_all();
                self.left = (0.0, 0.0);
                self.right = (0.0, 0.0);
                self.xbox_down_at = None;
                self.xbox_toggled = false;
            }
            Input::ToggleEnabled => self.toggle(),
        }
        std::mem::take(&mut self.actions)
    }

    /// Advances everything time-based: stick motion, button holds and repeats.
    pub fn tick(&mut self, now: Duration) -> Vec<Action> {
        let dt = self
            .last_tick
            .map_or(Duration::ZERO, |last| now.saturating_sub(last))
            .min(MAX_TICK)
            .as_secs_f32();
        self.last_tick = Some(now);

        if let Some(down_at) = self.xbox_down_at
            && !self.xbox_toggled
            && now.saturating_sub(down_at) >= self.config.xbox_hold()
        {
            self.xbox_toggled = true;
            self.toggle();
        }

        if self.enabled {
            self.tick_holds(now);
            self.tick_repeats(now);
            self.tick_switcher(now);
            self.tick_sticks(dt, now);
        }
        std::mem::take(&mut self.actions)
    }

    /// Lets go of everything held down. Call before exiting.
    pub fn shutdown(&mut self) -> Vec<Action> {
        self.release_all();
        std::mem::take(&mut self.actions)
    }

    /// Holding the Xbox button always toggles padcursor; a short press runs its command.
    fn xbox(&mut self, pressed: bool, now: Duration) {
        if pressed {
            self.xbox_down_at = Some(now);
            self.xbox_toggled = false;
        } else {
            if self.xbox_down_at.take().is_some() && !self.xbox_toggled && self.enabled {
                let command = self.config.buttons.get(Control::Xbox).press.clone();
                self.tap(&command, now);
            }
            self.xbox_toggled = false;
        }
    }

    fn control(&mut self, control: Control, pressed: bool, now: Duration) {
        let index = control.index();
        if self.pressed[index] == pressed {
            return;
        }
        self.pressed[index] = pressed;
        let binding = self.config.buttons.get(control).clone();
        if pressed {
            if binding.hold == Command::None {
                self.begin(control, binding.press, now);
            } else {
                // Wait to see whether this is a press or a hold.
                self.pending[index] = Some(now);
            }
        } else if self.pending[index].take().is_some() {
            self.tap(&binding.press, now);
        } else {
            self.end(control);
        }
    }

    fn axis(&mut self, axis: Axis, value: f32, now: Duration) {
        match axis {
            Axis::LeftX => self.left.0 = value,
            Axis::LeftY => self.left.1 = value,
            Axis::RightX => self.right.0 = value,
            Axis::RightY => self.right.1 = value,
            Axis::LeftTrigger | Axis::RightTrigger => {
                let control =
                    if axis == Axis::LeftTrigger { Control::LeftTrigger } else { Control::RightTrigger };
                if !self.enabled {
                    return;
                }
                let pressed = self.pressed[control.index()];
                if !pressed && value >= TRIGGER_PRESS {
                    self.control(control, true, now);
                } else if pressed && value < TRIGGER_RELEASE {
                    self.control(control, false, now);
                }
            }
            Axis::DpadX => self.dpad_axis(value, true, now),
            Axis::DpadY => self.dpad_axis(value, false, now),
        }
    }

    fn dpad_axis(&mut self, value: f32, horizontal: bool, now: Duration) {
        let direction = value.round() as i32;
        let (previous, negative, positive) = if horizontal {
            (self.dpad.0, Control::DpadLeft, Control::DpadRight)
        } else {
            (self.dpad.1, Control::DpadUp, Control::DpadDown)
        };
        if !self.enabled || previous == direction {
            return;
        }
        if horizontal {
            self.dpad.0 = direction;
        } else {
            self.dpad.1 = direction;
        }
        let control_for = |d: i32| if d < 0 { negative } else { positive };
        if previous != 0 {
            self.control(control_for(previous), false, now);
        }
        if direction != 0 {
            self.control(control_for(direction), true, now);
        }
    }

    /// Starts a command when its control goes down. Commands that last are ended by `end`.
    fn begin(&mut self, control: Control, command: Command, now: Duration) {
        let index = control.index();
        match &command {
            Command::None => return,
            Command::LeftClick | Command::RightClick | Command::MiddleClick => {
                self.actions.push(Action::MouseDown(mouse_button(&command)));
            }
            Command::Keys(shortcut) => {
                for modifier in &shortcut.modifiers {
                    self.actions.push(Action::KeyDown(*modifier));
                }
                self.actions.push(Action::KeyDown(shortcut.key));
            }
            Command::VolumeUp | Command::VolumeDown => {
                self.actions.push(Action::Tap(media_key(&command)));
                self.next_repeat[index] = Some(now + self.config.volume_delay());
            }
            _ => {
                self.fire(&command, now);
                return;
            }
        }
        self.active[index] = Some(command);
    }

    fn end(&mut self, control: Control) {
        let index = control.index();
        self.next_repeat[index] = None;
        match self.active[index].take() {
            Some(command @ (Command::LeftClick | Command::RightClick | Command::MiddleClick)) => {
                self.actions.push(Action::MouseUp(mouse_button(&command)));
            }
            Some(Command::Keys(shortcut)) => self.release_shortcut(&shortcut),
            _ => {}
        }
    }

    /// Runs a command at once, as a whole press and release.
    fn tap(&mut self, command: &Command, now: Duration) {
        match command {
            Command::None => {}
            Command::LeftClick | Command::RightClick | Command::MiddleClick => {
                self.actions.push(Action::Click(mouse_button(command)));
            }
            Command::Keys(shortcut) => {
                for modifier in &shortcut.modifiers {
                    self.actions.push(Action::KeyDown(*modifier));
                }
                self.actions.push(Action::Tap(shortcut.key));
                for modifier in shortcut.modifiers.iter().rev() {
                    self.actions.push(Action::KeyUp(*modifier));
                }
            }
            Command::VolumeUp | Command::VolumeDown => self.actions.push(Action::Tap(media_key(command))),
            other => self.fire(other, now),
        }
    }

    /// Commands that happen once, whether the control is pressed or held.
    fn fire(&mut self, command: &Command, now: Duration) {
        match command {
            Command::PreciseMode => self.precise = !self.precise,
            Command::AppSwitcher => {
                if self.switcher_release_at.is_none() {
                    self.actions.push(Action::SwitcherHold(true));
                }
                self.actions.push(Action::SwitcherNext);
                self.switcher_release_at = Some(now + self.config.alt_tab_commit());
            }
            Command::Launcher => self.actions.push(Action::Launcher),
            Command::OnScreenKeyboard => self.actions.push(Action::ToggleKeyboard),
            Command::MissionControl => self.actions.push(Action::MissionControl),
            Command::PlayPause | Command::NextTrack | Command::PreviousTrack | Command::Mute => {
                self.actions.push(Action::Tap(media_key(command)));
            }
            _ => self.tap(command, now),
        }
    }

    fn release_shortcut(&mut self, shortcut: &Shortcut) {
        self.actions.push(Action::KeyUp(shortcut.key));
        for modifier in shortcut.modifiers.iter().rev() {
            self.actions.push(Action::KeyUp(*modifier));
        }
    }

    fn tick_holds(&mut self, now: Duration) {
        for control in Control::ALL {
            let index = control.index();
            if let Some(down_at) = self.pending[index]
                && now.saturating_sub(down_at) >= self.config.hold()
            {
                self.pending[index] = None;
                let hold = self.config.buttons.get(control).hold.clone();
                self.tap(&hold, now);
            }
        }
    }

    fn tick_repeats(&mut self, now: Duration) {
        for index in 0..CONTROLS {
            if let Some(at) = self.next_repeat[index]
                && now >= at
                && let Some(command) = &self.active[index]
            {
                let key = media_key(command);
                self.actions.push(Action::Tap(key));
                self.next_repeat[index] = Some(now + self.config.volume_repeat());
            }
        }
    }

    fn tick_switcher(&mut self, now: Duration) {
        if self.switcher_release_at.is_some_and(|at| now >= at) {
            self.switcher_release_at = None;
            self.actions.push(Action::SwitcherHold(false));
        }
    }

    fn stick_for(&self, role: StickRole) -> (f32, f32) {
        let mut sum = (0.0, 0.0);
        for (stick, stick_role) in [(self.left, self.config.left_stick), (self.right, self.config.right_stick)] {
            if stick_role == role {
                let shaped = shape_stick(stick, self.config.stick_deadzone, self.config.stick_curve);
                sum = (sum.0 + shaped.0, sum.1 + shaped.1);
            }
        }
        sum
    }

    fn tick_sticks(&mut self, dt: f32, now: Duration) {
        let pointer = self.stick_for(StickRole::Pointer);
        if pointer != (0.0, 0.0) {
            let factor = if self.precise { self.config.precise_factor } else { 1.0 };
            let speed = self.config.pointer_speed * factor * dt;
            let x = pointer.0 * speed + self.move_remainder.0;
            let y = pointer.1 * speed + self.move_remainder.1;
            self.move_remainder = (x.fract(), y.fract());
            if x.trunc() != 0.0 || y.trunc() != 0.0 {
                self.actions.push(Action::Move(x.trunc() as i32, y.trunc() as i32));
            }
            self.last_steer = now;
            let real_mouse_has_priority = self
                .real_mouse_at
                .is_some_and(|at| now.saturating_sub(at) < REAL_MOUSE_PRIORITY);
            if !real_mouse_has_priority {
                self.set_steering(true);
            }
        } else if self.steering && now.saturating_sub(self.last_steer) >= self.config.steering_idle() {
            self.set_steering(false);
        }

        let scroll = self.stick_for(StickRole::Scroll);
        if scroll != (0.0, 0.0) {
            let speed = self.config.scroll_speed * dt;
            let horizontal = scroll.0 * speed + self.scroll_remainder.0;
            let vertical = -scroll.1 * speed + self.scroll_remainder.1;
            self.scroll_remainder = (horizontal.fract(), vertical.fract());
            if horizontal.trunc() != 0.0 || vertical.trunc() != 0.0 {
                self.actions.push(Action::Scroll(vertical.trunc() as i32, horizontal.trunc() as i32));
            }
        }
    }

    fn set_steering(&mut self, steering: bool) {
        if self.steering != steering {
            self.steering = steering;
            self.actions.push(Action::Steering(steering));
        }
    }

    fn toggle(&mut self) {
        self.enabled = !self.enabled;
        if !self.enabled {
            self.release_all();
        }
        self.actions.push(Action::SetEnabled(self.enabled));
        self.actions.push(Action::Rumble);
    }

    fn release_all(&mut self) {
        for control in Control::ALL {
            self.end(control);
        }
        self.pressed = [false; CONTROLS];
        self.pending = [None; CONTROLS];
        self.dpad = (0, 0);
        if self.switcher_release_at.take().is_some() {
            self.actions.push(Action::SwitcherHold(false));
        }
        self.set_steering(false);
    }
}

fn mouse_button(command: &Command) -> MouseButton {
    match command {
        Command::RightClick => MouseButton::Right,
        Command::MiddleClick => MouseButton::Middle,
        _ => MouseButton::Left,
    }
}

fn media_key(command: &Command) -> Key {
    match command {
        Command::VolumeUp => Key::VolumeUp,
        Command::VolumeDown => Key::VolumeDown,
        Command::NextTrack => Key::NextTrack,
        Command::PreviousTrack => Key::PreviousTrack,
        Command::Mute => Key::Mute,
        _ => Key::PlayPause,
    }
}

/// Applies a radial deadzone and a response curve, so small deflections give fine control.
fn shape_stick((x, y): (f32, f32), deadzone: f32, curve: f32) -> (f32, f32) {
    let magnitude = x.hypot(y);
    if magnitude <= deadzone {
        return (0.0, 0.0);
    }
    let strength = ((magnitude.min(1.0) - deadzone) / (1.0 - deadzone)).powf(curve);
    (x / magnitude * strength, y / magnitude * strength)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::Binding;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn engine() -> Engine {
        Engine::new(Config::default(), true)
    }

    fn engine_with(control: Control, press: &str, hold: &str) -> Engine {
        let mut config = Config::default();
        config.buttons.set(
            control,
            Binding { press: Command::parse(press).unwrap(), hold: Command::parse(hold).unwrap() },
        );
        Engine::new(config, true)
    }

    fn press(button: Button) -> Input {
        Input::Button(button, true)
    }

    fn release(button: Button) -> Input {
        Input::Button(button, false)
    }

    #[test]
    fn a_holds_left_mouse_for_dragging() {
        let mut e = engine();
        assert_eq!(e.handle(press(Button::A), ms(0)), [Action::MouseDown(MouseButton::Left)]);
        assert_eq!(e.tick(ms(500)), []);
        assert_eq!(e.handle(release(Button::A), ms(600)), [Action::MouseUp(MouseButton::Left)]);
    }

    #[test]
    fn b_tap_right_clicks_on_release() {
        let mut e = engine();
        assert_eq!(e.handle(press(Button::B), ms(0)), []);
        assert_eq!(e.tick(ms(100)), []);
        assert_eq!(e.handle(release(Button::B), ms(150)), [Action::Click(MouseButton::Right)]);
    }

    #[test]
    fn b_hold_sends_esc_and_no_right_click() {
        let mut e = engine();
        e.handle(press(Button::B), ms(0));
        assert_eq!(e.tick(ms(400)), [Action::Tap(Key::Esc)]);
        assert_eq!(e.tick(ms(500)), []);
        assert_eq!(e.handle(release(Button::B), ms(600)), []);
    }

    #[test]
    fn simple_buttons_match_the_layout() {
        let y = if cfg!(target_os = "macos") { Action::MissionControl } else { Action::ToggleKeyboard };
        let cases = [
            (Button::Y, y),
            (Button::LeftBumper, Action::Tap(Key::PreviousTrack)),
            (Button::RightBumper, Action::Tap(Key::NextTrack)),
            (Button::Menu, Action::Tap(Key::PlayPause)),
            (Button::RightStickClick, Action::Tap(Key::Mute)),
            (Button::X, Action::MouseDown(MouseButton::Middle)),
        ];
        for (button, action) in cases {
            assert_eq!(engine().handle(press(button), ms(0)), [action], "{button:?}");
        }
    }

    #[test]
    fn view_cycles_apps_then_commits() {
        let mut e = engine();
        assert_eq!(
            e.handle(press(Button::View), ms(0)),
            [Action::SwitcherHold(true), Action::SwitcherNext]
        );
        assert_eq!(e.tick(ms(500)), []);
        e.handle(release(Button::View), ms(510));
        assert_eq!(e.handle(press(Button::View), ms(520)), [Action::SwitcherNext]);
        assert_eq!(e.tick(ms(1300)), []);
        assert_eq!(e.tick(ms(1320)), [Action::SwitcherHold(false)]);
    }

    #[test]
    fn dpad_holds_arrows_and_switches_cleanly() {
        let mut e = engine();
        assert_eq!(e.handle(Input::Axis(Axis::DpadY, -1.0), ms(0)), [Action::KeyDown(Key::Up)]);
        assert_eq!(
            e.handle(Input::Axis(Axis::DpadY, 1.0), ms(10)),
            [Action::KeyUp(Key::Up), Action::KeyDown(Key::Down)]
        );
        assert_eq!(e.handle(Input::Axis(Axis::DpadY, 0.0), ms(20)), [Action::KeyUp(Key::Down)]);
        assert_eq!(e.handle(Input::Axis(Axis::DpadX, 1.0), ms(30)), [Action::KeyDown(Key::Right)]);
    }

    #[test]
    fn triggers_change_volume_and_repeat_while_held() {
        let mut e = engine();
        e.tick(ms(0));
        assert_eq!(e.handle(Input::Axis(Axis::RightTrigger, 0.9), ms(0)), [Action::Tap(Key::VolumeUp)]);
        assert_eq!(e.tick(ms(100)), []);
        assert_eq!(e.tick(ms(400)), [Action::Tap(Key::VolumeUp)]);
        assert_eq!(e.tick(ms(500)), []);
        assert_eq!(e.tick(ms(550)), [Action::Tap(Key::VolumeUp)]);
        assert_eq!(e.handle(Input::Axis(Axis::RightTrigger, 0.4), ms(555)), [], "still above release");
        e.handle(Input::Axis(Axis::RightTrigger, 0.0), ms(560));
        assert_eq!(e.tick(ms(570)), []);
        assert_eq!(e.tick(ms(2000)), []);
        assert_eq!(e.handle(Input::Axis(Axis::LeftTrigger, 1.0), ms(2000)), [Action::Tap(Key::VolumeDown)]);
    }

    #[test]
    fn a_button_can_send_a_held_shortcut() {
        let mut e = engine_with(Control::A, "cmd+shift+t", "none");
        assert_eq!(
            e.handle(press(Button::A), ms(0)),
            [Action::KeyDown(Key::Shift), Action::KeyDown(Key::Meta), Action::KeyDown(Key::Letter(b't'))]
        );
        assert_eq!(
            e.handle(release(Button::A), ms(50)),
            [Action::KeyUp(Key::Letter(b't')), Action::KeyUp(Key::Meta), Action::KeyUp(Key::Shift)]
        );
    }

    #[test]
    fn any_button_can_have_a_hold_action() {
        let mut e = engine_with(Control::Menu, "play-pause", "cmd+q");
        assert_eq!(e.handle(press(Button::Menu), ms(0)), []);
        assert_eq!(
            e.tick(ms(400)),
            [Action::KeyDown(Key::Meta), Action::Tap(Key::Letter(b'q')), Action::KeyUp(Key::Meta)]
        );
        assert_eq!(e.handle(release(Button::Menu), ms(450)), []);
        e.handle(press(Button::Menu), ms(1000));
        assert_eq!(e.handle(release(Button::Menu), ms(1100)), [Action::Tap(Key::PlayPause)]);
    }

    #[test]
    fn triggers_and_dpad_take_any_command() {
        let mut e = engine_with(Control::LeftTrigger, "left-click", "none");
        assert_eq!(
            e.handle(Input::Axis(Axis::LeftTrigger, 1.0), ms(0)),
            [Action::MouseDown(MouseButton::Left)]
        );
        assert_eq!(
            e.handle(Input::Axis(Axis::LeftTrigger, 0.0), ms(10)),
            [Action::MouseUp(MouseButton::Left)]
        );
        let mut e = engine_with(Control::DpadUp, "mission-control", "none");
        assert_eq!(e.handle(Input::Axis(Axis::DpadY, -1.0), ms(0)), [Action::MissionControl]);
    }

    #[test]
    fn none_does_nothing() {
        let mut e = engine_with(Control::A, "none", "none");
        assert_eq!(e.handle(press(Button::A), ms(0)), []);
        assert_eq!(e.handle(release(Button::A), ms(10)), []);
    }

    fn total_motion(actions: &[Action]) -> (i32, i32) {
        actions.iter().fold((0, 0), |sum, action| match action {
            Action::Move(x, y) => (sum.0 + x, sum.1 + y),
            _ => sum,
        })
    }

    fn steer_right_for(e: &mut Engine, ticks: u64) -> Vec<Action> {
        e.tick(ms(0));
        e.handle(Input::Axis(Axis::LeftX, 1.0), ms(0));
        (1..=ticks).flat_map(|i| e.tick(ms(i * 8))).collect()
    }

    #[test]
    fn left_stick_moves_at_configured_speed_and_shows_circle() {
        let mut e = engine();
        let actions = steer_right_for(&mut e, 125);
        assert!(actions.contains(&Action::Steering(true)));
        let (x, y) = total_motion(&actions);
        assert!((1395..=1400).contains(&x), "moved {x}");
        assert_eq!(y, 0);
    }

    #[test]
    fn sticks_can_swap_roles() {
        let config = Config { left_stick: StickRole::Scroll, right_stick: StickRole::Pointer, ..Config::default() };
        let mut e = Engine::new(config, true);
        e.tick(ms(0));
        e.handle(Input::Axis(Axis::LeftY, -1.0), ms(0));
        e.handle(Input::Axis(Axis::RightX, 1.0), ms(0));
        let actions = e.tick(ms(8));
        assert!(actions.iter().any(|a| matches!(a, Action::Scroll(v, 0) if *v > 0)), "{actions:?}");
        assert!(actions.iter().any(|a| matches!(a, Action::Move(x, 0) if *x > 0)), "{actions:?}");
    }

    #[test]
    fn stick_inside_deadzone_does_nothing() {
        let mut e = engine();
        e.tick(ms(0));
        e.handle(Input::Axis(Axis::LeftX, 0.1), ms(0));
        e.handle(Input::Axis(Axis::RightY, -0.1), ms(0));
        assert_eq!(e.tick(ms(8)), []);
    }

    #[test]
    fn left_stick_click_toggles_precise_mode() {
        let mut normal = engine();
        let normal_x = total_motion(&steer_right_for(&mut normal, 125)).0;

        let mut precise = engine();
        precise.handle(press(Button::LeftStickClick), ms(0));
        let precise_x = total_motion(&steer_right_for(&mut precise, 125)).0;
        assert!((normal_x as f32 * 0.3 - precise_x as f32).abs() < 3.0, "{normal_x} {precise_x}");

        precise.handle(release(Button::LeftStickClick), ms(0));
        precise.handle(press(Button::LeftStickClick), ms(0));
        assert!(!precise.precise);
    }

    #[test]
    fn circle_reverts_after_idle() {
        let mut e = engine();
        steer_right_for(&mut e, 1);
        e.handle(Input::Axis(Axis::LeftX, 0.0), ms(8));
        assert_eq!(e.tick(ms(900)), []);
        assert_eq!(e.tick(ms(1008)), [Action::Steering(false)]);
    }

    #[test]
    fn real_mouse_reverts_circle_immediately() {
        let mut e = engine();
        steer_right_for(&mut e, 1);
        assert_eq!(e.handle(Input::RealMouseMoved, ms(10)), [Action::Steering(false)]);
        // The stick is still deflected, but the real mouse wins for a moment.
        assert!(!e.tick(ms(50)).contains(&Action::Steering(true)));
        assert!(e.tick(ms(600)).contains(&Action::Steering(true)));
    }

    #[test]
    fn right_stick_scrolls_up_when_pushed_up() {
        let mut e = engine();
        e.tick(ms(0));
        e.handle(Input::Axis(Axis::RightY, -1.0), ms(0));
        let Action::Scroll(vertical, horizontal) = e.tick(ms(8))[0] else {
            panic!("expected a scroll");
        };
        assert!(vertical > 0);
        assert_eq!(horizontal, 0);
    }

    #[test]
    fn xbox_tap_opens_launcher() {
        let mut e = engine();
        e.handle(press(Button::Xbox), ms(0));
        assert_eq!(e.tick(ms(100)), []);
        assert_eq!(e.handle(release(Button::Xbox), ms(150)), [Action::Launcher]);
    }

    #[test]
    fn xbox_hold_toggles_once_without_opening_launcher() {
        let mut e = engine();
        e.handle(press(Button::Xbox), ms(0));
        assert_eq!(e.tick(ms(1000)), [Action::SetEnabled(false), Action::Rumble]);
        assert_eq!(e.tick(ms(3000)), []);
        assert_eq!(e.handle(release(Button::Xbox), ms(3100)), []);
    }

    #[test]
    fn disabled_ignores_everything_except_xbox_hold() {
        let mut e = Engine::new(Config::default(), false);
        for input in [
            press(Button::A),
            press(Button::B),
            press(Button::Menu),
            Input::Axis(Axis::LeftX, 1.0),
            Input::Axis(Axis::RightTrigger, 1.0),
            Input::Axis(Axis::DpadX, 1.0),
        ] {
            assert_eq!(e.handle(input, ms(0)), [], "{input:?}");
        }
        assert_eq!(e.tick(ms(8)), []);

        e.handle(press(Button::Xbox), ms(100));
        assert_eq!(e.handle(release(Button::Xbox), ms(200)), [], "tap must not open launcher");
        e.handle(press(Button::Xbox), ms(300));
        assert_eq!(e.tick(ms(1300))[..2], [Action::SetEnabled(true), Action::Rumble]);
    }

    #[test]
    fn disabling_releases_everything_held() {
        let mut e = engine();
        e.handle(press(Button::A), ms(0));
        e.handle(press(Button::View), ms(0));
        e.handle(Input::Axis(Axis::DpadX, -1.0), ms(0));
        steer_right_for(&mut e, 1);
        let actions = e.handle(Input::ToggleEnabled, ms(20));
        for expected in [
            Action::MouseUp(MouseButton::Left),
            Action::SwitcherHold(false),
            Action::KeyUp(Key::Left),
            Action::Steering(false),
            Action::SetEnabled(false),
        ] {
            assert!(actions.contains(&expected), "missing {expected:?} in {actions:?}");
        }
        assert_eq!(e.handle(release(Button::A), ms(30)), [], "no second release");
    }

    #[test]
    fn disconnect_releases_held_input_and_stops_motion() {
        let mut e = engine();
        e.tick(ms(0));
        e.handle(press(Button::A), ms(0));
        e.handle(Input::Axis(Axis::LeftX, 1.0), ms(0));
        let actions = e.handle(Input::Disconnected, ms(10));
        assert!(actions.contains(&Action::MouseUp(MouseButton::Left)));
        assert_eq!(total_motion(&e.tick(ms(20))), (0, 0));
    }
}
