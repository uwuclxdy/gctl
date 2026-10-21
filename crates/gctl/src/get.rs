//! `gctl get`: bare values and the full listing.

use std::io::Write;
use std::path::Path;

use gctl_proto::{Button, Device, Profile, Transport};

use crate::memory::Memory;
use crate::parse;
use crate::{CmdError, check_awake, resolve_profile, validate_profile, write_out};

/// The eight rebindable buttons, in the order the CLI lists them.
pub(crate) const BUTTONS: [Button; 8] = [
    Button::Left,
    Button::Right,
    Button::Scroll,
    Button::Back,
    Button::Forward,
    Button::ScrollUp,
    Button::ScrollDown,
    Button::Dpi,
];

pub(crate) enum GetCmd {
    All,
    Battery,
    Profile,
    Firmware,
    Dpi,
    DpiStage,
    DpiColors,
    PollingRate,
    LiftOff,
    Debounce,
    Sleep,
    Effect,
    Brightness,
    Bind(Button),
    Scroll,
}

/// Validates every argument without touching the device.
pub(crate) fn prepare(
    setting: Option<String>,
    button: Option<String>,
    profile_flag: Option<u8>,
) -> Result<(GetCmd, Option<Profile>), CmdError> {
    let cmd = parse_get(setting, button)?;
    let profile = validate_profile(profile_flag)?;
    if profile.is_some()
        && let Some(name) = device_wide_name(&cmd)
    {
        return Err(CmdError::Usage(format!(
            "{name} is device-wide; --profile does not apply"
        )));
    }
    Ok((cmd, profile))
}

fn device_wide_name(cmd: &GetCmd) -> Option<&'static str> {
    match cmd {
        GetCmd::Profile => Some("profile"),
        GetCmd::Battery => Some("battery"),
        GetCmd::Firmware => Some("firmware"),
        GetCmd::PollingRate => Some("polling-rate"),
        GetCmd::LiftOff => Some("lift-off"),
        GetCmd::Sleep => Some("sleep"),
        GetCmd::Brightness => Some("brightness"),
        GetCmd::Scroll => Some("scroll"),
        GetCmd::All
        | GetCmd::Dpi
        | GetCmd::DpiStage
        | GetCmd::DpiColors
        | GetCmd::Debounce
        | GetCmd::Effect
        | GetCmd::Bind(_) => None,
    }
}

pub(crate) fn get<T: Transport>(
    device: &Device<T>,
    state_dir: Option<&Path>,
    stdout: &mut dyn Write,
    cmd: GetCmd,
    profile_arg: Option<Profile>,
) -> Result<(), CmdError> {
    if !matches!(cmd, GetCmd::Battery) {
        check_awake(device)?;
    }

    let name = device.model().name;
    match cmd {
        GetCmd::All => {
            let memory = Memory::load(state_dir)?;
            let active = device.profile()?;
            let profile = profile_arg.unwrap_or(active);
            get_all(device, &memory, stdout, profile, active)
        }
        GetCmd::Battery => {
            let battery = device.battery()?;
            write_out(stdout, &format!("{}\n", parse::format_battery(battery)))
        }
        GetCmd::Profile => {
            let profile = device.profile()?;
            write_out(stdout, &format!("{}\n", parse::format_profile(profile)))
        }
        GetCmd::Firmware => {
            let firmware = device.firmware()?;
            write_out(stdout, &format!("{}\n", parse::format_firmware(firmware)))
        }
        GetCmd::Dpi => {
            let profile = resolve_profile(device, profile_arg)?;
            let dpi = device.dpi(profile)?;
            write_out(
                stdout,
                &format!("{}\n", parse::format_dpi_stages(&dpi.stages)),
            )
        }
        GetCmd::DpiStage => {
            let profile = resolve_profile(device, profile_arg)?;
            let dpi = device.dpi(profile)?;
            write_out(
                stdout,
                &format!("{}\n", parse::format_dpi_stage(dpi.active)),
            )
        }
        GetCmd::DpiColors => {
            let memory = Memory::load(state_dir)?;
            let profile = resolve_profile(device, profile_arg)?;
            let value = memory.dpi_colors(name, profile).map(parse::format_colors);
            write_out(stdout, &format!("{}\n", write_only(value.as_deref())))
        }
        GetCmd::PollingRate => {
            let rate = device.polling_rate()?;
            write_out(stdout, &format!("{}\n", parse::format_polling_rate(rate)))
        }
        GetCmd::LiftOff => {
            let lift_off = device.lift_off()?;
            write_out(stdout, &format!("{}\n", parse::format_lift_off(lift_off)))
        }
        GetCmd::Debounce => {
            let profile = resolve_profile(device, profile_arg)?;
            let debounce = device.debounce(profile)?;
            write_out(stdout, &format!("{}\n", parse::format_debounce(debounce)))
        }
        GetCmd::Sleep => {
            let sleep = device.sleep_delay()?;
            write_out(stdout, &format!("{}\n", parse::format_sleep(sleep)))
        }
        GetCmd::Effect => {
            let memory = Memory::load(state_dir)?;
            let profile = resolve_profile(device, profile_arg)?;
            let value = memory.effect(name, profile).map(parse::format_effect);
            write_out(stdout, &format!("{}\n", write_only(value.as_deref())))
        }
        GetCmd::Brightness => {
            let memory = Memory::load(state_dir)?;
            let value = memory.brightness(name).map(parse::format_brightness);
            write_out(stdout, &format!("{}\n", write_only(value.as_deref())))
        }
        GetCmd::Bind(button) => {
            let memory = Memory::load(state_dir)?;
            let profile = resolve_profile(device, profile_arg)?;
            let value = memory
                .bind(name, profile, button)
                .map(parse::format_binding);
            write_out(stdout, &format!("{}\n", write_only(value.as_deref())))
        }
        GetCmd::Scroll => {
            let memory = Memory::load(state_dir)?;
            let value = memory
                .scroll(name)
                .map(|direction| parse::format_scroll(direction).to_owned());
            write_out(stdout, &format!("{}\n", write_only(value.as_deref())))
        }
    }
}

fn parse_get(setting: Option<String>, button: Option<String>) -> Result<GetCmd, CmdError> {
    let Some(setting) = setting else {
        if button.is_some() {
            return Err(CmdError::Usage("unexpected argument".to_owned()));
        }
        return Ok(GetCmd::All);
    };
    if setting == "bind" {
        let button = button.ok_or_else(|| CmdError::Usage("bind needs a button".to_owned()))?;
        return Ok(GetCmd::Bind(parse::button(&button)?));
    }
    if button.is_some() {
        return Err(CmdError::Usage("unexpected argument".to_owned()));
    }
    match setting.as_str() {
        "profile" => Ok(GetCmd::Profile),
        "battery" => Ok(GetCmd::Battery),
        "firmware" => Ok(GetCmd::Firmware),
        "dpi" => Ok(GetCmd::Dpi),
        "dpi-stage" => Ok(GetCmd::DpiStage),
        "dpi-colors" => Ok(GetCmd::DpiColors),
        "polling-rate" => Ok(GetCmd::PollingRate),
        "lift-off" => Ok(GetCmd::LiftOff),
        "debounce" => Ok(GetCmd::Debounce),
        "sleep" => Ok(GetCmd::Sleep),
        "effect" => Ok(GetCmd::Effect),
        "brightness" => Ok(GetCmd::Brightness),
        "scroll" => Ok(GetCmd::Scroll),
        other => Err(CmdError::Usage(format!("unknown setting {other:?}"))),
    }
}

fn get_all<T: Transport>(
    device: &Device<T>,
    memory: &Memory,
    stdout: &mut dyn Write,
    profile: Profile,
    active: Profile,
) -> Result<(), CmdError> {
    let battery = device.battery()?;
    let firmware = device.firmware()?;
    let dpi = device.dpi(profile)?;
    let rate = device.polling_rate()?;
    let lift_off = device.lift_off()?;
    let debounce = device.debounce(profile)?;
    let sleep = device.sleep_delay()?;

    let name = device.model().name;
    let mut out = String::new();
    out.push_str(&format!("profile  {}\n", parse::format_profile(active)));
    out.push_str(&format!("battery  {}\n", parse::format_battery(battery)));
    out.push_str(&format!("firmware  {}\n", parse::format_firmware(firmware)));
    out.push_str(&format!("dpi  {}\n", parse::format_dpi_stages(&dpi.stages)));
    out.push_str(&format!(
        "dpi-stage  {}\n",
        parse::format_dpi_stage(dpi.active)
    ));
    out.push_str(&format!(
        "dpi-colors  {}\n",
        write_only(
            memory
                .dpi_colors(name, profile)
                .map(parse::format_colors)
                .as_deref()
        )
    ));
    out.push_str(&format!(
        "polling-rate  {}\n",
        parse::format_polling_rate(rate)
    ));
    out.push_str(&format!("lift-off  {}\n", parse::format_lift_off(lift_off)));
    out.push_str(&format!("debounce  {}\n", parse::format_debounce(debounce)));
    out.push_str(&format!("sleep  {}\n", parse::format_sleep(sleep)));
    out.push_str(&format!(
        "effect  {}\n",
        write_only(
            memory
                .effect(name, profile)
                .map(parse::format_effect)
                .as_deref()
        )
    ));
    out.push_str(&format!(
        "brightness  {}\n",
        write_only(
            memory
                .brightness(name)
                .map(parse::format_brightness)
                .as_deref()
        )
    ));
    for button in BUTTONS {
        let button_name = parse::format_button(button);
        let value = memory
            .bind(name, profile, button)
            .map(parse::format_binding);
        out.push_str(&format!(
            "bind.{button_name}  {}\n",
            write_only(value.as_deref())
        ));
    }
    out.push_str(&format!(
        "scroll  {}\n",
        write_only(
            memory
                .scroll(name)
                .map(|direction| parse::format_scroll(direction).to_owned())
                .as_deref()
        )
    ));
    write_out(stdout, &out)
}

/// A write-only value as the CLI prints it: marked when remembered.
fn write_only(value: Option<&str>) -> String {
    match value {
        Some(value) => format!("{value}  (last set by gctl)"),
        None => "unknown".to_owned(),
    }
}
