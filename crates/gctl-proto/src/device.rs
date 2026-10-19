use std::fmt;
use std::time::Duration;

use crate::bind::{Binding, Button, DpiFn, KeyboardFn, MediaFn, MouseFn};
use crate::error::{Error, Result};
use crate::model::Model;
#[cfg(feature = "hidapi")]
use crate::model::{Candidate, select};
use crate::transport::{REPORT_LEN, Report, Transport};
use crate::value::{
    Brightness, Debounce, Dpi, DpiConfig, DpiStage, DpiStages, Effect, LiftOff, PollingRate,
    Profile, Rgb, ScrollDirection, SleepDelay,
};

/// Power state from the status report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceState {
    Awake,
    Asleep,
    WakingUp,
    Unknown(u8),
    /// The reply was not a status report.
    Invalid,
}

impl DeviceState {
    /// State from the `resp[1]` field of a `0x83` status report.
    fn from_report(resp: &Report) -> Self {
        if resp[6] != 0x83 {
            return Self::Invalid;
        }
        match resp[1] {
            0xA1 => Self::Awake,
            0xA4 => Self::Asleep,
            0xA0 => Self::WakingUp,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Battery {
    pub state: DeviceState,
    /// `None` for the raw 0 the firmware reports right after power-on;
    /// `Some(1..=100)`, with anything above 100 clamped to 100.
    pub percent: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Firmware(pub [u8; 4]);

impl fmt::Display for Firmware {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d] = self.0;
        write!(f, "{a}.{b}.{c}.{d}")
    }
}

/// One mouse behind a [`Transport`].
///
/// Setters write without checking [`DeviceState`]; callers check
/// [`Device::status`] first, since an asleep mouse drops writes.
#[derive(Debug)]
pub struct Device<T> {
    transport: T,
    model: &'static Model,
    io_delay: Duration,
}

/// Number of times to poll for a read reply before giving up.
const MAX_READ_ATTEMPTS: usize = 5;

/// Number of times to wait for the device to echo back a new bind.
const MAX_BIND_ATTEMPTS: usize = 3;

impl<T: Transport> Device<T> {
    /// The firmware needs this pause between a request and reading its reply.
    pub const IO_DELAY: Duration = Duration::from_millis(50);

    pub const fn new(transport: T, model: &'static Model) -> Self {
        Self {
            transport,
            model,
            io_delay: Self::IO_DELAY,
        }
    }

    #[must_use]
    pub const fn with_io_delay(mut self, io_delay: Duration) -> Self {
        self.io_delay = io_delay;
        self
    }

    pub const fn model(&self) -> &'static Model {
        self.model
    }

    /// Reads the power state from a `0x83` status report.
    pub fn status(&self) -> Result<DeviceState> {
        Ok(DeviceState::from_report(&self.status_report()?))
    }

    /// Reads the battery percentage and power state from a `0x83` status report.
    pub fn battery(&self) -> Result<Battery> {
        let resp = self.status_report()?;
        Ok(Battery {
            state: DeviceState::from_report(&resp),
            percent: match resp[8] {
                0 => None,
                1..=100 => Some(resp[8]),
                _ => Some(100),
            },
        })
    }

    /// Reads the firmware version.
    pub fn firmware(&self) -> Result<Firmware> {
        let resp = self.read(0x03, 0x81, 0x00, 0x00)?;
        Ok(Firmware([resp[7], resp[8], resp[9], resp[10]]))
    }

    /// Reads the active profile.
    ///
    /// # Errors
    /// Returns [`Error::Implausible`] when the device reports a profile outside `1..=3`.
    pub fn profile(&self) -> Result<Profile> {
        let resp = self.read(0x01, 0x85, 0x00, 0x00)?;
        let id = resp[7];
        Profile::new(id).map_err(|_| Error::Implausible {
            what: "profile",
            value: u16::from(id),
        })
    }

    /// Writes the active profile.
    pub fn set_profile(&self, profile: Profile) -> Result<()> {
        let mut report = self.request();
        report[4] = 0x01;
        report[6] = 0x05;
        report[7] = profile.get();
        self.send(&report)
    }

    /// Reads the DPI stage list and active stage of one profile.
    ///
    /// # Errors
    /// Returns [`Error::Implausible`] when the device reports an implausible
    /// stage count, active stage, or stage value.
    pub fn dpi(&self, profile: Profile) -> Result<DpiConfig> {
        let active = self.read(0x02, 0x82, 0x01, profile.get())?[8];

        let stages_resp = self.read(26, 0x81, 0x01, profile.get())?;
        let count = check_stage_count(stages_resp[8])?;
        if !(1..=count).contains(&active) {
            return Err(Error::Implausible {
                what: "active DPI stage",
                value: active as u16,
            });
        }

        let mut stages = Vec::with_capacity(count as usize);
        for stage in 1..=count {
            let offset = 9 + 4 * (stage as usize - 1);
            let value = u16::from_be_bytes([stages_resp[offset], stages_resp[offset + 1]]);
            stages.push(Dpi::new(value).map_err(|_| Error::Implausible { what: "DPI", value })?);
        }

        Ok(DpiConfig {
            stages: DpiStages::new(stages)?,
            active: DpiStage::new(active)?,
        })
    }

    /// Writes a profile's DPI stage list, resetting the active stage to 1 first.
    pub fn set_dpi_stages(&self, profile: Profile, stages: &DpiStages) -> Result<()> {
        self.write_dpi_stage(profile, 1)?;
        self.sleep();

        let stages = stages.as_slice();
        let mut report = self.request();
        report[4] = (2 + stages.len() * 4) as u8;
        report[5] = 0x01;
        report[6] = 0x01;
        report[7] = profile.get();
        report[8] = stages.len() as u8;
        for (i, stage) in stages.iter().enumerate() {
            let [first, second] = stage.get().to_be_bytes();
            let offset = 9 + 4 * i;
            report[offset] = first;
            report[offset + 1] = second;
            report[offset + 2] = first;
            report[offset + 3] = second;
        }
        self.send(&report)
    }

    /// Writes the active DPI stage, after checking it against the device's stage count.
    ///
    /// # Errors
    /// Returns [`Error::OutOfRange`] when `stage` exceeds the profile's stage count.
    pub fn set_dpi_stage(&self, profile: Profile, stage: DpiStage) -> Result<()> {
        let count = self.dpi_stage_count(profile)?;
        let id = stage.get();
        if id > count {
            return Err(Error::OutOfRange {
                what: "DPI stage",
                value: id as i64,
                min: 1,
                max: count as i64,
            });
        }
        self.write_dpi_stage(profile, id)
    }

    /// Writes one colour per configured DPI stage of a profile.
    ///
    /// # Errors
    /// Returns [`Error::StageCountMismatch`] when `colors` does not match the stage count.
    pub fn set_dpi_colors(&self, profile: Profile, colors: &[Rgb]) -> Result<()> {
        let count = self.dpi_stage_count(profile)?;
        if colors.len() != count as usize {
            return Err(Error::StageCountMismatch {
                profile: profile.get(),
                stages: count,
                given: colors.len(),
            });
        }

        let mut report = self.request();
        report[4] = (1 + colors.len() * 3) as u8;
        report[5] = 0x02;
        report[6] = 0x01;
        report[7] = profile.get();
        for (i, color) in colors.iter().enumerate() {
            let offset = 8 + 3 * i;
            report[offset] = color.r;
            report[offset + 1] = color.g;
            report[offset + 2] = color.b;
        }
        self.send(&report)
    }

    /// Reads the polling rate.
    ///
    /// # Errors
    /// Returns [`Error::Implausible`] when the device reports an unknown interval.
    pub fn polling_rate(&self) -> Result<PollingRate> {
        let resp = self.read(0x01, 0x80, 0x01, 0x00)?;
        PollingRate::from_ms(resp[7])
    }

    /// Writes the polling rate.
    pub fn set_polling_rate(&self, rate: PollingRate) -> Result<()> {
        let mut report = self.request();
        report[4] = 0x01;
        report[5] = 0x01;
        report[7] = rate.ms();
        self.send(&report)
    }

    /// Reads the lift-off distance.
    ///
    /// # Errors
    /// Returns [`Error::Implausible`] when the device reports an unknown value.
    pub fn lift_off(&self) -> Result<LiftOff> {
        let resp = self.read(0x01, 0x87, 0x01, 0x00)?;
        match resp[7] {
            0 => Ok(LiftOff::Mm1),
            1 => Ok(LiftOff::Mm2),
            other => Err(Error::Implausible {
                what: "lift-off distance",
                value: other as u16,
            }),
        }
    }

    /// Writes the lift-off distance.
    pub fn set_lift_off(&self, lift_off: LiftOff) -> Result<()> {
        let mut report = self.request();
        report[4] = 0x01;
        report[5] = 0x01;
        report[6] = 0x07;
        report[7] = match lift_off {
            LiftOff::Mm1 => 0,
            LiftOff::Mm2 => 1,
        };
        self.send(&report)
    }

    /// Reads the debounce time of one profile.
    ///
    /// # Errors
    /// Returns [`Error::Implausible`] when the device reports a time above 16 ms.
    pub fn debounce(&self, profile: Profile) -> Result<Debounce> {
        let resp = self.read(0x02, 0x88, 0x00, profile.get())?;
        let ms = resp[8];
        if ms > Debounce::MAX {
            return Err(Error::Implausible {
                what: "debounce",
                value: ms as u16,
            });
        }
        Debounce::new(ms)
    }

    /// Writes the debounce time of one profile.
    pub fn set_debounce(&self, profile: Profile, debounce: Debounce) -> Result<()> {
        let mut report = self.request();
        report[4] = 0x01;
        report[6] = 0x08;
        report[7] = profile.get();
        report[8] = debounce.get();
        self.send(&report)
    }

    /// Reads the sleep delay.
    ///
    /// # Errors
    /// Returns [`Error::Implausible`] when the device reports a raw `0` delay.
    pub fn sleep_delay(&self) -> Result<SleepDelay> {
        let resp = self.read(0x02, 0x87, 0x00, 0x00)?;
        let total = u16::from_be_bytes([resp[7], resp[8]]);
        if total == 0 {
            return Err(Error::Implausible {
                what: "sleep delay",
                value: 0,
            });
        }
        Ok(if total == 0xFFFF {
            SleepDelay::Never
        } else {
            SleepDelay::Seconds(total)
        })
    }

    /// Writes the sleep delay; `0xFFFF` (never) comes only from [`SleepDelay::Never`].
    ///
    /// # Errors
    /// Returns [`Error::OutOfRange`] when `Seconds(n)` has `n == 0` or
    /// `n == 0xFFFF`, and sends nothing.
    pub fn set_sleep_delay(&self, delay: SleepDelay) -> Result<()> {
        let total = match delay {
            SleepDelay::Never => 0xFFFF,
            SleepDelay::Seconds(seconds) => {
                if seconds == 0 || seconds == 0xFFFF {
                    return Err(Error::OutOfRange {
                        what: "sleep delay",
                        value: seconds as i64,
                        min: 1,
                        max: 0xFFFE,
                    });
                }
                seconds
            }
        };
        let [first, second] = total.to_be_bytes();
        let mut report = self.request();
        report[4] = 0x02;
        report[6] = 0x07;
        report[7] = first;
        report[8] = second;
        self.send(&report)
    }

    /// Writes the LED brightness for the wired and wireless links.
    pub fn set_brightness(&self, brightness: Brightness) -> Result<()> {
        let mut report = self.request();
        report[4] = 0x02;
        report[5] = 0x02;
        report[6] = 0x02;
        report[7] = 0x01;
        report[8] = brightness.wired;
        self.send(&report)?;
        self.sleep();

        report[7] = 0x00;
        report[8] = brightness.wireless;
        self.send(&report)
    }

    /// Writes a lighting effect for one profile.
    ///
    /// # Errors
    /// Returns [`Error::OutOfRange`] when `effect` carries too few or too many colours.
    pub fn set_effect(&self, profile: Profile, effect: &Effect) -> Result<()> {
        effect.check()?;
        let mut report = self.request();
        report[5] = 0x02;
        report[7] = profile.get();
        report[8] = 0xFF;

        match effect {
            Effect::Off => {
                report[4] = 0x05;
                report[9] = 0x00;
            }
            Effect::Glorious { rate } => {
                report[4] = 0x05;
                report[9] = 0x01;
                report[11] = rate_default(rate.get());
            }
            Effect::Cycle { rate } => {
                report[4] = 0x05;
                report[9] = 0x02;
                report[11] = rate_default(rate.get());
                report[12] = 0xFF;
            }
            Effect::Pulse { rate, colors } => {
                report[4] = (colors.len() * 3 + 5) as u8;
                report[9] = 0x03;
                report[11] = rate_default(rate.get());
                write_colors(&mut report, colors, 6);
            }
            Effect::Solid { color } => {
                report[4] = 0x08;
                report[9] = 0x04;
                write_colors(&mut report, &[*color], 1);
            }
            Effect::PulseOne { rate, color } => {
                report[4] = 0x08;
                report[9] = 0x05;
                report[11] = rate_default(rate.get());
                write_colors(&mut report, &[*color], 1);
            }
            Effect::Tail { rate } => {
                report[4] = 0x05;
                report[9] = 0x06;
                report[11] = rate_default(rate.get());
            }
            Effect::Rave { rate, colors } => {
                report[4] = (colors.len() * 3 + 5) as u8;
                report[9] = 0x07;
                report[11] = rate_rave_wave(rate.get());
                write_colors(&mut report, colors, 2);
            }
            Effect::Wave { rate } => {
                report[4] = 0x05;
                report[9] = 0x08;
                report[11] = rate_rave_wave(rate.get());
            }
        }

        self.send(&report)
    }

    /// Re-binds one physical button of one profile.
    ///
    /// # Errors
    /// Returns [`Error::BindNotAcknowledged`] when the device never echoes the bind back.
    pub fn bind(&self, profile: Profile, button: Button, binding: Binding) -> Result<()> {
        let mut report = self.request();
        report[4] = 0x09;
        report[5] = 0x03;
        report[7] = profile.get();
        report[8] = button_id(button);

        match binding {
            Binding::Key { key, modifiers } => {
                report[10] = 0x04;
                report[11] = 0x02;
                report[12] = modifiers.bits();
                match key.modifier() {
                    Some(modifier) => report[12] |= modifier.bits(),
                    None => report[13] = key.usage(),
                }
            }
            Binding::Mouse(mouse_fn) => {
                let (a, b, c) = mouse_bytes(mouse_fn);
                report[10] = a;
                report[11] = b;
                report[12] = c;
            }
            Binding::Keyboard(keyboard_fn) => {
                report[10] = 0x05;
                report[11] = 0x02;
                report[12] = keyboard_id(keyboard_fn);
                report[13] = 0x0F;
            }
            Binding::Media(media_fn) => {
                let (b1, b2) = media_bytes(media_fn);
                report[10] = 0x05;
                report[11] = 0x02;
                report[12] = b1;
                report[13] = b2;
            }
            Binding::Dpi(dpi_fn) => {
                report[10] = 0x07;
                report[11] = 0x01;
                report[12] = dpi_id(dpi_fn);
            }
            Binding::None => {}
        }

        self.send(&report)?;
        self.set_and_check(&report)
    }

    /// Rebinds scroll up and down on every profile.
    pub fn set_scroll(&self, direction: ScrollDirection) -> Result<()> {
        let (up, down) = match direction {
            ScrollDirection::Default => (MouseFn::ScrollUp, MouseFn::ScrollDown),
            ScrollDirection::Invert => (MouseFn::ScrollDown, MouseFn::ScrollUp),
        };
        for profile in Profile::all() {
            self.bind(profile, Button::ScrollUp, Binding::Mouse(up))?;
            self.bind(profile, Button::ScrollDown, Binding::Mouse(down))?;
        }
        Ok(())
    }

    /// Sends a read request, then polls for the reply whose command byte matches.
    fn read(&self, length: u8, command: u8, sub: u8, arg: u8) -> Result<Report> {
        let mut request = self.request();
        request[4] = length;
        request[5] = sub;
        request[6] = command;
        request[7] = arg;
        self.send(&request)?;

        let mut resp = [0u8; REPORT_LEN];
        for _ in 0..MAX_READ_ATTEMPTS {
            self.sleep();
            self.receive(&mut resp)?;
            if resp[6] == command {
                return Ok(resp);
            }
        }
        Err(Error::NoResponse { command })
    }

    /// Sends one status request and reads its single reply, without retrying.
    fn status_report(&self) -> Result<Report> {
        let mut request = self.request();
        request[4] = 0x02;
        request[6] = 0x83;
        self.send(&request)?;
        self.sleep();
        let mut resp = [0u8; REPORT_LEN];
        self.receive(&mut resp)?;
        Ok(resp)
    }

    /// Reads a profile's DPI stage count.
    fn dpi_stage_count(&self, profile: Profile) -> Result<u8> {
        let resp = self.read(26, 0x81, 0x01, profile.get())?;
        check_stage_count(resp[8])
    }

    /// Writes the active-stage packet without re-checking the stage count.
    fn write_dpi_stage(&self, profile: Profile, id: u8) -> Result<()> {
        let mut report = self.request();
        report[4] = 0x02;
        report[5] = 0x01;
        report[6] = 0x02;
        report[7] = profile.get();
        report[8] = id;
        self.send(&report)
    }

    /// Waits for the device to echo a bind back, re-sending on request.
    fn set_and_check(&self, report: &Report) -> Result<()> {
        let mut waiting = false;
        for _ in 0..MAX_BIND_ATTEMPTS {
            self.sleep();
            if waiting {
                continue;
            }
            let mut read = [0u8; REPORT_LEN];
            self.receive(&mut read)?;
            self.sleep();
            match read[0] {
                0xA2 => self.send(report)?,
                0xA0 => {}
                0xA4 => waiting = true,
                _ => return Ok(()),
            }
        }
        Err(Error::BindNotAcknowledged {
            attempts: MAX_BIND_ATTEMPTS,
        })
    }

    fn request(&self) -> Report {
        let mut report = [0u8; REPORT_LEN];
        report[3] = 0x02;
        report
    }

    fn send(&self, report: &Report) -> Result<()> {
        self.transport.send(report).map_err(Error::Transport)
    }

    fn receive(&self, report: &mut Report) -> Result<()> {
        self.transport.receive(report).map_err(Error::Transport)
    }

    fn sleep(&self) {
        std::thread::sleep(self.io_delay);
    }
}

#[cfg(feature = "hidapi")]
impl Device<hidapi::HidDevice> {
    /// Opens the best known mouse behind `api`, or `Ok(None)` when none is present.
    ///
    /// Reads `api`'s cached device list; a caller re-running this to pick up a
    /// replug calls `HidApi::refresh_devices` first.
    ///
    /// # Errors
    /// Returns [`Error::Transport`] when the chosen device cannot be opened.
    pub fn open(api: &hidapi::HidApi) -> Result<Option<Self>> {
        let infos: Vec<&hidapi::DeviceInfo> = api.device_list().collect();
        let candidates: Vec<Candidate> = infos
            .iter()
            .map(|info| Candidate {
                vendor_id: info.vendor_id(),
                product_id: info.product_id(),
                interface: info.interface_number(),
            })
            .collect();

        let Some(candidate) = select(&candidates) else {
            return Ok(None);
        };

        let Some(info) = infos.iter().find(|info| {
            info.vendor_id() == candidate.vendor_id
                && info.product_id() == candidate.product_id
                && info.interface_number() == candidate.interface
        }) else {
            return Ok(None);
        };

        let device = info
            .open_device(api)
            .map_err(|error| Error::Transport(Box::new(error)))?;

        let Some(model) = Model::from_pid(candidate.product_id) else {
            return Ok(None);
        };

        Ok(Some(Self::new(device, model)))
    }
}

/// A stage count the firmware can plausibly hold.
fn check_stage_count(count: u8) -> Result<u8> {
    if (1..=DpiStages::MAX as u8).contains(&count) {
        Ok(count)
    } else {
        Err(Error::Implausible {
            what: "DPI stage count",
            value: count as u16,
        })
    }
}

const fn button_id(button: Button) -> u8 {
    match button {
        Button::Left => 1,
        Button::Right => 2,
        Button::Scroll => 3,
        Button::Back => 4,
        Button::Forward => 5,
        Button::ScrollUp => 16,
        Button::ScrollDown => 17,
        Button::Dpi => 20,
    }
}

const fn mouse_bytes(mouse_fn: MouseFn) -> (u8, u8, u8) {
    let id = match mouse_fn {
        MouseFn::Left => 1,
        MouseFn::Right => 2,
        MouseFn::Scroll => 3,
        MouseFn::Back => 4,
        MouseFn::Forward => 5,
        MouseFn::ScrollUp => 16,
        MouseFn::ScrollDown => 17,
        MouseFn::BatteryStatus => 12,
        MouseFn::ProfileCycleUp => 24,
        MouseFn::ProfileCycleDown => 25,
    };
    match id {
        12 => (12, 1, 1),
        24 => (8, 1, 4),
        25 => (8, 1, 3),
        other => (1, 1, other),
    }
}

const fn keyboard_id(keyboard_fn: KeyboardFn) -> u8 {
    match keyboard_fn {
        KeyboardFn::ProfileCycleUp => 1,
        KeyboardFn::ProfileCycleDown => 2,
        KeyboardFn::LayerCycleUp => 3,
        KeyboardFn::LayerCycleDown => 4,
    }
}

const fn media_bytes(media_fn: MediaFn) -> (u8, u8) {
    match media_fn {
        MediaFn::Player => (1, 131),
        MediaFn::PlayPause => (0, 205),
        MediaFn::Next => (0, 181),
        MediaFn::Previous => (0, 182),
        MediaFn::Stop => (0, 183),
        MediaFn::Mute => (0, 226),
        MediaFn::VolumeUp => (0, 233),
        MediaFn::VolumeDown => (0, 234),
    }
}

const fn dpi_id(dpi_fn: DpiFn) -> u8 {
    match dpi_fn {
        DpiFn::StageUp => 1,
        DpiFn::StageDown => 2,
        DpiFn::CycleUp => 6,
        DpiFn::CycleDown => 7,
    }
}

/// The wire rate for every animated effect except Rave and Wave: `(105 - rate) / 5`.
const fn rate_default(rate: u8) -> u8 {
    (105 - rate) / 5
}

/// The wire rate for the Rave and Wave effects: `(105 - rate) * 2`.
const fn rate_rave_wave(rate: u8) -> u8 {
    (105 - rate) * 2
}

/// Writes colours into 3-byte slots from byte 12, padding with black.
fn write_colors(report: &mut Report, colors: &[Rgb], slots: usize) {
    for (i, color) in colors
        .iter()
        .copied()
        .chain(std::iter::repeat(Rgb::default()))
        .take(slots)
        .enumerate()
    {
        let offset = 12 + 3 * i;
        report[offset] = color.r;
        report[offset + 1] = color.g;
        report[offset + 2] = color.b;
    }
}
