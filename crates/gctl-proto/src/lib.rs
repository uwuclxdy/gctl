//! Wire protocol for Glorious Core v1 wireless mice.
//!
//! Ported from mxw (<https://github.com/dkbednarczyk/mxw>, commit 5ae60fa),
//! Copyright (C) 2023 Damian Bednarczyk, licensed GPL-3.0, and modified for gctl
//! in October 2026. mxw's code from before its commit 24bc0a8 comes from mow
//! (<https://github.com/korkje/mow>), Copyright (c) 2023 Ola Næss Kaldestad,
//! under the MIT license in `LICENSE-MIT-mow`.

mod bind;
mod device;
mod error;
mod keys;
mod model;
mod transport;
mod value;

pub use bind::{Binding, Button, DpiFn, Key, KeyboardFn, MediaFn, Modifiers, MouseFn};
pub use device::{Battery, Device, DeviceState, Firmware};
pub use error::{Error, Result};
pub use model::{Candidate, INTERFACE, Link, MODELS, Model, VENDOR_ID, select};
pub use transport::{REPORT_LEN, Report, Transport, TransportError};
pub use value::{
    Brightness, Debounce, Dpi, DpiConfig, DpiStage, DpiStages, Effect, LiftOff, PollingRate,
    Profile, Rate, Rgb, ScrollDirection, SleepDelay,
};
