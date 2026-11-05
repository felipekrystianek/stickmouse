# Stickmouse

Stickmouse turns an Xbox controller into a mouse and media remote. The left stick moves the
pointer, the right stick scrolls, and the buttons click, control playback and volume, and switch
apps. While you steer, the pointer changes to a small circle, which goes away as soon as you touch
the real mouse.

It runs on macOS (paid, with a free 7-day trial) and on Linux with KDE Plasma (free).

## Default layout

| Control | Action |
|---|---|
| Left stick | Move the pointer |
| Left stick click | Toggle slow, precise pointer |
| Right stick | Scroll, vertically and sideways |
| Right stick click | Mute |
| A | Left click (hold to drag) |
| B | Tap: right click. Hold: Esc |
| X | Middle click |
| Y | macOS: Mission Control. Linux: open or close the on-screen keyboard |
| LB / RB | Previous / next track |
| LT / RT | Volume down / up (hold to keep changing) |
| D-pad | Arrow keys |
| Menu | Play / pause |
| View | Switch apps (each press moves to the next one) |
| Xbox button | Tap: Spotlight on macOS, app launcher on Linux. Hold: turn Stickmouse on or off |

Turn Stickmouse off before playing a game, so the game gets the controller to itself.

## Customizing

Every button, trigger, stick click and D-pad direction can be given a press action and a separate
hold action, and the two sticks can swap roles. On macOS, use Customize Buttons and Settings in the
menu bar icon; changes apply right away. On both systems the settings live in a TOML file:

- macOS: `~/Library/Application Support/Stickmouse/config.toml`
- Linux: `~/.config/stickmouse/config.toml`

The file is created with all defaults and comments on first run. Besides speeds, deadzone, curve
and timings, it has these keys:

```toml
# What each stick does: "pointer", "scroll" or "none".
left_stick = "pointer"
right_stick = "scroll"

[buttons]
y = "cmd+w"                 # press Y to close the window
y_hold = "mission-control"  # hold Y for Mission Control
dpad_up = "volume-up"
right_bumper = "ctrl+shift+t"
x = "none"
```

Button names: `a`, `b`, `x`, `y`, `left_bumper`, `right_bumper`, `left_trigger`, `right_trigger`,
`view`, `menu`, `left_stick_click`, `right_stick_click`, `dpad_up`, `dpad_down`, `dpad_left`,
`dpad_right`, `xbox`. Add `_hold` to a name to set what holding it does (after `hold_ms`).

Actions: `left-click`, `right-click`, `middle-click`, `precise-mode`, `app-switcher`, `launcher`,
`mission-control`, `on-screen-keyboard`, `play-pause`, `next-track`, `previous-track`,
`volume-up`, `volume-down`, `mute`, `none`, or a key or shortcut such as `"escape"`, `"enter"`,
`"f5"` or `"ctrl+shift+t"` (modifiers: `ctrl`, `alt`, `shift`, `cmd`).

Holding the Xbox button always turns Stickmouse on or off; `xbox` only sets what a short press
does. After editing the file, restart Stickmouse (on macOS: Quit from the menu bar icon and open it
again; on Linux: `systemctl --user restart stickmouse`).

## macOS

Requires macOS 14 Sonoma or later. Made and tested for the Xbox Wireless Controller, over Bluetooth or USB;
the default buttons follow the Xbox layout.

### Install

1. Download Stickmouse from [stickmouse.app](https://stickmouse.app) and drag it to Applications.
2. Open it. Stickmouse is not notarized, so macOS may say it "cannot be opened" or "could not be
   verified". Open System Settings > Privacy & Security, scroll down, click Open Anyway next to
   Stickmouse and confirm. This is needed only once.
3. Stickmouse asks for Accessibility access, which macOS requires for moving the pointer, clicking
   and pressing keys. Click Open Settings, switch on Stickmouse, then click Restart Now.

Stickmouse lives in the menu bar, with no Dock icon.

### Pricing

Every feature is free for 7 days. After that, a license costs $5, paid once through Polar.sh.
Buy it from the menu bar icon > Buy Stickmouse or on [stickmouse.app](https://stickmouse.app); the
license key arrives by email and is pasted into the same window.

### Build from source

With Rust installed:

    packaging/macos/build-app.sh

This builds `target/release/Stickmouse.app` and a `.dmg`. Without the maintainer's signing
certificate the app is signed ad hoc, so macOS asks for Accessibility access again after each
rebuild.

## Linux (KDE Plasma)

### Requirements

- Arch Linux or an Arch-based distro such as CachyOS, because the install uses a PKGBUILD
- KDE Plasma on Wayland
- An Xbox controller. Over Bluetooth, install `xpadneo-dkms` and pair the controller first
- Write access to `/dev/uinput` and membership of the `input` group

### Install

Run these in a normal terminal, because `makepkg` asks for your password:

    git clone https://github.com/felipekrystianek/stickmouse.git
    cd stickmouse/packaging
    makepkg -si
    systemctl --user daemon-reload
    systemctl --user enable --now stickmouse

Stickmouse now runs as the `stickmouse.service` user service, starts at every login and shows a
gamepad icon in the system tray.

To make Y open the on-screen keyboard, choose Plasma Keyboard under System Settings > Keyboard >
Virtual Keyboard.

The package also sets `disable_shift_mode=1` for `xpadneo`, so that holding the Xbox button can be
detected. This turns off the driver's own Xbox+A/B/X/Y profile shortcuts and its Xbox+View mouse
mode.

### Remove

    systemctl --user disable --now stickmouse
    sudo pacman -R stickmouse
    rm -rf ~/.icons/stickmouse-circle ~/.config/stickmouse ~/.local/state/stickmouse

## Support

Report bugs and ask questions at
[felipekrystianek/stickmouse-support](https://github.com/felipekrystianek/stickmouse-support/issues).

## License

Stickmouse is licensed under the GNU General Public License v3.0 or later. See [LICENSE](LICENSE).

Xbox is a trademark of Microsoft Corporation. Stickmouse is not affiliated with or endorsed by
Microsoft.
