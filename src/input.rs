use gpui::{Keystroke, Modifiers};
use libghostty_vt::key::{self, Key};

pub struct EncodedKey {
    pub key: Key,
    pub mods: key::Mods,
    pub consumed: key::Mods,
    pub utf8: Option<String>,
    pub unshifted: char,
}

pub fn encode_keystroke(keystroke: &Keystroke) -> Option<EncodedKey> {
    let (key, unshifted) = map_key(&keystroke.key)?;
    let mods = map_mods(&keystroke.modifiers);

    let utf8 = keystroke
        .key_char
        .as_deref()
        .filter(|text| {
            !text.is_empty()
                && text
                    .chars()
                    .all(|ch| !ch.is_control() && !matches!(ch as u32, 0xF700..=0xF8FF))
        })
        .map(str::to_string);

    let mut consumed = key::Mods::empty();
    if utf8.is_some() && mods.contains(key::Mods::SHIFT) {
        consumed |= key::Mods::SHIFT;
    }

    Some(EncodedKey {
        key,
        mods,
        consumed,
        utf8,
        unshifted,
    })
}

fn map_mods(modifiers: &Modifiers) -> key::Mods {
    let mut mods = key::Mods::empty();
    if modifiers.shift {
        mods |= key::Mods::SHIFT;
    }
    if modifiers.alt {
        mods |= key::Mods::ALT;
    }
    if modifiers.control {
        mods |= key::Mods::CTRL;
    }
    if modifiers.platform {
        mods |= key::Mods::SUPER;
    }
    mods
}

fn map_key(name: &str) -> Option<(Key, char)> {
    let mapped = match name {
        "a" => (Key::A, 'a'),
        "b" => (Key::B, 'b'),
        "c" => (Key::C, 'c'),
        "d" => (Key::D, 'd'),
        "e" => (Key::E, 'e'),
        "f" => (Key::F, 'f'),
        "g" => (Key::G, 'g'),
        "h" => (Key::H, 'h'),
        "i" => (Key::I, 'i'),
        "j" => (Key::J, 'j'),
        "k" => (Key::K, 'k'),
        "l" => (Key::L, 'l'),
        "m" => (Key::M, 'm'),
        "n" => (Key::N, 'n'),
        "o" => (Key::O, 'o'),
        "p" => (Key::P, 'p'),
        "q" => (Key::Q, 'q'),
        "r" => (Key::R, 'r'),
        "s" => (Key::S, 's'),
        "t" => (Key::T, 't'),
        "u" => (Key::U, 'u'),
        "v" => (Key::V, 'v'),
        "w" => (Key::W, 'w'),
        "x" => (Key::X, 'x'),
        "y" => (Key::Y, 'y'),
        "z" => (Key::Z, 'z'),
        "0" => (Key::Digit0, '0'),
        "1" => (Key::Digit1, '1'),
        "2" => (Key::Digit2, '2'),
        "3" => (Key::Digit3, '3'),
        "4" => (Key::Digit4, '4'),
        "5" => (Key::Digit5, '5'),
        "6" => (Key::Digit6, '6'),
        "7" => (Key::Digit7, '7'),
        "8" => (Key::Digit8, '8'),
        "9" => (Key::Digit9, '9'),
        "space" => (Key::Space, ' '),
        "enter" | "return" => (Key::Enter, '\0'),
        "tab" => (Key::Tab, '\0'),
        "backspace" => (Key::Backspace, '\0'),
        "delete" => (Key::Delete, '\0'),
        "escape" => (Key::Escape, '\0'),
        "up" => (Key::ArrowUp, '\0'),
        "down" => (Key::ArrowDown, '\0'),
        "left" => (Key::ArrowLeft, '\0'),
        "right" => (Key::ArrowRight, '\0'),
        "home" => (Key::Home, '\0'),
        "end" => (Key::End, '\0'),
        "pageup" => (Key::PageUp, '\0'),
        "pagedown" => (Key::PageDown, '\0'),
        "insert" => (Key::Insert, '\0'),
        "-" | "minus" => (Key::Minus, '-'),
        "=" | "equal" => (Key::Equal, '='),
        "[" => (Key::BracketLeft, '['),
        "]" => (Key::BracketRight, ']'),
        "\\" => (Key::Backslash, '\\'),
        ";" => (Key::Semicolon, ';'),
        "'" | "quote" => (Key::Quote, '\''),
        "," | "comma" => (Key::Comma, ','),
        "." | "period" => (Key::Period, '.'),
        "/" | "slash" => (Key::Slash, '/'),
        "`" | "backquote" => (Key::Backquote, '`'),
        "f1" => (Key::F1, '\0'),
        "f2" => (Key::F2, '\0'),
        "f3" => (Key::F3, '\0'),
        "f4" => (Key::F4, '\0'),
        "f5" => (Key::F5, '\0'),
        "f6" => (Key::F6, '\0'),
        "f7" => (Key::F7, '\0'),
        "f8" => (Key::F8, '\0'),
        "f9" => (Key::F9, '\0'),
        "f10" => (Key::F10, '\0'),
        "f11" => (Key::F11, '\0'),
        "f12" => (Key::F12, '\0'),
        _ => return None,
    };
    Some(mapped)
}
