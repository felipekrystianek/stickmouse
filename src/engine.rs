//! Turns controller input into desktop actions. Has no platform code and takes time as an
//! argument, so the whole button layout can be tested without hardware.

use std::time::Duration;

use crate::config::Config;

const MAX_TICK: Duration = Duration::from_millis(50);
const TRIGGER_PRESS: f32 = 0.5;
const TRIGGER_RELEASE: f32 = 0.3;
const REAL_MOUSE_PRIORITY: Duration = Duration::from_millis(500);

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
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Esc,
    Alt,
    Tab,
    Meta,
    PlayPause,
    NextTrack,
    PreviousTrack,
    VolumeUp,
    VolumeDown,
    Mute,
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
    ToggleKeyboard,
    Steering(bool),
    SetEnabled(bool),
    Rumble,
}

#[derive(Default)]
struct Trigger {
    value: f32,
    pressed: bool,
    next_repeat: Duration,
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
    b_down_at: Option<Duration>,
    xbox_down_at: Option<Duration>,
    xbox_toggled: bool,
    left_held: bool,
    middle_held: bool,
    dpad: (i32, i32),
    alt_release_at: Option<Duration>,
    volume_down: Trigger,
    volume_up: Trigger,
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
            b_down_at: None,
            xbox_down_at: None,
            xbox_toggled: false,
            left_held: false,
            middle_held: false,
            dpad: (0, 0),
            alt_release_at: None,
            volume_down: Trigger::default(),
            volume_up: Trigger::default(),
            last_tick: None,
            actions: Vec::new(),
        }
    }

    pub fn handle(&mut self, input: Input, now: Duration) -> Vec<Action> {
        match input {
            Input::Button(Button::Xbox, pressed) => self.xbox(pressed, now),
            Input::Button(button, pressed) if self.enabled => self.button(button, pressed, now),
            Input::Button(..) => {}
            Input::Axis(axis, value) => self.axis(axis, value),
            Input::RealMouseMoved => {
                self.real_mouse_at = Some(now);
                self.set_steering(false);
            }
            Input::Disconnected => {
                self.release_all();
                self.left = (0.0, 0.0);
                self.right = (0.0, 0.0);
                self.volume_down = Trigger::default();
                self.volume_up = Trigger::default();
                self.xbox_down_at = None;
                self.xbox_toggled = false;
            }
            Input::ToggleEnabled => self.toggle(),
        }
        std::mem::take(&mut self.actions)
    }

    /// Advances everything time-based: stick motion, button holds and key repeats.
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
            self.tick_b(now);
            self.tick_alt(now);
            self.tick_triggers(now);
            self.tick_sticks(dt, now);
        }
        std::mem::take(&mut self.actions)
    }

    /// Lets go of everything held down. Call before exiting.
    pub fn shutdown(&mut self) -> Vec<Action> {
        self.release_all();
        std::mem::take(&mut self.actions)
    }

    fn xbox(&mut self, pressed: bool, now: Duration) {
        if pressed {
            self.xbox_down_at = Some(now);
            self.xbox_toggled = false;
        } else {
            if self.xbox_down_at.take().is_some() && !self.xbox_toggled && self.enabled {
                self.actions.push(Action::Tap(Key::Meta));
            }
            self.xbox_toggled = false;
        }
    }

    fn button(&mut self, button: Button, pressed: bool, now: Duration) {
        match (button, pressed) {
            (Button::A, true) => {
                self.left_held = true;
                self.actions.push(Action::MouseDown(MouseButton::Left));
            }
            (Button::A, false) if self.left_held => {
                self.left_held = false;
                self.actions.push(Action::MouseUp(MouseButton::Left));
            }
            (Button::X, true) => {
                self.middle_held = true;
                self.actions.push(Action::MouseDown(MouseButton::Middle));
            }
            (Button::X, false) if self.middle_held => {
                self.middle_held = false;
                self.actions.push(Action::MouseUp(MouseButton::Middle));
            }
            (Button::B, true) => self.b_down_at = Some(now),
            (Button::B, false) => {
                if self.b_down_at.take().is_some() {
                    self.actions.push(Action::Click(MouseButton::Right));
                }
            }
            (Button::Y, true) => self.actions.push(Action::ToggleKeyboard),
            (Button::LeftBumper, true) => self.actions.push(Action::Tap(Key::PreviousTrack)),
            (Button::RightBumper, true) => self.actions.push(Action::Tap(Key::NextTrack)),
            (Button::Menu, true) => self.actions.push(Action::Tap(Key::PlayPause)),
            (Button::RightStickClick, true) => self.actions.push(Action::Tap(Key::Mute)),
            (Button::LeftStickClick, true) => self.precise = !self.precise,
            (Button::View, true) => {
                if self.alt_release_at.is_none() {
                    self.actions.push(Action::KeyDown(Key::Alt));
                }
                self.actions.push(Action::Tap(Key::Tab));
                self.alt_release_at = Some(now + self.config.alt_tab_commit());
            }
            _ => {}
        }
    }

    fn axis(&mut self, axis: Axis, value: f32) {
        match axis {
            Axis::LeftX => self.left.0 = value,
            Axis::LeftY => self.left.1 = value,
            Axis::RightX => self.right.0 = value,
            Axis::RightY => self.right.1 = value,
            Axis::LeftTrigger => self.volume_down.value = value,
            Axis::RightTrigger => self.volume_up.value = value,
            Axis::DpadX => self.dpad_axis(value, true),
            Axis::DpadY => self.dpad_axis(value, false),
        }
    }

    fn dpad_axis(&mut self, value: f32, horizontal: bool) {
        let direction = value.round() as i32;
        let (current, negative, positive) = if horizontal {
            (&mut self.dpad.0, Key::Left, Key::Right)
        } else {
            (&mut self.dpad.1, Key::Up, Key::Down)
        };
        let previous = *current;
        if !self.enabled || previous == direction {
            if !self.enabled {
                *current = 0;
            }
            return;
        }
        *current = direction;
        let key_for = |d| if d < 0 { negative } else { positive };
        if previous != 0 {
            self.actions.push(Action::KeyUp(key_for(previous)));
        }
        if direction != 0 {
            self.actions.push(Action::KeyDown(key_for(direction)));
        }
    }

    fn tick_b(&mut self, now: Duration) {
        if let Some(down_at) = self.b_down_at
            && now.saturating_sub(down_at) >= self.config.b_hold()
        {
            self.b_down_at = None;
            self.actions.push(Action::Tap(Key::Esc));
        }
    }

    fn tick_alt(&mut self, now: Duration) {
        if self.alt_release_at.is_some_and(|at| now >= at) {
            self.alt_release_at = None;
            self.actions.push(Action::KeyUp(Key::Alt));
        }
    }

    fn tick_triggers(&mut self, now: Duration) {
        let (first, repeat) = (self.config.volume_delay(), self.config.volume_repeat());
        for (trigger, key) in [
            (&mut self.volume_down, Key::VolumeDown),
            (&mut self.volume_up, Key::VolumeUp),
        ] {
            if !trigger.pressed && trigger.value >= TRIGGER_PRESS {
                trigger.pressed = true;
                trigger.next_repeat = now + first;
                self.actions.push(Action::Tap(key));
            } else if trigger.pressed && trigger.value < TRIGGER_RELEASE {
                trigger.pressed = false;
            } else if trigger.pressed && now >= trigger.next_repeat {
                trigger.next_repeat = now + repeat;
                self.actions.push(Action::Tap(key));
            }
        }
    }

    fn tick_sticks(&mut self, dt: f32, now: Duration) {
        let pointer = shape_stick(self.left, self.config.stick_deadzone, self.config.stick_curve);
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

        let scroll = shape_stick(self.right, self.config.stick_deadzone, self.config.stick_curve);
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
        if std::mem::take(&mut self.left_held) {
            self.actions.push(Action::MouseUp(MouseButton::Left));
        }
        if std::mem::take(&mut self.middle_held) {
            self.actions.push(Action::MouseUp(MouseButton::Middle));
        }
        self.b_down_at = None;
        self.dpad_axis_release();
        if self.alt_release_at.take().is_some() {
            self.actions.push(Action::KeyUp(Key::Alt));
        }
        self.volume_down.pressed = false;
        self.volume_up.pressed = false;
        self.set_steering(false);
    }

    fn dpad_axis_release(&mut self) {
        let (x, y) = std::mem::take(&mut self.dpad);
        match x {
            -1 => self.actions.push(Action::KeyUp(Key::Left)),
            1 => self.actions.push(Action::KeyUp(Key::Right)),
            _ => {}
        }
        match y {
            -1 => self.actions.push(Action::KeyUp(Key::Up)),
            1 => self.actions.push(Action::KeyUp(Key::Down)),
            _ => {}
        }
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

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn engine() -> Engine {
        Engine::new(Config::default(), true)
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
        let cases = [
            (Button::Y, Action::ToggleKeyboard),
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
    fn view_cycles_windows_then_commits() {
        let mut e = engine();
        assert_eq!(
            e.handle(press(Button::View), ms(0)),
            [Action::KeyDown(Key::Alt), Action::Tap(Key::Tab)]
        );
        assert_eq!(e.tick(ms(500)), []);
        assert_eq!(e.handle(press(Button::View), ms(500)), [Action::Tap(Key::Tab)]);
        assert_eq!(e.tick(ms(1200)), []);
        assert_eq!(e.tick(ms(1300)), [Action::KeyUp(Key::Alt)]);
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
        e.handle(Input::Axis(Axis::RightTrigger, 0.9), ms(0));
        assert_eq!(e.tick(ms(0)), [Action::Tap(Key::VolumeUp)]);
        assert_eq!(e.tick(ms(100)), []);
        assert_eq!(e.tick(ms(400)), [Action::Tap(Key::VolumeUp)]);
        assert_eq!(e.tick(ms(500)), []);
        assert_eq!(e.tick(ms(550)), [Action::Tap(Key::VolumeUp)]);
        e.handle(Input::Axis(Axis::RightTrigger, 0.0), ms(560));
        assert_eq!(e.tick(ms(570)), []);
        assert_eq!(e.tick(ms(2000)), []);
        e.handle(Input::Axis(Axis::LeftTrigger, 1.0), ms(2000));
        assert_eq!(e.tick(ms(2000)), [Action::Tap(Key::VolumeDown)]);
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
        assert_eq!(e.handle(release(Button::Xbox), ms(150)), [Action::Tap(Key::Meta)]);
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
            Action::KeyUp(Key::Alt),
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
