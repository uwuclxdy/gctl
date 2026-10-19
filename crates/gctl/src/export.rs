//! `gctl export`: one profile plus the device-wide settings, as TOML.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use gctl_proto::{Device, Transport};
use serde::Serialize;

use crate::memory::Memory;
use crate::parse;
use crate::{CmdError, check_awake, resolve_profile, validate_profile};

#[derive(Serialize)]
struct ExportFile {
    format: u8,
    model: String,
    device: ExportDevice,
    profile: ExportProfile,
}

#[derive(Serialize)]
struct ExportDevice {
    polling_rate: u16,
    lift_off: u8,
    sleep: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    brightness: Option<ExportBrightness>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scroll: Option<String>,
}

#[derive(Serialize)]
struct ExportBrightness {
    wired: u8,
    wireless: u8,
}

#[derive(Serialize)]
struct ExportProfile {
    dpi: Vec<u16>,
    dpi_stage: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    dpi_colors: Option<Vec<String>>,
    debounce: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    effect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    binds: Option<BTreeMap<String, String>>,
}

/// Refuses an existing path (a dangling symlink included) before the mouse opens.
pub(crate) fn prepare(
    path: PathBuf,
    profile_flag: Option<u8>,
) -> Result<(PathBuf, Option<gctl_proto::Profile>), CmdError> {
    let profile = validate_profile(profile_flag)?;
    if path.symlink_metadata().is_ok() {
        return Err(CmdError::Message(format!("{path:?} exists")));
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        match parent.metadata() {
            Ok(metadata) if !metadata.is_dir() => {
                return Err(CmdError::Message(format!(
                    "could not write {path:?}: {parent:?} is not a directory"
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(CmdError::Message(format!(
                    "could not write {path:?}: {parent:?} does not exist"
                )));
            }
            Err(error) => {
                return Err(CmdError::Message(format!(
                    "could not write {path:?}: {parent:?}: {error}"
                )));
            }
        }
    }
    Ok((path, profile))
}

pub(crate) fn export<T: Transport>(
    device: &Device<T>,
    state_dir: Option<&Path>,
    profile_arg: Option<gctl_proto::Profile>,
    path: &Path,
) -> Result<(), CmdError> {
    check_awake(device)?;
    let profile = resolve_profile(device, profile_arg)?;

    let polling_rate = device.polling_rate()?;
    let lift_off = device.lift_off()?;
    let sleep = device.sleep_delay()?;
    let dpi = device.dpi(profile)?;
    let debounce = device.debounce(profile)?;

    let name = device.model().name;
    let memory = Memory::load(state_dir)?;
    let brightness = memory.brightness(name);
    let scroll = memory.scroll(name);
    let dpi_colors = memory.dpi_colors(name, profile);
    let effect = memory.effect(name, profile);
    let binds = memory.binds(name, profile);

    let file = ExportFile {
        format: 1,
        model: name.to_owned(),
        device: ExportDevice {
            polling_rate: parse::polling_rate_value(polling_rate),
            lift_off: parse::lift_off_value(lift_off),
            sleep: parse::format_sleep(sleep),
            brightness: brightness.map(|brightness| {
                let (wired, wireless) = parse::brightness_values(brightness);
                ExportBrightness { wired, wireless }
            }),
            scroll: scroll.map(|direction| parse::format_scroll(direction).to_owned()),
        },
        profile: ExportProfile {
            dpi: parse::dpi_stages_values(&dpi.stages),
            dpi_stage: parse::dpi_stage_value(dpi.active),
            dpi_colors: dpi_colors.map(|colors| {
                colors
                    .iter()
                    .map(|color| parse::format_color(*color))
                    .collect()
            }),
            debounce: parse::debounce_value(debounce),
            effect: effect.map(parse::format_effect),
            binds: binds.and_then(|binds| {
                if binds.is_empty() {
                    None
                } else {
                    Some(
                        binds
                            .iter()
                            .map(|(button, binding)| {
                                (button.clone(), parse::format_binding(*binding))
                            })
                            .collect(),
                    )
                }
            }),
        },
    };
    let text = toml::to_string(&file).map_err(|error| CmdError::Message(error.to_string()))?;
    write_export(path, &text)
}

/// Writes through a temp file created with `create_new` (never following a
/// symlink planted at its name), then hard-links it to the path. Where the
/// filesystem refuses hard links, the path is created exclusively instead.
fn write_export(path: &Path, text: &str) -> Result<(), CmdError> {
    let mut fill = fill_new;
    write_export_with(path, text, |from, to| fs::hard_link(from, to), &mut fill)
}

/// Writes `text` into a file this run created and flushes it to disk.
fn fill_new(file: &mut fs::File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()
}

/// The export write with its two filesystem steps injected: `link` stands for
/// `fs::hard_link`, `fill` writes the bytes into every file this run creates
/// (the temp file, and the target on the no-hard-link fallback) and syncs it.
/// On any failure no file this run created is left behind, and the error reads
/// `could not write "<path>": <cause>`.
pub(crate) fn write_export_with(
    path: &Path,
    text: &str,
    link: fn(&Path, &Path) -> io::Result<()>,
    fill: &mut dyn FnMut(&mut fs::File, &[u8]) -> io::Result<()>,
) -> Result<(), CmdError> {
    path.file_name()
        .ok_or_else(|| CmdError::Message("invalid export path".to_owned()))?;
    let tmp = path.with_file_name(format!(".gctl-export.{}.tmp", std::process::id()));

    let mut temp = match fs::File::options().write(true).create_new(true).open(&tmp) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return Err(CmdError::Message(format!(
                "could not write {path:?}: cannot create temp file {tmp:?}: {error}"
            )));
        }
        Err(error) => return Err(write_error(path, error)),
    };
    if let Err(error) = fill(&mut temp, text.as_bytes()) {
        let _ = fs::remove_file(&tmp);
        return Err(write_error(path, error));
    }
    drop(temp);

    match link(&tmp, path) {
        Ok(()) => {
            let _ = fs::remove_file(&tmp);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&tmp);
            Err(CmdError::Message(format!("{path:?} exists")))
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
            ) =>
        {
            let _ = fs::remove_file(&tmp);
            let mut file = match fs::File::options().write(true).create_new(true).open(path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    return Err(CmdError::Message(format!("{path:?} exists")));
                }
                Err(error) => return Err(write_error(path, error)),
            };
            if let Err(error) = fill(&mut file, text.as_bytes()) {
                let _ = fs::remove_file(path);
                return Err(write_error(path, error));
            }
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&tmp);
            Err(write_error(path, error))
        }
    }
}

/// The export I/O error: the path, then the cause, on one line.
fn write_error(path: &Path, error: io::Error) -> CmdError {
    CmdError::Message(format!("could not write {path:?}: {error}"))
}

#[cfg(test)]
#[path = "../tests/unit/export.rs"]
mod tests;
