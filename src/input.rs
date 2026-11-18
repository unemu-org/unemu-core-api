// SPDX-License-Identifier: AGPL-3.0-or-later
//! Input (core-contract.md §3.7).
//!
//! The front end maps its toolkit's key codes to [`Key`] and decides what is
//! a host shortcut and what goes to the guest. Pointer capture, its release
//! chord and the swallowed first click stay in the front end. Everything
//! after [`crate::Machine::input`] is the core's: encoding, repeat, device
//! back-pressure, coalescing and overflow.

/// One input event for the guest.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// A key of a keyboard port, by physical position.
    Key {
        port: PortId,
        key: Key,
        state: KeyState,
    },
    /// Relative pointer movement in device counts, positive `dy` up.
    PointerMotion { port: PortId, dx: i32, dy: i32 },
    /// A pointer button, numbered from 0: left, right, middle.
    PointerButton {
        port: PortId,
        button: u8,
        pressed: bool,
    },
    /// A control of a game pad.
    Pad {
        port: PortId,
        control: PadControl,
        pressed: bool,
    },
    /// The front end lost focus: every held key, button and control of
    /// every port is released.
    ReleaseAll,
}

/// A key by its position: the usage id of the USB HID keyboard page (USB
/// HID Usage Tables, Keyboard/Keypad Page 0x07), such as `0x04` for the key
/// marked A on a US keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(pub u16);

/// Names of the keyboard page's usages (USB HID Usage Tables 1.3, §10,
/// Keyboard/Keypad Page 0x07), by the legend of a US keyboard.
impl Key {
    pub const A: Key = Key(0x04);
    pub const B: Key = Key(0x05);
    pub const C: Key = Key(0x06);
    pub const D: Key = Key(0x07);
    pub const E: Key = Key(0x08);
    pub const F: Key = Key(0x09);
    pub const G: Key = Key(0x0A);
    pub const H: Key = Key(0x0B);
    pub const I: Key = Key(0x0C);
    pub const J: Key = Key(0x0D);
    pub const K: Key = Key(0x0E);
    pub const L: Key = Key(0x0F);
    pub const M: Key = Key(0x10);
    pub const N: Key = Key(0x11);
    pub const O: Key = Key(0x12);
    pub const P: Key = Key(0x13);
    pub const Q: Key = Key(0x14);
    pub const R: Key = Key(0x15);
    pub const S: Key = Key(0x16);
    pub const T: Key = Key(0x17);
    pub const U: Key = Key(0x18);
    pub const V: Key = Key(0x19);
    pub const W: Key = Key(0x1A);
    pub const X: Key = Key(0x1B);
    pub const Y: Key = Key(0x1C);
    pub const Z: Key = Key(0x1D);
    pub const DIGIT_1: Key = Key(0x1E);
    pub const DIGIT_2: Key = Key(0x1F);
    pub const DIGIT_3: Key = Key(0x20);
    pub const DIGIT_4: Key = Key(0x21);
    pub const DIGIT_5: Key = Key(0x22);
    pub const DIGIT_6: Key = Key(0x23);
    pub const DIGIT_7: Key = Key(0x24);
    pub const DIGIT_8: Key = Key(0x25);
    pub const DIGIT_9: Key = Key(0x26);
    pub const DIGIT_0: Key = Key(0x27);
    pub const ENTER: Key = Key(0x28);
    pub const ESCAPE: Key = Key(0x29);
    pub const BACKSPACE: Key = Key(0x2A);
    pub const TAB: Key = Key(0x2B);
    pub const SPACE: Key = Key(0x2C);
    pub const MINUS: Key = Key(0x2D);
    pub const EQUAL: Key = Key(0x2E);
    pub const BRACKET_LEFT: Key = Key(0x2F);
    pub const BRACKET_RIGHT: Key = Key(0x30);
    pub const BACKSLASH: Key = Key(0x31);
    /// Non-US `#` and `~`, beside Enter on ISO keyboards.
    pub const NON_US_HASH: Key = Key(0x32);
    pub const SEMICOLON: Key = Key(0x33);
    /// Apostrophe and quotation mark.
    pub const QUOTE: Key = Key(0x34);
    /// Grave accent and tilde.
    pub const BACKQUOTE: Key = Key(0x35);
    pub const COMMA: Key = Key(0x36);
    pub const PERIOD: Key = Key(0x37);
    pub const SLASH: Key = Key(0x38);
    pub const CAPS_LOCK: Key = Key(0x39);
    pub const F1: Key = Key(0x3A);
    pub const F2: Key = Key(0x3B);
    pub const F3: Key = Key(0x3C);
    pub const F4: Key = Key(0x3D);
    pub const F5: Key = Key(0x3E);
    pub const F6: Key = Key(0x3F);
    pub const F7: Key = Key(0x40);
    pub const F8: Key = Key(0x41);
    pub const F9: Key = Key(0x42);
    pub const F10: Key = Key(0x43);
    pub const F11: Key = Key(0x44);
    pub const F12: Key = Key(0x45);
    pub const PRINT_SCREEN: Key = Key(0x46);
    pub const SCROLL_LOCK: Key = Key(0x47);
    pub const PAUSE: Key = Key(0x48);
    pub const INSERT: Key = Key(0x49);
    pub const HOME: Key = Key(0x4A);
    pub const PAGE_UP: Key = Key(0x4B);
    pub const DELETE: Key = Key(0x4C);
    pub const END: Key = Key(0x4D);
    pub const PAGE_DOWN: Key = Key(0x4E);
    pub const ARROW_RIGHT: Key = Key(0x4F);
    pub const ARROW_LEFT: Key = Key(0x50);
    pub const ARROW_DOWN: Key = Key(0x51);
    pub const ARROW_UP: Key = Key(0x52);
    pub const NUM_LOCK: Key = Key(0x53);
    pub const NUMPAD_DIVIDE: Key = Key(0x54);
    pub const NUMPAD_MULTIPLY: Key = Key(0x55);
    pub const NUMPAD_SUBTRACT: Key = Key(0x56);
    pub const NUMPAD_ADD: Key = Key(0x57);
    pub const NUMPAD_ENTER: Key = Key(0x58);
    pub const NUMPAD_1: Key = Key(0x59);
    pub const NUMPAD_2: Key = Key(0x5A);
    pub const NUMPAD_3: Key = Key(0x5B);
    pub const NUMPAD_4: Key = Key(0x5C);
    pub const NUMPAD_5: Key = Key(0x5D);
    pub const NUMPAD_6: Key = Key(0x5E);
    pub const NUMPAD_7: Key = Key(0x5F);
    pub const NUMPAD_8: Key = Key(0x60);
    pub const NUMPAD_9: Key = Key(0x61);
    pub const NUMPAD_0: Key = Key(0x62);
    pub const NUMPAD_DECIMAL: Key = Key(0x63);
    /// Non-US `\` and `|`, beside the left Shift on ISO keyboards.
    pub const NON_US_BACKSLASH: Key = Key(0x64);
    /// The context-menu key.
    pub const APPLICATION: Key = Key(0x65);
    pub const POWER: Key = Key(0x66);
    pub const NUMPAD_EQUAL: Key = Key(0x67);
    pub const F13: Key = Key(0x68);
    pub const F14: Key = Key(0x69);
    pub const F15: Key = Key(0x6A);
    pub const F16: Key = Key(0x6B);
    pub const F17: Key = Key(0x6C);
    pub const F18: Key = Key(0x6D);
    pub const F19: Key = Key(0x6E);
    pub const F20: Key = Key(0x6F);
    pub const F21: Key = Key(0x70);
    pub const F22: Key = Key(0x71);
    pub const F23: Key = Key(0x72);
    pub const F24: Key = Key(0x73);
    pub const HELP: Key = Key(0x75);
    pub const MENU: Key = Key(0x76);
    pub const SELECT: Key = Key(0x77);
    pub const STOP: Key = Key(0x78);
    pub const AGAIN: Key = Key(0x79);
    pub const UNDO: Key = Key(0x7A);
    pub const CUT: Key = Key(0x7B);
    pub const COPY: Key = Key(0x7C);
    pub const PASTE: Key = Key(0x7D);
    pub const FIND: Key = Key(0x7E);
    pub const MUTE: Key = Key(0x7F);
    pub const VOLUME_UP: Key = Key(0x80);
    pub const VOLUME_DOWN: Key = Key(0x81);
    pub const NUMPAD_COMMA: Key = Key(0x85);
    /// Ro on Japanese keyboards.
    pub const INTERNATIONAL_1: Key = Key(0x87);
    /// Katakana/Hiragana on Japanese keyboards.
    pub const INTERNATIONAL_2: Key = Key(0x88);
    /// Yen on Japanese keyboards.
    pub const INTERNATIONAL_3: Key = Key(0x89);
    /// Henkan (convert) on Japanese keyboards.
    pub const INTERNATIONAL_4: Key = Key(0x8A);
    /// Muhenkan (no convert) on Japanese keyboards.
    pub const INTERNATIONAL_5: Key = Key(0x8B);
    /// Hangul/English toggle; Kana on some Japanese keyboards.
    pub const LANG_1: Key = Key(0x90);
    /// Hanja conversion; Eisu on some Japanese keyboards.
    pub const LANG_2: Key = Key(0x91);
    pub const LANG_3: Key = Key(0x92);
    pub const LANG_4: Key = Key(0x93);
    pub const LANG_5: Key = Key(0x94);
    pub const CONTROL_LEFT: Key = Key(0xE0);
    pub const SHIFT_LEFT: Key = Key(0xE1);
    pub const ALT_LEFT: Key = Key(0xE2);
    /// The left Windows, Command or Super key.
    pub const GUI_LEFT: Key = Key(0xE3);
    pub const CONTROL_RIGHT: Key = Key(0xE4);
    pub const SHIFT_RIGHT: Key = Key(0xE5);
    pub const ALT_RIGHT: Key = Key(0xE6);
    /// The right Windows, Command or Super key.
    pub const GUI_RIGHT: Key = Key(0xE7);
}

/// What happened to a key.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    /// The host repeated a held key. A core with its own repeat ignores it.
    Repeated,
    Released,
}

/// A control of a game pad.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadControl {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    L,
    R,
    Start,
    Select,
}

/// One input port of a machine.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputPortDescription {
    pub id: PortId,
    pub kind: InputKind,
    /// The port's name for people, such as `Keyboard` or `Controller 1`.
    pub label: String,
}

impl InputPortDescription {
    pub fn new(id: PortId, kind: InputKind, label: impl Into<String>) -> Self {
        Self {
            id,
            kind,
            label: label.into(),
        }
    }
}

/// What an input port takes.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputKind {
    Keyboard,
    /// A mouse or similar; the front end offers pointer capture when a
    /// machine has one.
    RelativePointer {
        buttons: u8,
    },
    Pad {
        controls: Vec<PadControl>,
    },
}

/// An input port of a machine, by the number its description gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PortId(pub u8);
