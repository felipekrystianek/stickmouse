//! Mac virtual key codes (Carbon's Events.h, ANSI layout) and modifier flags.

use objc2_core_graphics::CGEventFlags;

use crate::bindings::Key;

/// Letters a to z in alphabetical order.
const LETTERS: [u16; 26] = [
    0x00, 0x0B, 0x08, 0x02, 0x0E, 0x03, 0x05, 0x04, 0x22, 0x26, 0x28, 0x25, 0x2E, 0x2D, 0x1F, 0x23, 0x0C, 0x0F, 0x01,
    0x11, 0x20, 0x09, 0x0D, 0x07, 0x10, 0x06,
];
/// Digits 0 to 9.
const DIGITS: [u16; 10] = [0x1D, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1A, 0x1C, 0x19];
/// F1 to F12.
const FUNCTION_KEYS: [u16; 12] = [0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F];
const NAMED: [(Key, u16); 18] = [
    (Key::Up, 0x7E),
    (Key::Down, 0x7D),
    (Key::Left, 0x7B),
    (Key::Right, 0x7C),
    (Key::Esc, 0x35),
    (Key::Tab, 0x30),
    (Key::Enter, 0x24),
    (Key::Space, 0x31),
    (Key::Backspace, 0x33),
    (Key::Delete, 0x75),
    (Key::Home, 0x73),
    (Key::End, 0x77),
    (Key::PageUp, 0x74),
    (Key::PageDown, 0x79),
    (Key::Ctrl, 0x3B),
    (Key::Shift, 0x38),
    (Key::Alt, 0x3A),
    (Key::Meta, 0x37),
];

pub const SPACE: u16 = 0x31;
pub const TAB: u16 = 0x30;

/// The key code for `key`, or `None` for media keys, which are sent differently.
pub fn code(key: Key) -> Option<u16> {
    match key {
        Key::Letter(letter) => LETTERS.get(usize::from(letter.checked_sub(b'a')?)).copied(),
        Key::Digit(digit) => DIGITS.get(usize::from(digit)).copied(),
        Key::F(number) => FUNCTION_KEYS.get(usize::from(number.checked_sub(1)?)).copied(),
        key => NAMED.iter().find(|(named, _)| *named == key).map(|(_, code)| *code),
    }
}

/// The key for a code from a real key press, for recording shortcuts.
pub fn key(code: u16) -> Option<Key> {
    if let Some(index) = LETTERS.iter().position(|c| *c == code) {
        return Some(Key::Letter(b'a' + index as u8));
    }
    if let Some(index) = DIGITS.iter().position(|c| *c == code) {
        return Some(Key::Digit(index as u8));
    }
    if let Some(index) = FUNCTION_KEYS.iter().position(|c| *c == code) {
        return Some(Key::F(index as u8 + 1));
    }
    // The right-hand modifiers count as the same modifier.
    match code {
        0x36 => return Some(Key::Meta),
        0x3C => return Some(Key::Shift),
        0x3D => return Some(Key::Alt),
        0x3E => return Some(Key::Ctrl),
        _ => {}
    }
    NAMED.iter().find(|(_, c)| *c == code).map(|(key, _)| *key)
}

/// "Not coalesced", which every real key event carries (IOKit's IOLLEvent.h).
pub const NON_COALESCED: u64 = 0x100;

/// The flag a held modifier sets, with the device bit of its left-hand key.
pub fn modifier_flag(key: Key) -> u64 {
    match key {
        Key::Ctrl => CGEventFlags::MaskControl.0 | 0x01,
        Key::Shift => CGEventFlags::MaskShift.0 | 0x02,
        Key::Alt => CGEventFlags::MaskAlternate.0 | 0x20,
        Key::Meta => CGEventFlags::MaskCommand.0 | 0x08,
        _ => 0,
    }
}

/// Keys a Mac keyboard sends with the Fn flag, and arrows also with the keypad flag.
pub fn extra_flags(key: Key) -> u64 {
    match key {
        Key::Up | Key::Down | Key::Left | Key::Right => {
            CGEventFlags::MaskSecondaryFn.0 | CGEventFlags::MaskNumericPad.0
        }
        Key::Home | Key::End | Key::PageUp | Key::PageDown | Key::Delete | Key::F(_) => CGEventFlags::MaskSecondaryFn.0,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_but_media_has_a_code_that_maps_back() {
        for key in Key::all() {
            match code(key) {
                Some(found) => assert_eq!(self::key(found), Some(key), "{key:?}"),
                None => assert!(matches!(
                    key,
                    Key::PlayPause | Key::NextTrack | Key::PreviousTrack | Key::VolumeUp | Key::VolumeDown | Key::Mute
                )),
            }
        }
    }

    #[test]
    fn codes_are_unique() {
        let mut codes: Vec<u16> = Key::all().into_iter().filter_map(code).collect();
        let count = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), count);
    }
}
