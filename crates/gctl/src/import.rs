//! `gctl import`: validate a whole TOML file, confirm device-wide changes,
//! then write device-wide settings and the profile in order.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use gctl_proto::{
    Binding, Brightness, Button, Debounce, Device, DpiStage, DpiStages, Effect, LiftOff,
    PollingRate, Profile, Rgb, ScrollDirection, SleepDelay, Transport,
};
use serde::Deserialize;

use crate::memory::Memory;
use crate::parse;
use crate::writes;
use crate::{CmdError, check_awake, resolve_profile, validate_profile};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportFile {
    format: u8,
    model: String,
    device: Option<ImportDevice>,
    profile: Option<ImportProfile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportDevice {
    polling_rate: Option<u16>,
    lift_off: Option<u8>,
    sleep: Option<String>,
    brightness: Option<ImportBrightness>,
    scroll: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportBrightness {
    wired: u8,
    wireless: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportProfile {
    dpi: Option<Vec<u16>>,
    dpi_stage: Option<u8>,
    dpi_colors: Option<Vec<String>>,
    debounce: Option<u8>,
    effect: Option<String>,
    binds: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Default)]
struct ValidDevice {
    polling_rate: Option<PollingRate>,
    lift_off: Option<LiftOff>,
    sleep: Option<SleepDelay>,
    brightness: Option<Brightness>,
    scroll: Option<ScrollDirection>,
}

#[derive(Debug, Default)]
struct ValidProfile {
    dpi: Option<DpiStages>,
    dpi_stage: Option<DpiStage>,
    dpi_colors: Option<Vec<Rgb>>,
    debounce: Option<Debounce>,
    effect: Option<Effect>,
    binds: Vec<(Button, Binding)>,
}

pub(crate) struct PreparedImport {
    path: PathBuf,
    profile: Option<Profile>,
    yes: bool,
    model: String,
    device_settings: ValidDevice,
    profile_settings: ValidProfile,
    has_device_table: bool,
}

impl PreparedImport {
    /// Prints the model note and asks for confirmation, before the lock is taken.
    pub(crate) fn confirm(
        &self,
        device_name: &str,
        stdin: &mut dyn BufRead,
        stderr: &mut dyn Write,
        interactive: bool,
    ) -> Result<(), CmdError> {
        if self.model != device_name {
            let _ = writeln!(
                stderr,
                "note: {:?} holds settings for {:?}; writing them to {}",
                self.path, self.model, device_name
            );
        }
        if self.has_device_table && !self.yes {
            if !interactive {
                return Err(CmdError::Message(
                    "import changes device-wide settings; pass --yes to confirm".to_owned(),
                ));
            }
            let _ = write!(
                stderr,
                "overwrite device-wide settings (polling rate, lift-off, sleep, brightness, scroll)? [y/N] "
            );
            let _ = stderr.flush();
            let mut answer = String::new();
            stdin.read_line(&mut answer).map_err(CmdError::from)?;
            let answer = answer.trim();
            if !(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")) {
                return Err(CmdError::Message("import cancelled".to_owned()));
            }
        }
        Ok(())
    }
}

/// Reads and validates the whole file before the mouse opens.
pub(crate) fn prepare(
    path: PathBuf,
    profile_flag: Option<u8>,
    yes: bool,
) -> Result<PreparedImport, CmdError> {
    let profile = validate_profile(profile_flag)?;
    let text = std::fs::read_to_string(&path)
        .map_err(|error| CmdError::Message(format!("{path:?}: {error}")))?;
    let file: ImportFile = toml::from_str(&text).map_err(|error| {
        CmdError::Message(format!("{path:?}: {}", crate::toml_error(&text, &error)))
    })?;
    if file.format != 1 {
        return Err(CmdError::Message(format!(
            "{path:?}: unknown format {}",
            file.format
        )));
    }
    let has_device_table = file.device.is_some();
    let device_settings = validate_device(&path, file.device.as_ref())?;
    let profile_settings = validate_profile_settings(&path, file.profile.as_ref())?;
    Ok(PreparedImport {
        path,
        profile,
        yes,
        model: file.model,
        device_settings,
        profile_settings,
        has_device_table,
    })
}

pub(crate) fn execute<T: Transport>(
    device: &Device<T>,
    state_dir: Option<&Path>,
    prepared: PreparedImport,
) -> Result<(), CmdError> {
    check_awake(device)?;
    let profile = resolve_profile(device, prepared.profile)?;
    check_against_device(device, &prepared.path, profile, &prepared.profile_settings)?;
    apply(
        device,
        state_dir,
        profile,
        &prepared.device_settings,
        &prepared.profile_settings,
    )
}

fn validate_device(path: &Path, device: Option<&ImportDevice>) -> Result<ValidDevice, CmdError> {
    let Some(device) = device else {
        return Ok(ValidDevice::default());
    };
    Ok(ValidDevice {
        polling_rate: device
            .polling_rate
            .map(parse::polling_rate_hz)
            .transpose()
            .map_err(|error| field_error(path, "device.polling_rate", error))?,
        lift_off: device
            .lift_off
            .map(parse::lift_off_number)
            .transpose()
            .map_err(|error| field_error(path, "device.lift_off", error))?,
        sleep: device
            .sleep
            .as_deref()
            .map(parse::sleep)
            .transpose()
            .map_err(|error| field_error(path, "device.sleep", error))?,
        brightness: device
            .brightness
            .as_ref()
            .map(|brightness| parse::brightness_numbers(brightness.wired, brightness.wireless)),
        scroll: device
            .scroll
            .as_deref()
            .map(parse::scroll)
            .transpose()
            .map_err(|error| field_error(path, "device.scroll", error))?,
    })
}

fn validate_profile_settings(
    path: &Path,
    profile: Option<&ImportProfile>,
) -> Result<ValidProfile, CmdError> {
    let Some(profile) = profile else {
        return Ok(ValidProfile::default());
    };

    let dpi = profile
        .dpi
        .as_deref()
        .map(parse::dpi_stages_numbers)
        .transpose()
        .map_err(|error| field_error(path, "profile.dpi", error))?;
    let dpi_stage = profile
        .dpi_stage
        .map(parse::dpi_stage_number)
        .transpose()
        .map_err(|error| field_error(path, "profile.dpi_stage", error))?;
    let dpi_colors = profile
        .dpi_colors
        .as_ref()
        .map(|values| parse::colors(values.iter().map(String::as_str)))
        .transpose()
        .map_err(|error| field_error(path, "profile.dpi_colors", error))?;
    let debounce = profile
        .debounce
        .map(parse::debounce_number)
        .transpose()
        .map_err(|error| field_error(path, "profile.debounce", error))?;
    let effect = profile
        .effect
        .as_deref()
        .map(parse::effect)
        .transpose()
        .map_err(|error| field_error(path, "profile.effect", error))?;
    let binds = match profile.binds.as_ref() {
        None => Vec::new(),
        Some(binds) => {
            let mut parsed = Vec::with_capacity(binds.len());
            for (button, binding) in binds {
                let key = format!("profile.binds.{}", parse::escape_for_terminal(button));
                let button =
                    parse::button(button).map_err(|error| field_error(path, &key, error))?;
                let binding =
                    parse::binding(binding).map_err(|error| field_error(path, &key, error))?;
                parsed.push((button, binding));
            }
            parsed
        }
    };

    if let (Some(dpi), Some(stage)) = (&dpi, &dpi_stage)
        && stage.get() as usize > dpi.as_slice().len()
    {
        return Err(CmdError::Message(format!(
            "{path:?}: profile.dpi_stage: DPI stage {} is out of range 1..={}",
            stage.get(),
            dpi.as_slice().len()
        )));
    }
    if let (Some(dpi), Some(colors)) = (&dpi, &dpi_colors)
        && dpi.as_slice().len() != colors.len()
    {
        return Err(CmdError::Message(format!(
            "{path:?}: profile.dpi_colors: got {} colour(s) for {} DPI stage(s)",
            colors.len(),
            dpi.as_slice().len()
        )));
    }

    Ok(ValidProfile {
        dpi,
        dpi_stage,
        dpi_colors,
        debounce,
        effect,
        binds,
    })
}

/// When the file has no `dpi` list, checks `dpi_stage` and `dpi_colors` against
/// the device's stage count, read once before the first write.
fn check_against_device<T: Transport>(
    device: &Device<T>,
    path: &Path,
    profile: Profile,
    settings: &ValidProfile,
) -> Result<(), CmdError> {
    if settings.dpi.is_none() && (settings.dpi_stage.is_some() || settings.dpi_colors.is_some()) {
        let count = device.dpi(profile)?.stages.as_slice().len();
        if let Some(stage) = settings.dpi_stage
            && stage.get() as usize > count
        {
            return Err(CmdError::Message(format!(
                "{path:?}: profile.dpi_stage: DPI stage {} is out of range 1..={count}",
                stage.get()
            )));
        }
        if let Some(colors) = &settings.dpi_colors
            && colors.len() != count
        {
            return Err(CmdError::Message(format!(
                "{path:?}: profile.dpi_colors: got {} colour(s) for {count} DPI stage(s)",
                colors.len()
            )));
        }
    }
    Ok(())
}

/// One device write the import makes, in cli.md's order.
enum WriteOp {
    PollingRate(PollingRate),
    LiftOff(LiftOff),
    Sleep(SleepDelay),
    Brightness(Brightness),
    Scroll(ScrollDirection),
    Dpi(DpiStages),
    DpiStage(DpiStage),
    DpiColors(Vec<Rgb>),
    Debounce(Debounce),
    Effect(Effect),
    Bind(Button, Binding),
}

impl WriteOp {
    /// Whether the write records or clears a remembered value, so it needs a
    /// readable memory loaded before the first write. The match is exhaustive,
    /// so a new variant cannot skip the question.
    fn needs_memory(&self) -> bool {
        match self {
            WriteOp::PollingRate(_)
            | WriteOp::LiftOff(_)
            | WriteOp::Sleep(_)
            | WriteOp::DpiStage(_)
            | WriteOp::Debounce(_) => false,
            WriteOp::Brightness(_)
            | WriteOp::Scroll(_)
            | WriteOp::Dpi(_)
            | WriteOp::DpiColors(_)
            | WriteOp::Effect(_)
            | WriteOp::Bind(_, _) => true,
        }
    }
}

fn apply<T: Transport>(
    device: &Device<T>,
    state_dir: Option<&Path>,
    profile: Profile,
    device_settings: &ValidDevice,
    profile_settings: &ValidProfile,
) -> Result<(), CmdError> {
    let ops = build_writes(device_settings, profile_settings);
    // Before the first write: memory loads iff some operation records one, so a
    // refusal leaves the mouse untouched. The question is answered by the same
    // variants the writes run, so the gate cannot drift from them.
    if ops.iter().any(WriteOp::needs_memory) {
        writes::with_memory(state_dir, |memory| {
            run_writes(device, profile, &ops, memory)
        })
    } else {
        run_writes(device, profile, &ops, &mut Memory::default()).map_err(CmdError::from)
    }
}

fn build_writes(device_settings: &ValidDevice, profile_settings: &ValidProfile) -> Vec<WriteOp> {
    let mut ops = Vec::new();
    if let Some(rate) = device_settings.polling_rate {
        ops.push(WriteOp::PollingRate(rate));
    }
    if let Some(lift_off) = device_settings.lift_off {
        ops.push(WriteOp::LiftOff(lift_off));
    }
    if let Some(sleep) = device_settings.sleep {
        ops.push(WriteOp::Sleep(sleep));
    }
    if let Some(brightness) = device_settings.brightness {
        ops.push(WriteOp::Brightness(brightness));
    }
    if let Some(scroll) = device_settings.scroll {
        ops.push(WriteOp::Scroll(scroll));
    }
    if let Some(dpi) = &profile_settings.dpi {
        ops.push(WriteOp::Dpi(dpi.clone()));
    }
    if let Some(stage) = profile_settings.dpi_stage {
        ops.push(WriteOp::DpiStage(stage));
    }
    if let Some(colors) = &profile_settings.dpi_colors {
        ops.push(WriteOp::DpiColors(colors.clone()));
    }
    if let Some(debounce) = profile_settings.debounce {
        ops.push(WriteOp::Debounce(debounce));
    }
    if let Some(effect) = &profile_settings.effect {
        ops.push(WriteOp::Effect(effect.clone()));
    }
    for (button, binding) in &profile_settings.binds {
        ops.push(WriteOp::Bind(*button, *binding));
    }
    ops
}

fn run_writes<T: Transport>(
    device: &Device<T>,
    profile: Profile,
    ops: &[WriteOp],
    memory: &mut Memory,
) -> gctl_proto::Result<()> {
    for op in ops {
        match op {
            WriteOp::PollingRate(rate) => device.set_polling_rate(*rate)?,
            WriteOp::LiftOff(lift_off) => device.set_lift_off(*lift_off)?,
            WriteOp::Sleep(sleep) => device.set_sleep_delay(*sleep)?,
            WriteOp::Brightness(brightness) => writes::brightness(device, *brightness, memory)?,
            WriteOp::Scroll(direction) => writes::scroll(device, *direction, memory)?,
            WriteOp::Dpi(stages) => writes::dpi_stages(device, profile, stages, memory)?,
            WriteOp::DpiStage(stage) => device.set_dpi_stage(profile, *stage)?,
            WriteOp::DpiColors(colors) => writes::dpi_colors(device, profile, colors, memory)?,
            WriteOp::Debounce(debounce) => device.set_debounce(profile, *debounce)?,
            WriteOp::Effect(effect) => writes::effect(device, profile, effect, memory)?,
            WriteOp::Bind(button, binding) => {
                writes::bind(device, profile, *button, *binding, memory)?
            }
        }
    }
    Ok(())
}

fn field_error(path: &Path, key: &str, error: parse::ParseError) -> CmdError {
    CmdError::Message(format!("{path:?}: {key}: {}", error.0))
}
