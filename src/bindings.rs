//! What each part of the controller does: the actions a button can have, the keys and shortcuts
//! it can send, and how they are written in the settings file.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A key the virtual keyboard can press.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Esc,
    Tab,
    Enter,
    Space,
    Backspace,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    /// A letter from `b'a'` to `b'z'`.
    Letter(u8),
    /// A digit from 0 to 9.
    Digit(u8),
    /// A function key from F1 to F12.
    F(u8),
    Ctrl,
    Shift,
    Alt,
    /// Command on a Mac, the Windows or Super key elsewhere.
    Meta,
    PlayPause,
    NextTrack,
    PreviousTrack,
    VolumeUp,
    VolumeDown,
    Mute,
}

const NAMED_KEYS: [(Key, &str); 18] = [
    (Key::Up, "up"),
    (Key::Down, "down"),
    (Key::Left, "left"),
    (Key::Right, "right"),
    (Key::Esc, "escape"),
    (Key::Tab, "tab"),
    (Key::Enter, "enter"),
    (Key::Space, "space"),
    (Key::Backspace, "backspace"),
    (Key::Delete, "delete"),
    (Key::Home, "home"),
    (Key::End, "end"),
    (Key::PageUp, "page-up"),
    (Key::PageDown, "page-down"),
    (Key::Ctrl, "ctrl"),
    (Key::Shift, "shift"),
    (Key::Alt, "alt"),
    (Key::Meta, "cmd"),
];

const KEY_ALIASES: [(&str, Key); 9] = [
    ("esc", Key::Esc),
    ("return", Key::Enter),
    ("control", Key::Ctrl),
    ("option", Key::Alt),
    ("opt", Key::Alt),
    ("command", Key::Meta),
    ("meta", Key::Meta),
    ("super", Key::Meta),
    ("win", Key::Meta),
];

impl Key {
    /// Every key Stickmouse can press, for registering the virtual keyboard.
    pub fn all() -> Vec<Key> {
        let mut keys: Vec<Key> = NAMED_KEYS.iter().map(|(key, _)| *key).collect();
        keys.extend((b'a'..=b'z').map(Key::Letter));
        keys.extend((0..=9).map(Key::Digit));
        keys.extend((1..=12).map(Key::F));
        keys.extend([Key::PlayPause, Key::NextTrack, Key::PreviousTrack, Key::VolumeUp, Key::VolumeDown, Key::Mute]);
        keys
    }

    pub fn is_modifier(self) -> bool {
        matches!(self, Key::Ctrl | Key::Shift | Key::Alt | Key::Meta)
    }

    fn name(self) -> String {
        match self {
            Key::Letter(letter) => (letter as char).to_string(),
            Key::Digit(digit) => digit.to_string(),
            Key::F(number) => format!("f{number}"),
            key => NAMED_KEYS
                .iter()
                .find(|(named, _)| *named == key)
                .map_or_else(|| format!("{key:?}").to_lowercase(), |(_, name)| name.to_string()),
        }
    }

    fn parse(name: &str) -> Option<Key> {
        let name = name.trim().to_lowercase();
        if let Some((key, _)) = NAMED_KEYS.iter().find(|(_, named)| *named == name) {
            return Some(*key);
        }
        if let Some((_, key)) = KEY_ALIASES.iter().find(|(alias, _)| *alias == name) {
            return Some(*key);
        }
        let bytes = name.as_bytes();
        match bytes {
            [letter @ b'a'..=b'z'] => Some(Key::Letter(*letter)),
            [digit @ b'0'..=b'9'] => Some(Key::Digit(digit - b'0')),
            [b'f', rest @ ..] => {
                let number: u8 = std::str::from_utf8(rest).ok()?.parse().ok()?;
                (1..=12).contains(&number).then_some(Key::F(number))
            }
            _ => None,
        }
    }

    /// How the key is shown to people, with Mac symbols for modifiers.
    pub fn label(self) -> String {
        match self {
            Key::Ctrl => "⌃".into(),
            Key::Shift => "⇧".into(),
            Key::Alt => "⌥".into(),
            Key::Meta => "⌘".into(),
            Key::Up => "↑".into(),
            Key::Down => "↓".into(),
            Key::Left => "←".into(),
            Key::Right => "→".into(),
            Key::Esc => "Esc".into(),
            Key::Enter => "Return".into(),
            Key::Backspace => "⌫".into(),
            Key::Delete => "⌦".into(),
            Key::PageUp => "Page Up".into(),
            Key::PageDown => "Page Down".into(),
            Key::Letter(letter) => (letter as char).to_ascii_uppercase().to_string(),
            Key::F(number) => format!("F{number}"),
            key => {
                let name = key.name();
                let mut chars = name.chars();
                chars.next().map_or(name.clone(), |first| first.to_uppercase().chain(chars).collect())
            }
        }
    }
}

/// A key, optionally with modifiers held: `escape`, `cmd+w`, `ctrl+shift+t`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    /// Always in the order ctrl, alt, shift, cmd, like macOS shows them.
    pub modifiers: Vec<Key>,
    pub key: Key,
}

const MODIFIER_ORDER: [Key; 4] = [Key::Ctrl, Key::Alt, Key::Shift, Key::Meta];

impl Shortcut {
    pub fn new(mut modifiers: Vec<Key>, key: Key) -> Self {
        modifiers.retain(|modifier| *modifier != key);
        modifiers.sort_by_key(|modifier| MODIFIER_ORDER.iter().position(|m| m == modifier));
        modifiers.dedup();
        Self { modifiers, key }
    }

    pub fn key(key: Key) -> Self {
        Self { modifiers: Vec::new(), key }
    }

    fn parse(text: &str) -> Option<Self> {
        let mut parts: Vec<Key> = text.split('+').map(Key::parse).collect::<Option<_>>()?;
        let key = parts.pop()?;
        if parts.iter().any(|part| !part.is_modifier()) {
            return None;
        }
        Some(Self::new(parts, key))
    }

    pub fn label(&self) -> String {
        let modifiers: String = self.modifiers.iter().map(|m| m.label()).collect();
        format!("{modifiers}{}", self.key.label())
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for modifier in &self.modifiers {
            write!(f, "{}+", modifier.name())?;
        }
        write!(f, "{}", self.key.name())
    }
}

/// Something a button, trigger or D-pad direction can do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    None,
    /// Held while the button is held, so holding it drags.
    LeftClick,
    RightClick,
    MiddleClick,
    /// Slows the pointer down until pressed again.
    PreciseMode,
    /// Alt+Tab (Command+Tab on a Mac): each press moves one app further.
    AppSwitcher,
    /// The application launcher: the Meta key on Linux, Spotlight on a Mac.
    Launcher,
    OnScreenKeyboard,
    /// Mission Control on a Mac, the window overview on KDE Plasma.
    MissionControl,
    PlayPause,
    NextTrack,
    PreviousTrack,
    /// Repeats while held.
    VolumeUp,
    /// Repeats while held.
    VolumeDown,
    Mute,
    /// Held while the button is held, so the system repeats it like a real key.
    Keys(Shortcut),
}

/// The commands with a fixed name, in the order settings show them.
pub const NAMED_COMMANDS: [(&str, &str); 15] = [
    ("none", "Nothing"),
    ("left-click", "Left click"),
    ("right-click", "Right click"),
    ("middle-click", "Middle click"),
    ("precise-mode", "Precise pointer mode"),
    ("app-switcher", "Switch apps"),
    ("launcher", "Launcher / Spotlight"),
    ("mission-control", "Mission Control / Overview"),
    ("on-screen-keyboard", "On-screen keyboard (Linux)"),
    ("play-pause", "Play / pause"),
    ("next-track", "Next track"),
    ("previous-track", "Previous track"),
    ("volume-up", "Volume up"),
    ("volume-down", "Volume down"),
    ("mute", "Mute"),
];

impl Command {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.trim() {
            "none" | "" => Command::None,
            "left-click" => Command::LeftClick,
            "right-click" => Command::RightClick,
            "middle-click" => Command::MiddleClick,
            "precise-mode" => Command::PreciseMode,
            "app-switcher" => Command::AppSwitcher,
            "launcher" => Command::Launcher,
            "on-screen-keyboard" => Command::OnScreenKeyboard,
            "mission-control" => Command::MissionControl,
            "play-pause" => Command::PlayPause,
            "next-track" => Command::NextTrack,
            "previous-track" => Command::PreviousTrack,
            "volume-up" => Command::VolumeUp,
            "volume-down" => Command::VolumeDown,
            "mute" => Command::Mute,
            other => Command::Keys(Shortcut::parse(other)?),
        })
    }

    /// How the command is shown in settings.
    pub fn label(&self) -> String {
        match self {
            Command::Keys(shortcut) => format!("Key: {}", shortcut.label()),
            other => {
                let name = other.to_string();
                NAMED_COMMANDS.iter().find(|(named, _)| *named == name).map_or(name, |(_, label)| label.to_string())
            }
        }
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Command::None => "none",
            Command::LeftClick => "left-click",
            Command::RightClick => "right-click",
            Command::MiddleClick => "middle-click",
            Command::PreciseMode => "precise-mode",
            Command::AppSwitcher => "app-switcher",
            Command::Launcher => "launcher",
            Command::OnScreenKeyboard => "on-screen-keyboard",
            Command::MissionControl => "mission-control",
            Command::PlayPause => "play-pause",
            Command::NextTrack => "next-track",
            Command::PreviousTrack => "previous-track",
            Command::VolumeUp => "volume-up",
            Command::VolumeDown => "volume-down",
            Command::Mute => "mute",
            Command::Keys(shortcut) => return write!(f, "{shortcut}"),
        };
        f.write_str(name)
    }
}

/// What a stick does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StickRole {
    Pointer,
    Scroll,
    None,
}

impl StickRole {
    pub const ALL: [StickRole; 3] = [StickRole::Pointer, StickRole::Scroll, StickRole::None];

    pub fn label(self) -> &'static str {
        match self {
            StickRole::Pointer => "Move the pointer",
            StickRole::Scroll => "Scroll",
            StickRole::None => "Nothing",
        }
    }
}

/// Every part of the controller that can be given a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Control {
    A,
    B,
    X,
    Y,
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    View,
    Menu,
    LeftStickClick,
    RightStickClick,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    /// Its press can be changed; holding it always turns Stickmouse on or off.
    Xbox,
}

impl Control {
    pub const ALL: [Control; 17] = [
        Control::A,
        Control::B,
        Control::X,
        Control::Y,
        Control::LeftBumper,
        Control::RightBumper,
        Control::LeftTrigger,
        Control::RightTrigger,
        Control::View,
        Control::Menu,
        Control::LeftStickClick,
        Control::RightStickClick,
        Control::DpadUp,
        Control::DpadDown,
        Control::DpadLeft,
        Control::DpadRight,
        Control::Xbox,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|control| *control == self).unwrap()
    }

    /// The name in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Control::A => "a",
            Control::B => "b",
            Control::X => "x",
            Control::Y => "y",
            Control::LeftBumper => "left_bumper",
            Control::RightBumper => "right_bumper",
            Control::LeftTrigger => "left_trigger",
            Control::RightTrigger => "right_trigger",
            Control::View => "view",
            Control::Menu => "menu",
            Control::LeftStickClick => "left_stick_click",
            Control::RightStickClick => "right_stick_click",
            Control::DpadUp => "dpad_up",
            Control::DpadDown => "dpad_down",
            Control::DpadLeft => "dpad_left",
            Control::DpadRight => "dpad_right",
            Control::Xbox => "xbox",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Control::A => "A",
            Control::B => "B",
            Control::X => "X",
            Control::Y => "Y",
            Control::LeftBumper => "Left bumper (LB)",
            Control::RightBumper => "Right bumper (RB)",
            Control::LeftTrigger => "Left trigger (LT)",
            Control::RightTrigger => "Right trigger (RT)",
            Control::View => "View",
            Control::Menu => "Menu",
            Control::LeftStickClick => "Left stick click",
            Control::RightStickClick => "Right stick click",
            Control::DpadUp => "D-pad up",
            Control::DpadDown => "D-pad down",
            Control::DpadLeft => "D-pad left",
            Control::DpadRight => "D-pad right",
            Control::Xbox => "Xbox button",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub press: Command,
    /// Done instead of `press` when the control is held for `hold_ms`. `None` reacts at once.
    pub hold: Command,
}

/// The command for every control.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bindings(Vec<Binding>);

impl Bindings {
    pub fn get(&self, control: Control) -> &Binding {
        &self.0[control.index()]
    }

    pub fn set(&mut self, control: Control, binding: Binding) {
        self.0[control.index()] = binding;
    }
}

impl Default for Bindings {
    fn default() -> Self {
        let binding = |control| {
            let (press, hold) = match control {
                Control::A => (Command::LeftClick, Command::None),
                Control::B => (Command::RightClick, Command::Keys(Shortcut::key(Key::Esc))),
                Control::X => (Command::MiddleClick, Command::None),
                // macOS does not let other apps show its on-screen keyboard.
                Control::Y if cfg!(target_os = "macos") => (Command::MissionControl, Command::None),
                Control::Y => (Command::OnScreenKeyboard, Command::None),
                Control::LeftBumper => (Command::PreviousTrack, Command::None),
                Control::RightBumper => (Command::NextTrack, Command::None),
                Control::LeftTrigger => (Command::VolumeDown, Command::None),
                Control::RightTrigger => (Command::VolumeUp, Command::None),
                Control::View => (Command::AppSwitcher, Command::None),
                Control::Menu => (Command::PlayPause, Command::None),
                Control::LeftStickClick => (Command::PreciseMode, Command::None),
                Control::RightStickClick => (Command::Mute, Command::None),
                Control::DpadUp => (Command::Keys(Shortcut::key(Key::Up)), Command::None),
                Control::DpadDown => (Command::Keys(Shortcut::key(Key::Down)), Command::None),
                Control::DpadLeft => (Command::Keys(Shortcut::key(Key::Left)), Command::None),
                Control::DpadRight => (Command::Keys(Shortcut::key(Key::Right)), Command::None),
                Control::Xbox => (Command::Launcher, Command::None),
            };
            Binding { press, hold }
        };
        Self(Control::ALL.iter().map(|control| binding(*control)).collect())
    }
}

impl Bindings {
    /// The `[buttons]` lines of the settings file. A hold line is written whenever it differs
    /// from nothing or the default has one, so clearing a default hold survives a reload.
    pub fn to_lines(&self) -> Vec<String> {
        let defaults = Bindings::default();
        let mut lines = Vec::new();
        for control in Control::ALL {
            let binding = self.get(control);
            lines.push(format!("{} = \"{}\"", control.key(), binding.press));
            let default_hold = &defaults.get(control).hold;
            if control != Control::Xbox && (binding.hold != Command::None || *default_hold != Command::None) {
                lines.push(format!("{}_hold = \"{}\"", control.key(), binding.hold));
            }
        }
        lines
    }
}

impl Serialize for Bindings {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = BTreeMap::new();
        for control in Control::ALL {
            let binding = self.get(control);
            map.insert(control.key().to_string(), binding.press.to_string());
            map.insert(format!("{}_hold", control.key()), binding.hold.to_string());
        }
        map.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Bindings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let map = BTreeMap::<String, String>::deserialize(deserializer)?;
        let mut bindings = Bindings::default();
        for (name, value) in map {
            let (control_name, hold) = match name.strip_suffix("_hold") {
                Some(base) => (base, true),
                None => (name.as_str(), false),
            };
            let control = Control::ALL
                .into_iter()
                .find(|control| control.key() == control_name)
                .ok_or_else(|| D::Error::custom(format!("unknown button `{name}`")))?;
            let command = Command::parse(&value)
                .ok_or_else(|| D::Error::custom(format!("unknown action `{value}` for `{name}`")))?;
            let mut binding = bindings.get(control).clone();
            if hold {
                binding.hold = command;
            } else {
                binding.press = command;
            }
            bindings.set(control, binding);
        }
        Ok(bindings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_parse_and_print_in_a_fixed_order() {
        let shortcut = Shortcut::parse("Shift+cmd+W").unwrap();
        assert_eq!(shortcut.to_string(), "shift+cmd+w");
        assert_eq!(Shortcut::parse("command+option+f5").unwrap().to_string(), "alt+cmd+f5");
        assert_eq!(shortcut.label(), "⇧⌘W");
    }

    #[test]
    fn bad_shortcuts_are_rejected() {
        for text in ["cmd+", "w+cmd", "f13", "hyper+a", "a+b"] {
            assert!(Shortcut::parse(text).is_none(), "{text}");
        }
    }

    #[test]
    fn every_command_round_trips() {
        for (name, _) in NAMED_COMMANDS {
            assert_eq!(Command::parse(name).unwrap().to_string(), name);
        }
        for text in ["escape", "ctrl+shift+t", "f12", "7", "page-down"] {
            assert_eq!(Command::parse(text).unwrap().to_string(), text);
        }
    }

    #[test]
    fn every_key_has_a_name_that_parses_back() {
        for key in Key::all() {
            let name = key.name();
            if !matches!(
                key,
                Key::PlayPause | Key::NextTrack | Key::PreviousTrack | Key::VolumeUp | Key::VolumeDown | Key::Mute
            ) {
                assert_eq!(Key::parse(&name), Some(key), "{name}");
            }
        }
    }

    #[test]
    fn written_bindings_read_back_the_same() {
        let mut bindings = Bindings::default();
        bindings.set(Control::A, Binding { press: Command::parse("cmd+w").unwrap(), hold: Command::Mute });
        bindings.set(Control::B, Binding { press: Command::RightClick, hold: Command::None });
        let text = bindings.to_lines().join("\n");
        let parsed: Bindings = toml::from_str(&text).unwrap();
        assert_eq!(parsed, bindings);
    }

    #[test]
    fn unknown_buttons_and_actions_are_rejected() {
        assert!(toml::from_str::<Bindings>("z = \"mute\"").is_err());
        assert!(toml::from_str::<Bindings>("a = \"dance\"").is_err());
    }
}
