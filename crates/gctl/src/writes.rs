//! Device writes that also maintain the write-only memory: record on success,
//! clear that setting's memory on failure. Shared by `set`, `import` and the GUI.

use std::path::Path;

use gctl_proto::{
    Binding, Brightness, Button, Debounce, Device, DpiStage, DpiStages, Effect, LiftOff, MouseFn,
    PollingRate, Profile, Rgb, ScrollDirection, SleepDelay, Transport,
};

use crate::CmdError;
use crate::memory::{self, Memory};
use crate::parse;

/// A write failure with the ops that landed before it.
#[derive(Debug)]
pub(crate) struct WriteError {
    pub(crate) error: CmdError,
    pub(crate) landed: Vec<WriteOp>,
    /// Whether the failing op handed at least one write report to the transport
    /// (a report that failed counts: it may have landed); the ops that landed
    /// before it are in `landed`. The GUI's "this window wrote it" rule reads
    /// it; the CLI reads `error` only.
    pub(crate) reached: bool,
}

/// A fail-fast write's error, with the ops that landed before it and whether
/// the failing op handed at least one write report to the transport.
#[derive(Debug)]
pub(crate) struct WriteAttempt {
    error: gctl_proto::Error,
    landed: Vec<WriteOp>,
    reached: bool,
}

/// The `reached` value of a failure with no failing operation (a memory
/// refusal, or every op landed and the save then failed): no report is
/// attributed to it, so it owns no field.
const NO_FAILING_OP: bool = false;

/// Whether the operation that just failed handed a write report to the
/// transport: the count moved, even when the send itself failed.
pub(crate) fn sent_since<T: Transport>(device: &Device<T>, before: u64) -> bool {
    device.writes_sent() != before
}

/// The scroll binds a direction writes to the scroll-up/down buttons: invert
/// swaps the two mouse functions. The direction itself is never stored; it
/// derives from these binds.
fn scroll_binds(direction: ScrollDirection) -> (MouseFn, MouseFn) {
    match direction {
        ScrollDirection::Default => (MouseFn::ScrollUp, MouseFn::ScrollDown),
        ScrollDirection::Invert => (MouseFn::ScrollDown, MouseFn::ScrollUp),
    }
}

/// Whether a `set dpi` changes the stage count, so the profile's remembered
/// colours no longer line up.
fn dpi_changes_stage_count(old_count: usize, stages: &DpiStages) -> bool {
    stages.as_slice().len() != old_count
}

/// Records one LANDED write's memory effect (record on success). The `Dpi` arm
/// drops the profile's colours when the stage count changed; `old_stage_count`
/// is the count before the write. `profile` is `None` only for device-wide ops.
fn record_op(
    model: &str,
    profile: Option<Profile>,
    op: &WriteOp,
    old_stage_count: Option<usize>,
    memory: &mut Memory,
) {
    match op {
        WriteOp::Brightness(value) => memory.model(model).brightness = Some(*value),
        WriteOp::Scroll(direction) => {
            let (up, down) = scroll_binds(*direction);
            for profile in Profile::all() {
                let mem = memory.profile(model, profile);
                mem.binds.insert("scroll-up".to_owned(), Binding::Mouse(up));
                mem.binds
                    .insert("scroll-down".to_owned(), Binding::Mouse(down));
            }
        }
        WriteOp::Dpi(stages) => {
            let Some(profile) = profile else {
                return;
            };
            if old_stage_count.is_some_and(|old| dpi_changes_stage_count(old, stages)) {
                memory.profile(model, profile).dpi_colors = None;
            }
        }
        WriteOp::DpiColors(colors) => {
            let Some(profile) = profile else {
                return;
            };
            memory.profile(model, profile).dpi_colors = Some(colors.clone());
        }
        WriteOp::Effect(effect) => {
            let Some(profile) = profile else {
                return;
            };
            memory.profile(model, profile).effect = Some(effect.clone());
        }
        WriteOp::Bind(button, binding) => {
            let Some(profile) = profile else {
                return;
            };
            memory
                .profile(model, profile)
                .binds
                .insert(parse::format_button(*button).to_owned(), *binding);
        }
        WriteOp::PollingRate(_)
        | WriteOp::LiftOff(_)
        | WriteOp::Sleep(_)
        | WriteOp::DpiStage(_)
        | WriteOp::Debounce(_) => {}
    }
}

/// Clears one FAILED write's memory effect (a failed write clears its
/// remembered value, since the report may have half-written).
fn clear_op(
    model: &str,
    profile: Option<Profile>,
    op: &WriteOp,
    old_stage_count: Option<usize>,
    memory: &mut Memory,
) {
    match op {
        WriteOp::Brightness(_) => memory.model(model).brightness = None,
        WriteOp::Scroll(_) => {
            for profile in Profile::all() {
                let mem = memory.profile(model, profile);
                mem.binds.remove("scroll-up");
                mem.binds.remove("scroll-down");
            }
        }
        WriteOp::Dpi(stages) => {
            let Some(profile) = profile else {
                return;
            };
            if old_stage_count.is_some_and(|old| dpi_changes_stage_count(old, stages)) {
                memory.profile(model, profile).dpi_colors = None;
            }
        }
        WriteOp::DpiColors(_) => {
            let Some(profile) = profile else {
                return;
            };
            memory.profile(model, profile).dpi_colors = None;
        }
        WriteOp::Effect(_) => {
            let Some(profile) = profile else {
                return;
            };
            memory.profile(model, profile).effect = None;
        }
        WriteOp::Bind(button, _) => {
            let Some(profile) = profile else {
                return;
            };
            memory
                .profile(model, profile)
                .binds
                .remove(parse::format_button(*button));
        }
        WriteOp::PollingRate(_)
        | WriteOp::LiftOff(_)
        | WriteOp::Sleep(_)
        | WriteOp::DpiStage(_)
        | WriteOp::Debounce(_) => {}
    }
}

/// Records a successful write's memory effect or clears a failed one's, and on
/// failure reports whether that op's report reached the transport.
fn finish<T: Transport>(
    device: &Device<T>,
    profile: Option<Profile>,
    op: &WriteOp,
    old_stage_count: Option<usize>,
    before: u64,
    result: gctl_proto::Result<()>,
    memory: &mut Memory,
) -> Result<(), WriteAttempt> {
    match result {
        Ok(()) => {
            record_op(device.model().name, profile, op, old_stage_count, memory);
            Ok(())
        }
        Err(error) => {
            clear_op(device.model().name, profile, op, old_stage_count, memory);
            Err(WriteAttempt {
                error,
                landed: Vec::new(),
                reached: sent_since(device, before),
            })
        }
    }
}

pub(crate) fn effect<T: Transport>(
    device: &Device<T>,
    profile: Profile,
    effect: &Effect,
    memory: &mut Memory,
) -> Result<(), WriteAttempt> {
    let op = WriteOp::Effect(effect.clone());
    let before = device.writes_sent();
    let result = device.set_effect(profile, effect);
    finish(device, Some(profile), &op, None, before, result, memory)
}

pub(crate) fn brightness<T: Transport>(
    device: &Device<T>,
    brightness: Brightness,
    memory: &mut Memory,
) -> Result<(), WriteAttempt> {
    let op = WriteOp::Brightness(brightness);
    let before = device.writes_sent();
    let result = device.set_brightness(brightness);
    finish(device, None, &op, None, before, result, memory)
}

pub(crate) fn dpi_colors<T: Transport>(
    device: &Device<T>,
    profile: Profile,
    colors: &[Rgb],
    memory: &mut Memory,
) -> Result<(), WriteAttempt> {
    let op = WriteOp::DpiColors(colors.to_vec());
    let before = device.writes_sent();
    let result = device.set_dpi_colors(profile, colors);
    finish(device, Some(profile), &op, None, before, result, memory)
}

pub(crate) fn bind<T: Transport>(
    device: &Device<T>,
    profile: Profile,
    button: Button,
    binding: Binding,
    memory: &mut Memory,
) -> Result<(), WriteAttempt> {
    let op = WriteOp::Bind(button, binding);
    let before = device.writes_sent();
    let result = device.bind(profile, button, binding);
    finish(device, Some(profile), &op, None, before, result, memory)
}

pub(crate) fn scroll<T: Transport>(
    device: &Device<T>,
    direction: ScrollDirection,
    memory: &mut Memory,
) -> Result<(), WriteAttempt> {
    let op = WriteOp::Scroll(direction);
    let before = device.writes_sent();
    let result = device.set_scroll(direction);
    finish(device, None, &op, None, before, result, memory)
}

/// Writes a profile's DPI stage list; when the stage count changes, that
/// profile's remembered DPI colours no longer line up and are dropped. A failed
/// write drops them too: the stage-list report may have reached the mouse.
pub(crate) fn dpi_stages<T: Transport>(
    device: &Device<T>,
    profile: Profile,
    stages: &DpiStages,
    memory: &mut Memory,
) -> Result<(), WriteAttempt> {
    let before = device.writes_sent();
    let old_count = match device.dpi(profile) {
        Ok(config) => config.stages.as_slice().len(),
        Err(error) => {
            return Err(WriteAttempt {
                error,
                landed: Vec::new(),
                reached: sent_since(device, before),
            });
        }
    };
    let result = device.set_dpi_stages(profile, stages);
    let op = WriteOp::Dpi(stages.clone());
    finish(
        device,
        Some(profile),
        &op,
        Some(old_count),
        before,
        result,
        memory,
    )
}

/// Runs one write, then persists its memory effect: recorded on success, the
/// setting cleared on failure. A save failure after a failed write is reported
/// on a second line; after a successful write it names the mouse as written.
pub(crate) fn with_memory(
    state_dir: Option<&Path>,
    write: impl FnOnce(&mut Memory) -> Result<(), WriteAttempt>,
) -> Result<(), CmdError> {
    with_memory_landed(state_dir, &[], |memory| write(memory).map(|()| Vec::new()))
        .map(|_| ())
        .map_err(|failure| failure.error)
}

/// `with_memory` with the ops a landed write reports back, so a fail-fast
/// caller can patch its snapshot with exactly the ops that landed. The closure
/// also reports whether its failing op sent a write report.
pub(crate) fn with_memory_landed(
    state_dir: Option<&Path>,
    ops: &[WriteOp],
    write: impl FnOnce(&mut Memory) -> Result<Vec<WriteOp>, WriteAttempt>,
) -> Result<Vec<WriteOp>, WriteError> {
    let state_dir = memory::require_state_dir(state_dir).map_err(|error| WriteError {
        error,
        landed: Vec::new(),
        reached: NO_FAILING_OP,
    })?;
    let mut memory = Memory::load(Some(state_dir)).map_err(|error| WriteError {
        error,
        landed: Vec::new(),
        reached: NO_FAILING_OP,
    })?;
    match write(&mut memory) {
        Ok(landed) => match memory.save(state_dir) {
            Ok(()) => Ok(landed),
            Err(error) => Err(WriteError {
                error: CmdError::Message(memory::save_message(state_dir, &error, true)),
                landed: ops.to_vec(),
                reached: NO_FAILING_OP,
            }),
        },
        Err(WriteAttempt {
            error: device_error,
            landed,
            reached,
        }) => match memory.save(state_dir) {
            Ok(()) => Err(WriteError {
                error: CmdError::Runtime(device_error),
                landed,
                reached,
            }),
            Err(error) => Err(WriteError {
                error: CmdError::WriteThenSave {
                    device_error,
                    save_message: memory::save_message(state_dir, &error, false),
                },
                landed,
                reached,
            }),
        },
    }
}

/// One device write a transaction makes, in the CLI's write order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WriteOp {
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
    pub(crate) fn needs_memory(&self) -> bool {
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

    /// Whether the write changes a device-wide value, one every profile shares.
    pub(crate) fn device_wide(&self) -> bool {
        match self {
            WriteOp::PollingRate(_)
            | WriteOp::LiftOff(_)
            | WriteOp::Sleep(_)
            | WriteOp::Brightness(_)
            | WriteOp::Scroll(_) => true,
            WriteOp::Dpi(_)
            | WriteOp::DpiStage(_)
            | WriteOp::DpiColors(_)
            | WriteOp::Debounce(_)
            | WriteOp::Effect(_)
            | WriteOp::Bind(_, _) => false,
        }
    }
}

/// Runs the shared write path and reports the ops that landed, on success and
/// on a fail-fast error.
pub(crate) fn apply<T: Transport>(
    device: &Device<T>,
    state_dir: Option<&Path>,
    profile: Profile,
    ops: &[WriteOp],
) -> Result<Vec<WriteOp>, WriteError> {
    // Before the first write: memory loads iff some operation records one, so a
    // refusal leaves the mouse untouched. The question is answered by the same
    // variants the writes run, so the gate cannot drift from them.
    if ops.iter().any(WriteOp::needs_memory) {
        with_memory_landed(state_dir, ops, |memory| {
            run_writes(device, profile, ops, memory)
        })
    } else {
        run_writes(device, profile, ops, &mut Memory::default()).map_err(|attempt| WriteError {
            error: CmdError::from(attempt.error),
            landed: attempt.landed,
            reached: attempt.reached,
        })
    }
}

/// Writes each operation against the live device, updating `memory` for the
/// write-only settings through `record_op`/`clear_op`. Fail-fast: returns the
/// ops that landed before the first error; the error says whether that op's own
/// report was handed to the transport.
pub(crate) fn run_writes<T: Transport>(
    device: &Device<T>,
    profile: Profile,
    ops: &[WriteOp],
    memory: &mut Memory,
) -> Result<Vec<WriteOp>, WriteAttempt> {
    let mut landed = Vec::new();
    for op in ops {
        // Taken before this op's own pre-read: an earlier op that landed is
        // explained by `landed`, never by this failure's `reached`.
        let before = device.writes_sent();
        // `set dpi` needs the stage count read before the write, so its colour
        // drop matches the memory's rule exactly.
        let old_stage_count = match op {
            WriteOp::Dpi(_) => match device.dpi(profile) {
                Ok(config) => Some(config.stages.as_slice().len()),
                Err(error) => {
                    return Err(WriteAttempt {
                        error,
                        landed,
                        reached: sent_since(device, before),
                    });
                }
            },
            _ => None,
        };
        let result = match op {
            WriteOp::PollingRate(rate) => device.set_polling_rate(*rate),
            WriteOp::LiftOff(lift_off) => device.set_lift_off(*lift_off),
            WriteOp::Sleep(sleep) => device.set_sleep_delay(*sleep),
            WriteOp::Brightness(value) => device.set_brightness(*value),
            WriteOp::Scroll(direction) => device.set_scroll(*direction),
            WriteOp::Dpi(stages) => device.set_dpi_stages(profile, stages),
            WriteOp::DpiStage(stage) => device.set_dpi_stage(profile, *stage),
            WriteOp::DpiColors(colors) => device.set_dpi_colors(profile, colors),
            WriteOp::Debounce(debounce) => device.set_debounce(profile, *debounce),
            WriteOp::Effect(value) => device.set_effect(profile, value),
            WriteOp::Bind(button, binding) => device.bind(profile, *button, *binding),
        };
        match result {
            Ok(()) => {
                record_op(
                    device.model().name,
                    Some(profile),
                    op,
                    old_stage_count,
                    memory,
                );
                landed.push(op.clone());
            }
            Err(error) => {
                clear_op(
                    device.model().name,
                    Some(profile),
                    op,
                    old_stage_count,
                    memory,
                );
                return Err(WriteAttempt {
                    error,
                    landed,
                    reached: sent_since(device, before),
                });
            }
        }
    }
    Ok(landed)
}

#[cfg(test)]
#[path = "../tests/unit/memory_record.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/write_attempt.rs"]
mod attempt_tests;
