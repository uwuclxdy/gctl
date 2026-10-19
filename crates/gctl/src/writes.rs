//! Device writes that also maintain the write-only memory: record on success,
//! clear that setting's memory on failure. Shared by `set` and `import`.

use std::path::Path;

use gctl_proto::{
    Binding, Brightness, Button, Device, DpiStages, Effect, MouseFn, Profile, Rgb, ScrollDirection,
    Transport,
};

use crate::CmdError;
use crate::memory::{self, Memory};
use crate::parse;

pub(crate) fn effect<T: Transport>(
    device: &Device<T>,
    profile: Profile,
    effect: &Effect,
    memory: &mut Memory,
) -> gctl_proto::Result<()> {
    match device.set_effect(profile, effect) {
        Ok(()) => {
            memory.profile(device.model().name, profile).effect = Some(effect.clone());
            Ok(())
        }
        Err(error) => {
            memory.profile(device.model().name, profile).effect = None;
            Err(error)
        }
    }
}

pub(crate) fn brightness<T: Transport>(
    device: &Device<T>,
    brightness: Brightness,
    memory: &mut Memory,
) -> gctl_proto::Result<()> {
    match device.set_brightness(brightness) {
        Ok(()) => {
            memory.model(device.model().name).brightness = Some(brightness);
            Ok(())
        }
        Err(error) => {
            memory.model(device.model().name).brightness = None;
            Err(error)
        }
    }
}

pub(crate) fn dpi_colors<T: Transport>(
    device: &Device<T>,
    profile: Profile,
    colors: &[Rgb],
    memory: &mut Memory,
) -> gctl_proto::Result<()> {
    match device.set_dpi_colors(profile, colors) {
        Ok(()) => {
            memory.profile(device.model().name, profile).dpi_colors = Some(colors.to_vec());
            Ok(())
        }
        Err(error) => {
            memory.profile(device.model().name, profile).dpi_colors = None;
            Err(error)
        }
    }
}

pub(crate) fn bind<T: Transport>(
    device: &Device<T>,
    profile: Profile,
    button: Button,
    binding: Binding,
    memory: &mut Memory,
) -> gctl_proto::Result<()> {
    match device.bind(profile, button, binding) {
        Ok(()) => {
            let name = parse::format_button(button);
            memory
                .profile(device.model().name, profile)
                .binds
                .insert(name.to_owned(), binding);
            Ok(())
        }
        Err(error) => {
            memory
                .profile(device.model().name, profile)
                .binds
                .remove(parse::format_button(button));
            Err(error)
        }
    }
}

pub(crate) fn scroll<T: Transport>(
    device: &Device<T>,
    direction: ScrollDirection,
    memory: &mut Memory,
) -> gctl_proto::Result<()> {
    match device.set_scroll(direction) {
        Ok(()) => {
            let (up, down) = match direction {
                ScrollDirection::Default => (MouseFn::ScrollUp, MouseFn::ScrollDown),
                ScrollDirection::Invert => (MouseFn::ScrollDown, MouseFn::ScrollUp),
            };
            for profile in Profile::all() {
                let mem = memory.profile(device.model().name, profile);
                mem.binds.insert("scroll-up".to_owned(), Binding::Mouse(up));
                mem.binds
                    .insert("scroll-down".to_owned(), Binding::Mouse(down));
            }
            Ok(())
        }
        Err(error) => {
            for profile in Profile::all() {
                let mem = memory.profile(device.model().name, profile);
                mem.binds.remove("scroll-up");
                mem.binds.remove("scroll-down");
            }
            Err(error)
        }
    }
}

/// Writes a profile's DPI stage list; when the stage count changes, that
/// profile's remembered DPI colours no longer line up and are dropped. A failed
/// write drops them too: the stage-list report may have reached the mouse.
pub(crate) fn dpi_stages<T: Transport>(
    device: &Device<T>,
    profile: Profile,
    stages: &DpiStages,
    memory: &mut Memory,
) -> gctl_proto::Result<()> {
    let old_count = device.dpi(profile)?.stages.as_slice().len();
    let result = device.set_dpi_stages(profile, stages);
    if stages.as_slice().len() != old_count {
        memory.profile(device.model().name, profile).dpi_colors = None;
    }
    result
}

/// Runs one write, then persists its memory effect: recorded on success, the
/// setting cleared on failure. A save failure after a failed write is reported
/// on a second line; after a successful write it names the mouse as written.
pub(crate) fn with_memory(
    state_dir: Option<&Path>,
    write: impl FnOnce(&mut Memory) -> gctl_proto::Result<()>,
) -> Result<(), CmdError> {
    let state_dir = memory::require_state_dir(state_dir)?;
    let mut memory = Memory::load(Some(state_dir))?;
    match write(&mut memory) {
        Ok(()) => match memory.save(state_dir) {
            Ok(()) => Ok(()),
            Err(error) => Err(CmdError::Message(memory::save_message(
                state_dir, &error, true,
            ))),
        },
        Err(device_error) => match memory.save(state_dir) {
            Ok(()) => Err(CmdError::Runtime(device_error)),
            Err(error) => Err(CmdError::WriteThenSave {
                device_error,
                save_message: memory::save_message(state_dir, &error, false),
            }),
        },
    }
}
