use gpui::{Keystroke, Modifiers};
use libghostty_vt::key::{self, Key};

pub struct EncodedKey {
    pub key: Key,
    pub mods: key::Mods,
    pub consumed: key::Mods,
    pub utf8: Option<String>,
    pub unshifted: char,
    /// When set, write these bytes to the PTY instead of running the encoder.
    pub raw: Option<Vec<u8>>,
}

pub fn encode_keystroke(keystroke: &Keystroke) -> Option<EncodedKey> {
    if let Some(raw) = macos_line_editing(keystroke) {
        return Some(raw);
    }

    let utf8 = printable_text(keystroke);
    let mut mods = map_mods(&keystroke.modifiers);

    let (key, unshifted, implied_shift) = match map_key(&keystroke.key) {
        Some(mapped) => mapped,
        None if utf8.is_some() => (Key::Unidentified, '\0', false),
        None => return None,
    };

    // On macOS, GPUI folds Shift into the key name for non-letter keys
    // (`shift-1` arrives as key "!" with shift=false). Restore the modifier
    // so libghostty encodes the physical key correctly.
    if implied_shift {
        mods |= key::Mods::SHIFT;
    }

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
        raw: None,
    })
}

/// Ghostty's macOS "natural text editing" bindings: Cmd+arrows jump to the
/// start/end of the line, Option+arrows move by readline words.
fn macos_line_editing(keystroke: &Keystroke) -> Option<EncodedKey> {
    let mods = &keystroke.modifiers;
    if mods.control {
        return None;
    }

    let bytes: &[u8] = match (mods.platform, mods.alt, keystroke.key.as_str()) {
        (true, false, "left") => b"\x01",   // Ctrl-A, beginning of line
        (true, false, "right") => b"\x05",  // Ctrl-E, end of line
        (false, true, "left") => b"\x1bb",  // ESC b, backward-word
        (false, true, "right") => b"\x1bf", // ESC f, forward-word
        _ => return None,
    };

    Some(EncodedKey {
        key: Key::Unidentified,
        mods: key::Mods::empty(),
        consumed: key::Mods::empty(),
        utf8: None,
        unshifted: '\0',
        raw: Some(bytes.to_vec()),
    })
}

fn printable_text(keystroke: &Keystroke) -> Option<String> {
    let text = keystroke.key_char.as_deref().filter(|text| !text.is_empty()).or_else(|| {
        let key = keystroke.key.as_str();
        (key.chars().count() == 1).then_some(key)
    })?;

    if text
        .chars()
        .all(|ch| !ch.is_control() && !matches!(ch as u32, 0xF700..=0xF8FF))
    {
        Some(text.to_string())
    } else {
        None
    }
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

/// Returns (physical key, unshifted codepoint, shift was implied by the key name).
fn map_key(name: &str) -> Option<(Key, char, bool)> {
    let mapped = match name {
        "a" => (Key::A, 'a', false),
        "b" => (Key::B, 'b', false),
        "c" => (Key::C, 'c', false),
        "d" => (Key::D, 'd', false),
        "e" => (Key::E, 'e', false),
        "f" => (Key::F, 'f', false),
        "g" => (Key::G, 'g', false),
        "h" => (Key::H, 'h', false),
        "i" => (Key::I, 'i', false),
        "j" => (Key::J, 'j', false),
        "k" => (Key::K, 'k', false),
        "l" => (Key::L, 'l', false),
        "m" => (Key::M, 'm', false),
        "n" => (Key::N, 'n', false),
        "o" => (Key::O, 'o', false),
        "p" => (Key::P, 'p', false),
        "q" => (Key::Q, 'q', false),
        "r" => (Key::R, 'r', false),
        "s" => (Key::S, 's', false),
        "t" => (Key::T, 't', false),
        "u" => (Key::U, 'u', false),
        "v" => (Key::V, 'v', false),
        "w" => (Key::W, 'w', false),
        "x" => (Key::X, 'x', false),
        "y" => (Key::Y, 'y', false),
        "z" => (Key::Z, 'z', false),
        "0" => (Key::Digit0, '0', false),
        "1" => (Key::Digit1, '1', false),
        "2" => (Key::Digit2, '2', false),
        "3" => (Key::Digit3, '3', false),
        "4" => (Key::Digit4, '4', false),
        "5" => (Key::Digit5, '5', false),
        "6" => (Key::Digit6, '6', false),
        "7" => (Key::Digit7, '7', false),
        "8" => (Key::Digit8, '8', false),
        "9" => (Key::Digit9, '9', false),
        ")" => (Key::Digit0, '0', true),
        "!" => (Key::Digit1, '1', true),
        "@" => (Key::Digit2, '2', true),
        "#" => (Key::Digit3, '3', true),
        "$" => (Key::Digit4, '4', true),
        "%" => (Key::Digit5, '5', true),
        "^" => (Key::Digit6, '6', true),
        "&" => (Key::Digit7, '7', true),
        "*" => (Key::Digit8, '8', true),
        "(" => (Key::Digit9, '9', true),
        "space" => (Key::Space, ' ', false),
        "enter" | "return" => (Key::Enter, '\0', false),
        "tab" => (Key::Tab, '\0', false),
        "backspace" => (Key::Backspace, '\0', false),
        "delete" => (Key::Delete, '\0', false),
        "escape" => (Key::Escape, '\0', false),
        "up" => (Key::ArrowUp, '\0', false),
        "down" => (Key::ArrowDown, '\0', false),
        "left" => (Key::ArrowLeft, '\0', false),
        "right" => (Key::ArrowRight, '\0', false),
        "home" => (Key::Home, '\0', false),
        "end" => (Key::End, '\0', false),
        "pageup" => (Key::PageUp, '\0', false),
        "pagedown" => (Key::PageDown, '\0', false),
        "insert" => (Key::Insert, '\0', false),
        "-" | "minus" => (Key::Minus, '-', false),
        "_" => (Key::Minus, '-', true),
        "=" | "equal" => (Key::Equal, '=', false),
        "+" => (Key::Equal, '=', true),
        "[" => (Key::BracketLeft, '[', false),
        "{" => (Key::BracketLeft, '[', true),
        "]" => (Key::BracketRight, ']', false),
        "}" => (Key::BracketRight, ']', true),
        "\\" => (Key::Backslash, '\\', false),
        "|" => (Key::Backslash, '\\', true),
        ";" => (Key::Semicolon, ';', false),
        ":" => (Key::Semicolon, ';', true),
        "'" | "quote" => (Key::Quote, '\'', false),
        "\"" => (Key::Quote, '\'', true),
        "," | "comma" => (Key::Comma, ',', false),
        "<" => (Key::Comma, ',', true),
        "." | "period" => (Key::Period, '.', false),
        ">" => (Key::Period, '.', true),
        "/" | "slash" => (Key::Slash, '/', false),
        "?" => (Key::Slash, '/', true),
        "`" | "backquote" => (Key::Backquote, '`', false),
        "~" => (Key::Backquote, '`', true),
        "f1" => (Key::F1, '\0', false),
        "f2" => (Key::F2, '\0', false),
        "f3" => (Key::F3, '\0', false),
        "f4" => (Key::F4, '\0', false),
        "f5" => (Key::F5, '\0', false),
        "f6" => (Key::F6, '\0', false),
        "f7" => (Key::F7, '\0', false),
        "f8" => (Key::F8, '\0', false),
        "f9" => (Key::F9, '\0', false),
        "f10" => (Key::F10, '\0', false),
        "f11" => (Key::F11, '\0', false),
        "f12" => (Key::F12, '\0', false),
        _ => return None,
    };
    Some(mapped)
}
