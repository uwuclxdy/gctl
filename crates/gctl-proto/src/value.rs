use std::fmt;
use std::str::FromStr;

use crate::error::{Error, Result};

/// Onboard profile, `1..=3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Profile(u8);

impl Profile {
    pub const MAX: u8 = 3;

    /// Creates a profile id.
    ///
    /// # Errors
    /// Returns [`Error::OutOfRange`] when `id` is not in `1..=3`.
    pub fn new(id: u8) -> Result<Self> {
        if (1..=Self::MAX).contains(&id) {
            Ok(Self(id))
        } else {
            Err(Error::OutOfRange {
                what: "profile",
                value: id as i64,
                min: 1,
                max: Self::MAX as i64,
            })
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// Every profile, in id order.
    pub fn all() -> [Self; 3] {
        [Self(1), Self(2), Self(3)]
    }
}

/// Sensor resolution, `100..=19000`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dpi(u16);

impl Dpi {
    pub const MIN: u16 = 100;
    pub const MAX: u16 = 19000;

    /// Creates a DPI value.
    ///
    /// # Errors
    /// Returns [`Error::OutOfRange`] when `dpi` is outside `100..=19000`.
    pub fn new(dpi: u16) -> Result<Self> {
        if (Self::MIN..=Self::MAX).contains(&dpi) {
            Ok(Self(dpi))
        } else {
            Err(Error::OutOfRange {
                what: "DPI",
                value: dpi as i64,
                min: Self::MIN as i64,
                max: Self::MAX as i64,
            })
        }
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// The DPI stage list of one profile, `1..=6` stages.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DpiStages(Vec<Dpi>);

impl DpiStages {
    pub const MAX: usize = 6;

    /// Creates a stage list.
    ///
    /// # Errors
    /// Returns [`Error::OutOfRange`] when `stages` is empty or longer than 6.
    pub fn new(stages: Vec<Dpi>) -> Result<Self> {
        if stages.is_empty() || stages.len() > Self::MAX {
            return Err(Error::OutOfRange {
                what: "DPI stage count",
                value: stages.len() as i64,
                min: 1,
                max: Self::MAX as i64,
            });
        }
        Ok(Self(stages))
    }

    pub fn as_slice(&self) -> &[Dpi] {
        &self.0
    }
}

/// A 1-based index into a profile's DPI stage list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DpiStage(u8);

impl DpiStage {
    /// Creates a stage index.
    ///
    /// # Errors
    /// Returns [`Error::OutOfRange`] when `stage` is not in `1..=6`.
    pub fn new(stage: u8) -> Result<Self> {
        if (1..=DpiStages::MAX as u8).contains(&stage) {
            Ok(Self(stage))
        } else {
            Err(Error::OutOfRange {
                what: "DPI stage",
                value: stage as i64,
                min: 1,
                max: DpiStages::MAX as i64,
            })
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DpiConfig {
    pub stages: DpiStages,
    pub active: DpiStage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PollingRate {
    Hz1000,
    Hz500,
    Hz250,
    Hz125,
}

impl PollingRate {
    /// Report interval in milliseconds, the value on the wire.
    pub const fn ms(self) -> u8 {
        match self {
            Self::Hz1000 => 1,
            Self::Hz500 => 2,
            Self::Hz250 => 4,
            Self::Hz125 => 8,
        }
    }

    pub const fn hz(self) -> u16 {
        1000 / self.ms() as u16
    }

    /// Decodes a report interval in milliseconds.
    ///
    /// # Errors
    /// Returns [`Error::Implausible`] for any `ms` other than 1, 2, 4 or 8.
    pub fn from_ms(ms: u8) -> Result<Self> {
        match ms {
            1 => Ok(Self::Hz1000),
            2 => Ok(Self::Hz500),
            4 => Ok(Self::Hz250),
            8 => Ok(Self::Hz125),
            other => Err(Error::Implausible {
                what: "polling rate",
                value: other as u16,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LiftOff {
    Mm1,
    Mm2,
}

/// Switch debounce time in milliseconds, `0..=16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Debounce(u8);

impl Debounce {
    pub const MAX: u8 = 16;

    /// Creates a debounce time.
    ///
    /// # Errors
    /// Returns [`Error::OutOfRange`] when `ms` is above 16.
    pub fn new(ms: u8) -> Result<Self> {
        if ms <= Self::MAX {
            Ok(Self(ms))
        } else {
            Err(Error::OutOfRange {
                what: "debounce",
                value: ms as i64,
                min: 0,
                max: Self::MAX as i64,
            })
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SleepDelay {
    Never,
    /// `1..=0xFFFE` seconds; `0xFFFF` on the wire means never.
    Seconds(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Accepts six hex digits, `ff8800`.
impl FromStr for Rgb {
    type Err = Error;

    /// # Errors
    /// Returns [`Error::InvalidColour`] when `s` is not exactly six ASCII hex digits.
    fn from_str(s: &str) -> Result<Self> {
        let hex = s.as_bytes();
        if hex.len() != 6 || !hex.iter().all(u8::is_ascii_hexdigit) {
            return Err(Error::InvalidColour(s.to_string()));
        }
        let parsed = u32::from_str_radix(s, 16).map_err(|_| Error::InvalidColour(s.to_string()))?;
        let [_, r, g, b] = parsed.to_be_bytes();
        Ok(Self { r, g, b })
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// Lighting animation speed, `0..=100`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rate(u8);

impl Rate {
    pub const MAX: u8 = 100;
    pub const DEFAULT: Self = Self(40);

    /// Creates a lighting animation speed.
    ///
    /// # Errors
    /// Returns [`Error::OutOfRange`] when `rate` is above 100.
    pub fn new(rate: u8) -> Result<Self> {
        if rate <= Self::MAX {
            Ok(Self(rate))
        } else {
            Err(Error::OutOfRange {
                what: "rate",
                value: rate as i64,
                min: 0,
                max: Self::MAX as i64,
            })
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Effect {
    Off,
    Glorious {
        rate: Rate,
    },
    Cycle {
        rate: Rate,
    },
    /// `2..=6` colours.
    Pulse {
        rate: Rate,
        colors: Vec<Rgb>,
    },
    Solid {
        color: Rgb,
    },
    PulseOne {
        rate: Rate,
        color: Rgb,
    },
    Tail {
        rate: Rate,
    },
    /// `1..=2` colours.
    Rave {
        rate: Rate,
        colors: Vec<Rgb>,
    },
    Wave {
        rate: Rate,
    },
}

impl Effect {
    /// Checks the colour count against the wire bounds.
    ///
    /// # Errors
    /// Returns [`Error::OutOfRange`] when a pulse carries other than `2..=6`
    /// colours, or a rave other than `1..=2`.
    pub fn check(&self) -> Result<()> {
        let (count, min, max) = match self {
            Self::Pulse { colors, .. } => (colors.len(), 2, 6),
            Self::Rave { colors, .. } => (colors.len(), 1, 2),
            Self::Off
            | Self::Glorious { .. }
            | Self::Cycle { .. }
            | Self::Solid { .. }
            | Self::PulseOne { .. }
            | Self::Tail { .. }
            | Self::Wave { .. } => return Ok(()),
        };
        let count = count as i64;
        if !(min..=max).contains(&count) {
            return Err(Error::OutOfRange {
                what: "colour count",
                value: count,
                min,
                max,
            });
        }
        Ok(())
    }
}

/// LED brightness per link, `0..=255` each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Brightness {
    pub wired: u8,
    pub wireless: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollDirection {
    Default,
    Invert,
}
