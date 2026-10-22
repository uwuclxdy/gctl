//! The GUI's device transactions, generic over `T: Transport`. Each runs on
//! whatever thread the caller chooses, under one `with_lock` transaction.

use std::io::{self, Write};
use std::path::Path;

use gctl_proto::{Device, DeviceState, Profile, Transport};

use crate::gui::state::{ImportCheck, LoadError, Snapshot, WriteFailure, fill_write_only};
use crate::memory::Memory;
use crate::writes::WriteOp;
use crate::{CmdError, NO_LOCK_LOCATION, check_awake};

/// Reads everything the window shows, in one transaction: the battery read
/// first (its state decides asleep), then every device read, then the
/// write-only memory. An unreadable memory (or no state dir) still yields the
/// device half: every write-only value reads `None` and the CLI's refusal
/// rides in `Snapshot::memory_error`.
pub(crate) fn read_snapshot<T: Transport>(
    device: &Device<T>,
    state_dir: Option<&Path>,
    profile: Option<Profile>,
) -> Result<Snapshot, LoadError> {
    let battery = device.battery().map_err(failed)?;
    if battery.state == DeviceState::Asleep {
        return Err(LoadError::Asleep);
    }

    let firmware = device.firmware().map_err(failed)?;
    let active = device.profile().map_err(failed)?;
    let profile = profile.unwrap_or(active);
    let polling_rate = device.polling_rate().map_err(failed)?;
    let lift_off = device.lift_off().map_err(failed)?;
    let sleep = device.sleep_delay().map_err(failed)?;
    let dpi = device.dpi(profile).map_err(failed)?;
    let debounce = device.debounce(profile).map_err(failed)?;

    let (memory, memory_error) = match Memory::load(state_dir) {
        Ok(memory) => (Some(memory), None),
        Err(error) => (None, Some(render(error))),
    };

    let mut snap = Snapshot {
        model: device.model(),
        battery,
        firmware,
        active,
        profile,
        polling_rate,
        lift_off,
        sleep,
        brightness: None,
        scroll: None,
        dpi: dpi.stages,
        dpi_stage: dpi.active,
        dpi_colors: None,
        debounce,
        effect: None,
        binds: crate::get::BUTTONS
            .iter()
            .map(|button| (*button, None))
            .collect(),
        memory_error: None,
    };
    fill_write_only(&mut snap, memory.as_ref(), memory_error);
    Ok(snap)
}

/// Runs one Apply through the CLI's shared write path, reporting the ops that
/// landed before a fail-fast error for the window's snapshot patch.
pub(crate) fn apply<T: Transport>(
    device: &Device<T>,
    state_dir: Option<&Path>,
    profile: Profile,
    ops: &[WriteOp],
) -> Result<(), WriteFailure> {
    check_awake(device).map_err(|error| WriteFailure {
        message: render(error),
        landed: Vec::new(),
        reached: false,
    })?;
    crate::writes::apply(device, state_dir, profile, ops)
        .map(|_| ())
        .map_err(|failure| WriteFailure {
            message: render(failure.error),
            landed: failure.landed,
            reached: failure.reached,
        })
}

/// Re-loads the write-only memory the write path saved, for a write's answer
/// whose device re-read failed. It takes no lock itself, and its callers must
/// not wrap it in one: reading the file sends nothing to the mouse, and
/// `Memory::save` is atomic. No test reaches the window's call sites.
pub(crate) fn read_memory(state_dir: Option<&Path>) -> Result<Memory, String> {
    Memory::load(state_dir).map_err(render)
}

/// Makes a profile active, after a status check.
pub(crate) fn make_active<T: Transport>(
    device: &Device<T>,
    profile: Profile,
) -> Result<(), String> {
    check_awake(device).map_err(render)?;
    device
        .set_profile(profile)
        .map_err(|error| render(CmdError::from(error)))
}

/// Checks an import file before anything touches the mouse: resolves the path,
/// prepares it and reports the device-wide flag, the model note and the writes
/// the file will make. The one place an `ImportCheck` is built.
pub(crate) fn check_import(
    path: &Path,
    profile: Profile,
    device_model: &str,
) -> Result<ImportCheck, String> {
    let path =
        std::path::absolute(path).map_err(|error| format!("cannot resolve {path:?}: {error}"))?;
    let prepared = crate::import::prepare(path, Some(profile.get()), false).map_err(render)?;
    Ok(ImportCheck {
        note: prepared.model_note(device_model),
        device_wide: prepared.has_device_table(),
        writes: prepared.writes(),
    })
}

/// Runs one import after the user confirmed its check, reporting the ops that
/// landed on failure too. Re-checks the file against `confirmed` first, writes
/// included; a file whose check changed since the question writes nothing and
/// names the absolute path.
pub(crate) fn run_import<T: Transport>(
    device: &Device<T>,
    state_dir: Option<&Path>,
    profile: Profile,
    path: &Path,
    confirmed: &ImportCheck,
) -> Result<Vec<WriteOp>, WriteFailure> {
    let prepared =
        prepare_confirmed(device, profile, path, confirmed).map_err(|message| WriteFailure {
            message,
            landed: Vec::new(),
            reached: false,
        })?;
    crate::import::execute_landed(device, state_dir, prepared).map_err(|failure| WriteFailure {
        message: render(failure.error),
        landed: failure.landed,
        reached: failure.reached,
    })
}

/// Re-checks the import file against the confirmed check, returning the
/// prepared import when it still matches.
fn prepare_confirmed<T: Transport>(
    device: &Device<T>,
    profile: Profile,
    path: &Path,
    confirmed: &ImportCheck,
) -> Result<crate::import::PreparedImport, String> {
    let path =
        std::path::absolute(path).map_err(|error| format!("cannot resolve {path:?}: {error}"))?;
    let prepared =
        crate::import::prepare(path.clone(), Some(profile.get()), false).map_err(render)?;
    let check = ImportCheck {
        note: prepared.model_note(device.model().name),
        device_wide: prepared.has_device_table(),
        writes: prepared.writes(),
    };
    if check != *confirmed {
        return Err(format!(
            "{path:?} changed since it was checked; import it again"
        ));
    }
    Ok(prepared)
}

/// Runs `run` while holding the cross-process lock, releasing it after.
/// `on_wait` fires once, before blocking, when another process holds the lock;
/// it never fires when the lock is free. The wait is learned from `take_lock`'s
/// writer argument, never by parsing its stderr text.
pub(crate) fn with_lock<R>(
    path: Option<&Path>,
    on_wait: impl FnOnce(),
    run: impl FnOnce() -> R,
) -> Result<R, String> {
    let Some(path) = path else {
        return Err(NO_LOCK_LOCATION.to_owned());
    };
    let file =
        crate::take_lock(path, &mut WaitWriter::new(on_wait)).map_err(|error| error.to_string())?;
    let result = run();
    drop(file);
    Ok(result)
}

/// A writer that fires `on_wait` on its first write and discards the bytes.
/// `take_lock` writes its "waiting" line only when the lock is contended, so
/// `on_wait` fires exactly then and exactly once.
struct WaitWriter<F> {
    on_wait: Option<F>,
}

impl<F: FnOnce()> WaitWriter<F> {
    fn new(on_wait: F) -> Self {
        Self {
            on_wait: Some(on_wait),
        }
    }
}

impl<F: FnOnce()> Write for WaitWriter<F> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(on_wait) = self.on_wait.take() {
            on_wait();
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A device error as the CLI renders it, classified by whether the transport
/// failed (the handle is dead) or the device reported a bad value.
fn failed(error: gctl_proto::Error) -> LoadError {
    let transport = matches!(&error, gctl_proto::Error::Transport(_));
    let message = render(CmdError::from(error));
    if transport {
        LoadError::Transport(message)
    } else {
        LoadError::Failed(message)
    }
}

/// A command error as its CLI rendering, on one or more lines. The one error
/// renderer, shared by the GUI's transactions and the window.
pub(crate) fn render(error: CmdError) -> String {
    error.message().join("\n")
}

#[cfg(test)]
#[path = "../../tests/unit/gui_io.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/unit/gui_landed.rs"]
mod landed_tests;
