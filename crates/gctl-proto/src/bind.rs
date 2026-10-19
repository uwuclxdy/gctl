use std::ops::BitOr;

use crate::error::{Error, Result};
use crate::keys::KEYS;

/// A physical control the mouse lets you rebind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    Left,
    Right,
    Scroll,
    Back,
    Forward,
    ScrollUp,
    ScrollDown,
    Dpi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseFn {
    Left,
    Right,
    Scroll,
    Back,
    Forward,
    ScrollUp,
    ScrollDown,
    ProfileCycleUp,
    ProfileCycleDown,
    BatteryStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyboardFn {
    ProfileCycleUp,
    ProfileCycleDown,
    LayerCycleUp,
    LayerCycleDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DpiFn {
    StageUp,
    StageDown,
    CycleUp,
    CycleDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaFn {
    Player,
    PlayPause,
    Next,
    Previous,
    Stop,
    Mute,
    VolumeUp,
    VolumeDown,
}

/// A keyboard key, identified by its USB HID usage id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    usage: u8,
}

impl Key {
    /// Looks a key up by its `KeyboardEvent.code` name (`KeyA`, `ShiftLeft`, `F5`).
    ///
    /// # Errors
    /// Returns [`Error::UnknownKey`] when `name` is not in the firmware's key set.
    pub fn from_name(name: &str) -> Result<Self> {
        KEYS.iter()
            .find(|key| key.name == name)
            .map(|key| Self { usage: key.usage })
            .ok_or_else(|| Error::UnknownKey(name.to_string()))
    }

    /// The key's USB HID usage id, the value on the wire.
    pub const fn usage(self) -> u8 {
        self.usage
    }

    /// The key's `KeyboardEvent.code` name.
    pub fn name(self) -> &'static str {
        KEYS.iter()
            .find(|key| key.usage == self.usage)
            .map_or("", |key| key.name)
    }

    /// The modifier bit this key sets when it is itself a modifier.
    pub fn modifier(self) -> Option<Modifiers> {
        KEYS.iter()
            .find(|key| key.usage == self.usage)
            .and_then(|key| key.modifier)
    }
}

/// The HID boot-protocol modifier byte, limited to the bits the firmware is known to accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const CTRL_LEFT: Self = Self(0x01);
    pub const SHIFT_LEFT: Self = Self(0x02);
    pub const ALT_LEFT: Self = Self(0x04);
    pub const META_LEFT: Self = Self(0x08);
    pub const META_RIGHT: Self = Self(0x80);

    pub const fn bits(self) -> u8 {
        self.0
    }
}

impl BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Binding {
    Key { key: Key, modifiers: Modifiers },
    Mouse(MouseFn),
    Keyboard(KeyboardFn),
    Media(MediaFn),
    Dpi(DpiFn),
    None,
}
