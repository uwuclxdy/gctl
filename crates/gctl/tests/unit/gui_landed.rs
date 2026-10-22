#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The io side reporting the ops that landed before a fail-fast error, against
//! a fake mouse that fails mid-sequence.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;

use gctl_proto::{
    Debounce, Device, Dpi, DpiStages, Effect, MODELS, PollingRate, Profile, Report, Transport,
    TransportError,
};

use super::*;
use crate::writes::WriteOp;

/// A fake mouse with the same status reply as `tests/unit/gui_io.rs`, whose
/// writes fail once a given number of writes have gone through.
struct Mouse {
    replies: RefCell<HashMap<(u8, u8), Report>>,
    sent: RefCell<Vec<Report>>,
    /// Writes with index `>= fail_after_writes` fail, so `1` fails the second
    /// write mid-sequence.
    fail_after_writes: Cell<usize>,
    /// Reads already received, so a read at index `>= fail_after_reads` fails.
    reads: Cell<usize>,
    fail_after_reads: Cell<usize>,
}

impl Mouse {
    fn new() -> Self {
        let mouse = Self {
            replies: RefCell::default(),
            sent: RefCell::default(),
            fail_after_writes: Cell::new(usize::MAX),
            reads: Cell::new(0),
            fail_after_reads: Cell::new(usize::MAX),
        };
        mouse.reply(0x00, 0x83, &[(1, 0xA1), (8, 35)]);
        mouse
    }

    fn reply(&self, sub: u8, command: u8, bytes: &[(usize, u8)]) {
        let mut report = [0u8; 65];
        report[6] = command;
        for &(i, b) in bytes {
            report[i] = b;
        }
        self.replies.borrow_mut().insert((sub, command), report);
    }

    fn sent(&self) -> Vec<Report> {
        self.sent.borrow().clone()
    }

    fn writes(&self) -> Vec<Report> {
        self.sent().into_iter().filter(|r| r[6] < 0x80).collect()
    }
}

impl Transport for Mouse {
    fn send(&self, report: &Report) -> Result<(), TransportError> {
        if report[6] < 0x80 {
            let written = self.sent.borrow().iter().filter(|r| r[6] < 0x80).count();
            if written >= self.fail_after_writes.get() {
                return Err("write refused mid-sequence".into());
            }
        }
        self.sent.borrow_mut().push(*report);
        Ok(())
    }

    fn receive(&self, report: &mut Report) -> Result<(), TransportError> {
        let reads = self.reads.get();
        self.reads.set(reads + 1);
        if reads >= self.fail_after_reads.get() {
            return Err("read refused by the fake".into());
        }
        let key = self.sent.borrow().last().map_or((0, 0), |r| (r[5], r[6]));
        *report = self.replies.borrow().get(&key).copied().unwrap_or([0; 65]);
        Ok(())
    }
}

fn device(mouse: &Mouse) -> Device<&Mouse> {
    Device::new(mouse, &MODELS[0]).with_io_delay(std::time::Duration::ZERO)
}

fn profile(id: u8) -> Profile {
    Profile::new(id).unwrap()
}

/// Device-only ops: the no-memory branch of `run_writes`.
fn no_memory_ops() -> Vec<WriteOp> {
    vec![
        WriteOp::PollingRate(PollingRate::Hz500),
        WriteOp::Debounce(Debounce::new(4).unwrap()),
    ]
}

/// A write-only op in the plan: the memory branch of `run_writes`.
fn memory_ops() -> Vec<WriteOp> {
    vec![
        WriteOp::PollingRate(PollingRate::Hz500),
        WriteOp::Effect(Effect::Off),
    ]
}

const MESSAGE: &str = "HID transport failed: write refused mid-sequence";

const IMPORT: &str = "format = 1\nmodel = \"Model O Wireless\"\n[device]\npolling_rate = 500\n[profile]\ndebounce = 4\n";

#[test]
fn a_failed_apply_reports_the_ops_that_landed_before_the_failure() {
    let dir = tempfile::tempdir().unwrap();
    let mouse = Mouse::new();
    mouse.fail_after_writes.set(1);
    assert_eq!(
        apply(
            &device(&mouse),
            Some(dir.path()),
            profile(2),
            &no_memory_ops()
        ),
        Err(WriteFailure {
            message: MESSAGE.to_owned(),
            landed: vec![WriteOp::PollingRate(PollingRate::Hz500)],
            reached: true,
        })
    );
    assert_eq!(mouse.writes().len(), 1);
}

#[test]
fn a_failed_memory_branch_apply_reports_the_landed_ops() {
    let dir = tempfile::tempdir().unwrap();
    let mouse = Mouse::new();
    mouse.fail_after_writes.set(1);
    assert_eq!(
        apply(&device(&mouse), Some(dir.path()), profile(2), &memory_ops()),
        Err(WriteFailure {
            message: MESSAGE.to_owned(),
            landed: vec![WriteOp::PollingRate(PollingRate::Hz500)],
            reached: true,
        })
    );
    assert_eq!(mouse.writes().len(), 1);
}

#[test]
fn a_failed_import_reports_the_ops_that_landed_before_the_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.toml");
    std::fs::write(&path, IMPORT).unwrap();
    let mouse = Mouse::new();
    mouse.fail_after_writes.set(1);
    let confirmed = check_import(&path, profile(2), "Model O Wireless").unwrap();
    assert_eq!(
        run_import(
            &device(&mouse),
            Some(dir.path()),
            profile(2),
            &path,
            &confirmed,
        ),
        Err(WriteFailure {
            message: MESSAGE.to_owned(),
            landed: vec![WriteOp::PollingRate(PollingRate::Hz500)],
            reached: true,
        })
    );
    assert_eq!(mouse.writes().len(), 1);
}

#[test]
fn a_memory_save_failure_reports_the_whole_plan_as_landed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let mouse = Mouse::new();
    let ops = vec![WriteOp::Effect(Effect::Off)];
    let result = apply(&device(&mouse), Some(dir.path()), profile(2), &ops);
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    // No op failed, so no failing report is attributed to this failure: the
    // landed plan explains the fields it wrote instead.
    assert_eq!(
        result,
        Err(WriteFailure {
            message: format!(
                "could not save remembered values to {:?}: Permission denied (os error 13) (the mouse was written)",
                dir.path().join("memory.toml")
            ),
            landed: ops,
            reached: false,
        })
    );
}

/// A DPI write reads the stage count before its report; a failed read sends
/// nothing, so the failure owns no field. The awake check's read comes first.
#[test]
fn a_pre_write_read_failure_owns_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mouse = Mouse::new();
    mouse.fail_after_reads.set(1);
    let stages = DpiStages::new(vec![Dpi::new(400).unwrap(), Dpi::new(800).unwrap()]).unwrap();
    assert_eq!(
        apply(
            &device(&mouse),
            Some(dir.path()),
            profile(2),
            &[WriteOp::Dpi(stages)]
        ),
        Err(WriteFailure {
            message: "HID transport failed: read refused by the fake".to_owned(),
            landed: Vec::new(),
            reached: false,
        })
    );
    assert!(mouse.writes().is_empty());
}
