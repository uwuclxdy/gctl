//! `gctl set`: write one setting, recording write-only values in the memory.

use std::path::Path;

use gctl_proto::{
    Binding, Brightness, Button, Debounce, Device, DpiStage, DpiStages, Effect, LiftOff,
    PollingRate, Profile, Rgb, ScrollDirection, SleepDelay, Transport,
};

use crate::parse;
use crate::writes;
use crate::{CmdError, check_awake, resolve_profile, validate_profile};

pub(crate) enum SetCmd {
    Profile(Profile),
    Dpi(DpiStages),
    DpiStage(DpiStage),
    DpiColors(Vec<Rgb>),
    PollingRate(PollingRate),
    LiftOff(LiftOff),
    Debounce(Debounce),
    Sleep(SleepDelay),
    Effect(Effect),
    Brightness(Brightness),
    Bind(Button, Binding),
    Scroll(ScrollDirection),
}

/// Validates every argument without touching the device.
pub(crate) fn prepare(
    setting: String,
    args: Vec<String>,
    rate: Option<String>,
    profile_flag: Option<u8>,
) -> Result<(SetCmd, Option<Profile>), CmdError> {
    let profile = validate_profile(profile_flag)?;
    let cmd = parse_set(&setting, &args, rate.as_deref())?;
    if rate.is_some() && !matches!(cmd, SetCmd::Effect(_)) {
        return Err(CmdError::Usage("--rate only applies to effect".to_owned()));
    }
    if profile.is_some()
        && let Some(name) = device_wide_name(&cmd)
    {
        return Err(CmdError::Usage(format!(
            "{name} is device-wide; --profile does not apply"
        )));
    }
    Ok((cmd, profile))
}

fn device_wide_name(cmd: &SetCmd) -> Option<&'static str> {
    match cmd {
        SetCmd::Profile(_) => Some("profile"),
        SetCmd::PollingRate(_) => Some("polling-rate"),
        SetCmd::LiftOff(_) => Some("lift-off"),
        SetCmd::Sleep(_) => Some("sleep"),
        SetCmd::Brightness(_) => Some("brightness"),
        SetCmd::Scroll(_) => Some("scroll"),
        SetCmd::Dpi(_)
        | SetCmd::DpiStage(_)
        | SetCmd::DpiColors(_)
        | SetCmd::Debounce(_)
        | SetCmd::Effect(_)
        | SetCmd::Bind(_, _) => None,
    }
}

pub(crate) fn set<T: Transport>(
    device: &Device<T>,
    state_dir: Option<&Path>,
    cmd: SetCmd,
    profile_arg: Option<Profile>,
) -> Result<(), CmdError> {
    check_awake(device)?;

    match cmd {
        SetCmd::Profile(profile) => {
            device.set_profile(profile)?;
            Ok(())
        }
        SetCmd::PollingRate(rate) => {
            device.set_polling_rate(rate)?;
            Ok(())
        }
        SetCmd::LiftOff(lift_off) => {
            device.set_lift_off(lift_off)?;
            Ok(())
        }
        SetCmd::Sleep(delay) => {
            device.set_sleep_delay(delay)?;
            Ok(())
        }
        SetCmd::Debounce(debounce) => {
            let profile = resolve_profile(device, profile_arg)?;
            device.set_debounce(profile, debounce)?;
            Ok(())
        }
        SetCmd::DpiStage(stage) => {
            let profile = resolve_profile(device, profile_arg)?;
            device.set_dpi_stage(profile, stage)?;
            Ok(())
        }
        SetCmd::Dpi(stages) => {
            let profile = resolve_profile(device, profile_arg)?;
            writes::with_memory(state_dir, |memory| {
                writes::dpi_stages(device, profile, &stages, memory)
            })
        }
        SetCmd::DpiColors(colors) => {
            let profile = resolve_profile(device, profile_arg)?;
            writes::with_memory(state_dir, |memory| {
                writes::dpi_colors(device, profile, &colors, memory)
            })
        }
        SetCmd::Effect(effect) => {
            let profile = resolve_profile(device, profile_arg)?;
            writes::with_memory(state_dir, |memory| {
                writes::effect(device, profile, &effect, memory)
            })
        }
        SetCmd::Brightness(brightness) => writes::with_memory(state_dir, |memory| {
            writes::brightness(device, brightness, memory)
        }),
        SetCmd::Bind(button, binding) => {
            let profile = resolve_profile(device, profile_arg)?;
            writes::with_memory(state_dir, |memory| {
                writes::bind(device, profile, button, binding, memory)
            })
        }
        SetCmd::Scroll(direction) => writes::with_memory(state_dir, |memory| {
            writes::scroll(device, direction, memory)
        }),
    }
}

fn parse_set(setting: &str, args: &[String], rate: Option<&str>) -> Result<SetCmd, CmdError> {
    match setting {
        "profile" => {
            let id = one(args, "profile")?;
            Ok(SetCmd::Profile(parse::profile(id)?))
        }
        "dpi" => Ok(SetCmd::Dpi(parse::dpi_stages(
            args.iter().map(String::as_str),
        )?)),
        "dpi-stage" => {
            let stage = one(args, "dpi-stage")?;
            Ok(SetCmd::DpiStage(parse::dpi_stage(stage)?))
        }
        "dpi-colors" => Ok(SetCmd::DpiColors(parse::colors(
            args.iter().map(String::as_str),
        )?)),
        "polling-rate" => {
            let hz = one(args, "polling-rate")?;
            Ok(SetCmd::PollingRate(parse::polling_rate(hz)?))
        }
        "lift-off" => {
            let mm = one(args, "lift-off")?;
            Ok(SetCmd::LiftOff(parse::lift_off(mm)?))
        }
        "debounce" => {
            let ms = one(args, "debounce")?;
            Ok(SetCmd::Debounce(parse::debounce(ms)?))
        }
        "sleep" => {
            let delay = one(args, "sleep")?;
            Ok(SetCmd::Sleep(parse::sleep(delay)?))
        }
        "effect" => Ok(SetCmd::Effect(parse_effect(args, rate)?)),
        "brightness" => {
            let [wired, wireless] = args else {
                return Err(CmdError::Usage(
                    "brightness takes wired and wireless values".to_owned(),
                ));
            };
            Ok(SetCmd::Brightness(parse::brightness_parts(
                wired, wireless,
            )?))
        }
        "bind" => {
            let (button, binding) = parse_bind(args)?;
            Ok(SetCmd::Bind(button, binding))
        }
        "scroll" => {
            let direction = one(args, "scroll")?;
            Ok(SetCmd::Scroll(parse::scroll(direction)?))
        }
        other => Err(CmdError::Usage(format!("unknown setting {other:?}"))),
    }
}

fn one<'a>(args: &'a [String], what: &str) -> Result<&'a str, CmdError> {
    match args {
        [single] => Ok(single),
        _ => Err(CmdError::Usage(format!("{what} takes exactly one value"))),
    }
}

fn parse_effect(args: &[String], rate: Option<&str>) -> Result<Effect, CmdError> {
    let mut text = args.join(" ");
    if let Some(rate) = rate {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str("rate=");
        text.push_str(rate);
    }
    parse::effect(&text).map_err(CmdError::from)
}

fn parse_bind(args: &[String]) -> Result<(Button, Binding), CmdError> {
    match args {
        [button, binding] => Ok((parse::button(button)?, parse::binding(binding)?)),
        _ => Err(CmdError::Usage(
            "bind takes a button and a binding".to_owned(),
        )),
    }
}
