use crate::bind::Modifiers;

/// One keyboard key the firmware accepts, keyed by its USB HID usage id.
pub(crate) struct KeyDef {
    pub(crate) usage: u8,
    pub(crate) name: &'static str,
    pub(crate) modifier: Option<Modifiers>,
}

/// The exact key set mxw knows, named by real USB HID usage id.
pub(crate) const KEYS: [KeyDef; 90] = [
    KeyDef {
        usage: 4,
        name: "KeyA",
        modifier: None,
    },
    KeyDef {
        usage: 5,
        name: "KeyB",
        modifier: None,
    },
    KeyDef {
        usage: 6,
        name: "KeyC",
        modifier: None,
    },
    KeyDef {
        usage: 7,
        name: "KeyD",
        modifier: None,
    },
    KeyDef {
        usage: 8,
        name: "KeyE",
        modifier: None,
    },
    KeyDef {
        usage: 9,
        name: "KeyF",
        modifier: None,
    },
    KeyDef {
        usage: 10,
        name: "KeyG",
        modifier: None,
    },
    KeyDef {
        usage: 11,
        name: "KeyH",
        modifier: None,
    },
    KeyDef {
        usage: 12,
        name: "KeyI",
        modifier: None,
    },
    KeyDef {
        usage: 13,
        name: "KeyJ",
        modifier: None,
    },
    KeyDef {
        usage: 14,
        name: "KeyK",
        modifier: None,
    },
    KeyDef {
        usage: 15,
        name: "KeyL",
        modifier: None,
    },
    KeyDef {
        usage: 16,
        name: "KeyM",
        modifier: None,
    },
    KeyDef {
        usage: 17,
        name: "KeyN",
        modifier: None,
    },
    KeyDef {
        usage: 18,
        name: "KeyO",
        modifier: None,
    },
    KeyDef {
        usage: 19,
        name: "KeyP",
        modifier: None,
    },
    KeyDef {
        usage: 20,
        name: "KeyQ",
        modifier: None,
    },
    KeyDef {
        usage: 21,
        name: "KeyR",
        modifier: None,
    },
    KeyDef {
        usage: 22,
        name: "KeyS",
        modifier: None,
    },
    KeyDef {
        usage: 23,
        name: "KeyT",
        modifier: None,
    },
    KeyDef {
        usage: 24,
        name: "KeyU",
        modifier: None,
    },
    KeyDef {
        usage: 25,
        name: "KeyV",
        modifier: None,
    },
    KeyDef {
        usage: 26,
        name: "KeyW",
        modifier: None,
    },
    KeyDef {
        usage: 27,
        name: "KeyX",
        modifier: None,
    },
    KeyDef {
        usage: 28,
        name: "KeyY",
        modifier: None,
    },
    KeyDef {
        usage: 29,
        name: "KeyZ",
        modifier: None,
    },
    KeyDef {
        usage: 30,
        name: "Digit1",
        modifier: None,
    },
    KeyDef {
        usage: 31,
        name: "Digit2",
        modifier: None,
    },
    KeyDef {
        usage: 32,
        name: "Digit3",
        modifier: None,
    },
    KeyDef {
        usage: 33,
        name: "Digit4",
        modifier: None,
    },
    KeyDef {
        usage: 34,
        name: "Digit5",
        modifier: None,
    },
    KeyDef {
        usage: 35,
        name: "Digit6",
        modifier: None,
    },
    KeyDef {
        usage: 36,
        name: "Digit7",
        modifier: None,
    },
    KeyDef {
        usage: 37,
        name: "Digit8",
        modifier: None,
    },
    KeyDef {
        usage: 38,
        name: "Digit9",
        modifier: None,
    },
    KeyDef {
        usage: 39,
        name: "Digit0",
        modifier: None,
    },
    KeyDef {
        usage: 40,
        name: "Enter",
        modifier: None,
    },
    KeyDef {
        usage: 41,
        name: "Escape",
        modifier: None,
    },
    KeyDef {
        usage: 42,
        name: "Backspace",
        modifier: None,
    },
    KeyDef {
        usage: 43,
        name: "Tab",
        modifier: None,
    },
    KeyDef {
        usage: 44,
        name: "Space",
        modifier: None,
    },
    KeyDef {
        usage: 45,
        name: "Minus",
        modifier: None,
    },
    KeyDef {
        usage: 46,
        name: "Equal",
        modifier: None,
    },
    KeyDef {
        usage: 47,
        name: "BracketLeft",
        modifier: None,
    },
    KeyDef {
        usage: 48,
        name: "BracketRight",
        modifier: None,
    },
    KeyDef {
        usage: 49,
        name: "Backslash",
        modifier: None,
    },
    KeyDef {
        usage: 51,
        name: "Semicolon",
        modifier: None,
    },
    KeyDef {
        usage: 52,
        name: "Quote",
        modifier: None,
    },
    KeyDef {
        usage: 53,
        name: "Backquote",
        modifier: None,
    },
    KeyDef {
        usage: 54,
        name: "Comma",
        modifier: None,
    },
    KeyDef {
        usage: 55,
        name: "Period",
        modifier: None,
    },
    KeyDef {
        usage: 56,
        name: "Slash",
        modifier: None,
    },
    KeyDef {
        usage: 57,
        name: "CapsLock",
        modifier: None,
    },
    KeyDef {
        usage: 58,
        name: "F1",
        modifier: None,
    },
    KeyDef {
        usage: 59,
        name: "F2",
        modifier: None,
    },
    KeyDef {
        usage: 60,
        name: "F3",
        modifier: None,
    },
    KeyDef {
        usage: 61,
        name: "F4",
        modifier: None,
    },
    KeyDef {
        usage: 62,
        name: "F5",
        modifier: None,
    },
    KeyDef {
        usage: 63,
        name: "F6",
        modifier: None,
    },
    KeyDef {
        usage: 64,
        name: "F7",
        modifier: None,
    },
    KeyDef {
        usage: 65,
        name: "F8",
        modifier: None,
    },
    KeyDef {
        usage: 66,
        name: "F9",
        modifier: None,
    },
    KeyDef {
        usage: 67,
        name: "F10",
        modifier: None,
    },
    KeyDef {
        usage: 68,
        name: "F11",
        modifier: None,
    },
    KeyDef {
        usage: 69,
        name: "F12",
        modifier: None,
    },
    KeyDef {
        usage: 70,
        name: "PrintScreen",
        modifier: None,
    },
    KeyDef {
        usage: 71,
        name: "ScrollLock",
        modifier: None,
    },
    KeyDef {
        usage: 72,
        name: "Pause",
        modifier: None,
    },
    KeyDef {
        usage: 73,
        name: "Insert",
        modifier: None,
    },
    KeyDef {
        usage: 74,
        name: "Home",
        modifier: None,
    },
    KeyDef {
        usage: 75,
        name: "PageUp",
        modifier: None,
    },
    KeyDef {
        usage: 76,
        name: "Delete",
        modifier: None,
    },
    KeyDef {
        usage: 77,
        name: "End",
        modifier: None,
    },
    KeyDef {
        usage: 78,
        name: "PageDown",
        modifier: None,
    },
    KeyDef {
        usage: 79,
        name: "ArrowRight",
        modifier: None,
    },
    KeyDef {
        usage: 80,
        name: "ArrowLeft",
        modifier: None,
    },
    KeyDef {
        usage: 81,
        name: "ArrowDown",
        modifier: None,
    },
    KeyDef {
        usage: 82,
        name: "ArrowUp",
        modifier: None,
    },
    KeyDef {
        usage: 83,
        name: "NumLock",
        modifier: None,
    },
    KeyDef {
        usage: 84,
        name: "NumpadDivide",
        modifier: None,
    },
    KeyDef {
        usage: 85,
        name: "NumpadMultiply",
        modifier: None,
    },
    KeyDef {
        usage: 86,
        name: "NumpadSubtract",
        modifier: None,
    },
    KeyDef {
        usage: 87,
        name: "NumpadAdd",
        modifier: None,
    },
    KeyDef {
        usage: 99,
        name: "NumpadDecimal",
        modifier: None,
    },
    KeyDef {
        usage: 101,
        name: "ContextMenu",
        modifier: None,
    },
    KeyDef {
        usage: 224,
        name: "ControlLeft",
        modifier: Some(Modifiers::CTRL_LEFT),
    },
    KeyDef {
        usage: 225,
        name: "ShiftLeft",
        modifier: Some(Modifiers::SHIFT_LEFT),
    },
    KeyDef {
        usage: 226,
        name: "AltLeft",
        modifier: Some(Modifiers::ALT_LEFT),
    },
    KeyDef {
        usage: 227,
        name: "MetaLeft",
        modifier: Some(Modifiers::META_LEFT),
    },
    KeyDef {
        usage: 231,
        name: "MetaRight",
        modifier: Some(Modifiers::META_RIGHT),
    },
];
