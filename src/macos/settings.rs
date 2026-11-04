//! The settings window. Every change is saved to config.toml and applied right away.

use std::cell::RefCell;
use std::fs;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{
    NSButton, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSImageView, NSSlider,
    NSTextField, NSView, NSWindow,
};
use objc2_foundation::NSString;

use super::{cursor_theme, desktop, permissions, tray, ui};
use crate::Message;
use crate::config::Config;

const CONTENT_WIDTH: f64 = 520.0;
const TITLE_WIDTH: f64 = 180.0;
const VALUE_WIDTH: f64 = 90.0;
const REFRESH_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy)]
enum Format {
    PixelsPerSecond,
    NotchesPerSecond,
    Percent,
    Times,
    Decimal,
    Millis,
}

struct Field {
    title: &'static str,
    min: f64,
    max: f64,
    format: Format,
    get: fn(&Config) -> f64,
    set: fn(&mut Config, f64),
}

fn sections() -> Vec<(&'static str, Vec<Field>)> {
    vec![
        (
            "Pointer",
            vec![
                Field {
                    title: "Pointer speed",
                    min: 300.0,
                    max: 4000.0,
                    format: Format::PixelsPerSecond,
                    get: |c| f64::from(c.pointer_speed),
                    set: |c, v| c.pointer_speed = v.round() as f32,
                },
                Field {
                    title: "Precise mode speed",
                    min: 0.1,
                    max: 1.0,
                    format: Format::Percent,
                    get: |c| f64::from(c.precise_factor),
                    set: |c, v| c.precise_factor = round_to(v, 0.05),
                },
                Field {
                    title: "Stick deadzone",
                    min: 0.0,
                    max: 0.4,
                    format: Format::Percent,
                    get: |c| f64::from(c.stick_deadzone),
                    set: |c, v| c.stick_deadzone = round_to(v, 0.01),
                },
                Field {
                    title: "Response curve",
                    min: 1.0,
                    max: 4.0,
                    format: Format::Decimal,
                    get: |c| f64::from(c.stick_curve),
                    set: |c, v| c.stick_curve = round_to(v, 0.1),
                },
                Field {
                    title: "Steering circle size",
                    min: 0.5,
                    max: 3.0,
                    format: Format::Times,
                    get: |c| f64::from(c.circle_scale),
                    set: |c, v| c.circle_scale = round_to(v, 0.1),
                },
            ],
        ),
        (
            "Scrolling",
            vec![Field {
                title: "Scroll speed",
                min: 300.0,
                max: 6000.0,
                format: Format::NotchesPerSecond,
                get: |c| f64::from(c.scroll_speed),
                set: |c, v| c.scroll_speed = (v / 60.0).round() as f32 * 60.0,
            }],
        ),
        (
            "Buttons",
            vec![
                Field {
                    title: "Hold B for Esc after",
                    min: 150.0,
                    max: 1500.0,
                    format: Format::Millis,
                    get: |c| c.b_hold_ms as f64,
                    set: |c, v| c.b_hold_ms = round_ms(v),
                },
                Field {
                    title: "Hold Xbox to turn off after",
                    min: 300.0,
                    max: 3000.0,
                    format: Format::Millis,
                    get: |c| c.xbox_hold_ms as f64,
                    set: |c, v| c.xbox_hold_ms = round_ms(v),
                },
                Field {
                    title: "App switcher picks after",
                    min: 300.0,
                    max: 2000.0,
                    format: Format::Millis,
                    get: |c| c.alt_tab_commit_ms as f64,
                    set: |c, v| c.alt_tab_commit_ms = round_ms(v),
                },
                Field {
                    title: "Hide circle after",
                    min: 200.0,
                    max: 5000.0,
                    format: Format::Millis,
                    get: |c| c.steering_idle_ms as f64,
                    set: |c, v| c.steering_idle_ms = round_ms(v),
                },
            ],
        ),
        (
            "Volume triggers",
            vec![
                Field {
                    title: "Repeat after",
                    min: 100.0,
                    max: 1500.0,
                    format: Format::Millis,
                    get: |c| c.volume_delay_ms as f64,
                    set: |c, v| c.volume_delay_ms = round_ms(v),
                },
                Field {
                    title: "Repeat every",
                    min: 50.0,
                    max: 600.0,
                    format: Format::Millis,
                    get: |c| c.volume_repeat_ms as f64,
                    set: |c, v| c.volume_repeat_ms = round_ms(v),
                },
            ],
        ),
    ]
}

fn round_to(value: f64, step: f64) -> f32 {
    ((value / step).round() * step) as f32
}

fn round_ms(value: f64) -> u64 {
    ((value / 10.0).round() * 10.0) as u64
}

fn describe(format: Format, value: f64) -> String {
    match format {
        Format::PixelsPerSecond => format!("{value:.0} px/s"),
        Format::NotchesPerSecond => format!("{:.1} notches/s", value / 120.0),
        Format::Percent => format!("{:.0} %", value * 100.0),
        Format::Times => format!("{value:.1}×"),
        Format::Decimal => format!("{value:.1}"),
        Format::Millis => format!("{value:.0} ms"),
    }
}

struct Window {
    window: Retained<NSWindow>,
    fields: Vec<Field>,
    sliders: Vec<Retained<NSSlider>>,
    values: Vec<Retained<NSTextField>>,
    login: Retained<NSButton>,
    access_icon: Retained<NSImageView>,
    access_text: Retained<NSTextField>,
    access_button: Retained<NSButton>,
    config: Config,
    refreshed: Instant,
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
}

fn load_config() -> Config {
    fs::read_to_string(desktop::config_file())
        .ok()
        .and_then(|text| Config::parse(&text).ok())
        .unwrap_or_default()
}

pub fn show(mtm: MainThreadMarker) {
    WINDOW.with_borrow_mut(|slot| {
        let window = slot.get_or_insert_with(|| build(mtm));
        window.config = load_config();
        sync_sliders(window);
        refresh_status(window);
        ui::present(mtm, &window.window);
    });
}

fn build(mtm: MainThreadMarker) -> Window {
    let config = load_config();
    let mut rows: Vec<Retained<NSView>> = Vec::new();
    let mut fields = Vec::new();
    let mut sliders = Vec::new();
    let mut values = Vec::new();

    let header = ui::hstack(
        mtm,
        &[
            &ui::app_icon(mtm, 48.0),
            &ui::vstack(
                mtm,
                &[
                    &ui::bold(mtm, "Stickmouse", 16.0),
                    &ui::secondary(mtm, "Changes are saved and used right away.", 400.0),
                ],
                2.0,
            ),
        ],
        12.0,
    );
    rows.push(header.into_super());

    for (title, section) in sections() {
        rows.push(ui::separator(mtm, CONTENT_WIDTH).into_super());
        rows.push(ui::bold(mtm, title, 13.0).into_super().into_super());
        for field in section {
            let value = (field.get)(&config);
            let slider = ui::slider(mtm, value, field.min, field.max, sel!(sliderChanged:));
            slider.setTag(sliders.len() as isize);
            ui::fixed_width(&slider, CONTENT_WIDTH - TITLE_WIDTH - VALUE_WIDTH - 16.0);
            let name = ui::label(mtm, field.title);
            ui::fixed_width(&name, TITLE_WIDTH);
            let shown = ui::label(mtm, &describe(field.format, value));
            ui::right_aligned(&shown);
            ui::fixed_width(&shown, VALUE_WIDTH);
            rows.push(ui::hstack(mtm, &[&name, &slider, &shown], 8.0).into_super());
            fields.push(field);
            sliders.push(slider);
            values.push(shown);
        }
    }

    rows.push(ui::separator(mtm, CONTENT_WIDTH).into_super());
    rows.push(ui::bold(mtm, "General", 13.0).into_super().into_super());
    let login = ui::checkbox(mtm, "Start Stickmouse when I log in", sel!(toggleLogin:));
    rows.push(login.clone().into_super().into_super());

    let access_icon = ui::symbol(mtm, "checkmark.circle.fill", &NSColor::systemGreenColor(), 18.0);
    let access_text = ui::label(mtm, "");
    let access_button = ui::button(mtm, "Open Privacy Settings", sel!(openAccessibility:));
    rows.push(
        ui::hstack(mtm, &[&access_icon, &access_text, &access_button], 8.0).into_super(),
    );

    let footer = ui::hstack(
        mtm,
        &[
            &ui::button(mtm, "Restore Defaults", sel!(restoreDefaults:)),
            &ui::button(mtm, "Open Config File", sel!(openConfigFile:)),
            &ui::button(mtm, "Setup & Permissions…", sel!(openSetup:)),
        ],
        8.0,
    );
    rows.push(ui::separator(mtm, CONTENT_WIDTH).into_super());
    rows.push(footer.into_super());
    rows.push(
        ui::secondary(
            mtm,
            "Xbox is a trademark of Microsoft Corporation. Stickmouse is not affiliated with or endorsed by Microsoft.",
            CONTENT_WIDTH,
        )
        .into_super()
        .into_super(),
    );

    let views: Vec<&NSView> = rows.iter().map(|view| &**view).collect();
    let content = ui::vstack(mtm, &views, 10.0);
    let window = ui::window(mtm, "Stickmouse Settings", &content);
    Window {
        window,
        fields,
        sliders,
        values,
        login,
        access_icon,
        access_text,
        access_button,
        config,
        refreshed: Instant::now(),
    }
}

pub fn slider_changed(slider: &NSSlider) {
    WINDOW.with_borrow_mut(|slot| {
        let Some(window) = slot.as_mut() else {
            return;
        };
        let index = slider.tag() as usize;
        let Some(field) = window.fields.get(index) else {
            return;
        };
        (field.set)(&mut window.config, slider.doubleValue());
        let value = (field.get)(&window.config);
        window.values[index].setStringValue(&NSString::from_str(&describe(field.format, value)));
        save(&window.config);
    });
}

pub fn restore_defaults() {
    WINDOW.with_borrow_mut(|slot| {
        if let Some(window) = slot.as_mut() {
            window.config = Config::default();
            sync_sliders(window);
            save(&window.config);
        }
    });
}

pub fn open_config_file() {
    permissions::open(&desktop::config_file().to_string_lossy());
}

fn sync_sliders(window: &Window) {
    for (index, field) in window.fields.iter().enumerate() {
        let value = (field.get)(&window.config);
        window.sliders[index].setDoubleValue(value);
        window.values[index].setStringValue(&NSString::from_str(&describe(field.format, value)));
    }
}

fn save(config: &Config) {
    let path = desktop::config_file();
    if let Err(error) = fs::write(&path, config.to_toml()) {
        eprintln!("padcursor: could not write {}: {error}", path.display());
    }
    cursor_theme::set_scale(config.circle_scale);
    tray::send(Message::ConfigChanged(config.clone()));
}

/// Called on the main thread every few milliseconds; checks permissions twice a second.
pub fn refresh() {
    WINDOW.with_borrow_mut(|slot| {
        if let Some(window) = slot.as_mut()
            && window.window.isVisible()
            && window.refreshed.elapsed() >= REFRESH_INTERVAL
        {
            window.refreshed = Instant::now();
            refresh_status(window);
        }
    });
}

fn refresh_status(window: &Window) {
    window.login.setState(if permissions::login_item_enabled() {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    let (symbol, color, text) = if permissions::needs_restart() {
        ("arrow.clockwise.circle.fill", NSColor::systemOrangeColor(), "Accessibility allowed – restart Stickmouse to use it")
    } else if permissions::accessibility_granted() {
        ("checkmark.circle.fill", NSColor::systemGreenColor(), "Accessibility access is on")
    } else {
        ("exclamationmark.triangle.fill", NSColor::systemOrangeColor(), "Accessibility access is off")
    };
    ui::set_symbol(&window.access_icon, symbol, &color);
    window.access_text.setStringValue(&NSString::from_str(text));
    window.access_button.setHidden(permissions::accessibility_granted());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_fits_its_slider() {
        let config = Config::default();
        for (_, fields) in sections() {
            for field in fields {
                let value = (field.get)(&config);
                assert!((field.min..=field.max).contains(&value), "{}", field.title);
            }
        }
    }

    #[test]
    fn setters_round_to_tidy_values() {
        let mut config = Config::default();
        for (_, fields) in sections() {
            for field in fields {
                (field.set)(&mut config, (field.get)(&Config::default()) + 0.0001);
                assert_eq!(
                    describe(field.format, (field.get)(&config)),
                    describe(field.format, (field.get)(&Config::default())),
                    "{}",
                    field.title
                );
            }
        }
    }
}
