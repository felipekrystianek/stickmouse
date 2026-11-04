//! User settings, read from a TOML file. Missing values fall back to the defaults.

use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const DEFAULT_FILE: &str = include_str!("../packaging/config.toml");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub pointer_speed: f32,
    pub precise_factor: f32,
    pub scroll_speed: f32,
    pub stick_deadzone: f32,
    pub stick_curve: f32,
    pub circle_scale: f32,
    pub b_hold_ms: u64,
    pub xbox_hold_ms: u64,
    pub alt_tab_commit_ms: u64,
    pub steering_idle_ms: u64,
    pub volume_delay_ms: u64,
    pub volume_repeat_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            pointer_speed: 1400.0,
            precise_factor: 0.3,
            scroll_speed: 1800.0,
            stick_deadzone: 0.12,
            stick_curve: 2.0,
            circle_scale: 1.5,
            b_hold_ms: 400,
            xbox_hold_ms: 1000,
            alt_tab_commit_ms: 800,
            steering_idle_ms: 1000,
            volume_delay_ms: 400,
            volume_repeat_ms: 150,
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// The settings as a file: the shipped file with its comments, with these values filled in.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn to_toml(&self) -> String {
        let values = toml::Table::try_from(self).expect("settings always serialize");
        let mut text = String::new();
        for line in DEFAULT_FILE.lines() {
            let key = line.split_once(" = ").map(|(key, _)| key);
            match key.and_then(|key| Some((key, values.get(key)?))) {
                Some((key, toml::Value::Float(value))) => {
                    // Settings are f32, so print them as f32 to avoid 0.30000001192092896.
                    text.push_str(&format!("{key} = {:?}", *value as f32));
                }
                Some((key, value)) => text.push_str(&format!("{key} = {value}")),
                None => text.push_str(line),
            }
            text.push('\n');
        }
        text
    }

    pub fn b_hold(&self) -> Duration {
        Duration::from_millis(self.b_hold_ms)
    }

    pub fn xbox_hold(&self) -> Duration {
        Duration::from_millis(self.xbox_hold_ms)
    }

    pub fn alt_tab_commit(&self) -> Duration {
        Duration::from_millis(self.alt_tab_commit_ms)
    }

    pub fn steering_idle(&self) -> Duration {
        Duration::from_millis(self.steering_idle_ms)
    }

    pub fn volume_delay(&self) -> Duration {
        Duration::from_millis(self.volume_delay_ms)
    }

    pub fn volume_repeat(&self) -> Duration {
        Duration::from_millis(self.volume_repeat_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_file_matches_the_defaults() {
        let parsed = Config::parse(DEFAULT_FILE).unwrap();
        assert_eq!(format!("{parsed:?}"), format!("{:?}", Config::default()));
    }

    #[test]
    fn missing_values_use_defaults() {
        let parsed = Config::parse("pointer_speed = 900.0").unwrap();
        assert_eq!(parsed.pointer_speed, 900.0);
        assert_eq!(parsed.b_hold_ms, Config::default().b_hold_ms);
    }

    #[test]
    fn default_settings_write_the_shipped_file() {
        assert_eq!(Config::default().to_toml(), DEFAULT_FILE);
    }

    #[test]
    fn written_settings_read_back_the_same() {
        let config = Config { pointer_speed: 2200.0, precise_factor: 0.45, b_hold_ms: 650, ..Config::default() };
        let parsed = Config::parse(&config.to_toml()).unwrap();
        assert_eq!(format!("{parsed:?}"), format!("{config:?}"));
    }

    #[test]
    fn unknown_settings_are_rejected() {
        assert!(Config::parse("pointer_sped = 900.0").is_err());
    }
}
