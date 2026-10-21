#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The GUI's device transactions against a fake mouse and a temp state dir:
//! the snapshot read, Apply through the CLI's write path, make-active, and the
//! lock held for exactly one transaction.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs::{File, TryLockError};
use std::path::Path;
use std::time::Duration;

use gctl_proto::{
    Battery, Binding, Brightness, Button, Debounce, Device, DeviceState, Dpi, DpiStage, DpiStages,
    Effect, Firmware, LiftOff, MODELS, MouseFn, PollingRate, Profile, Rate, Report, Rgb,
    ScrollDirection, SleepDelay, Transport, TransportError,
};
use tempfile::TempDir;

use super::*;
use crate::gui::state::{ImportCheck, LoadError, Snapshot, WriteFailure};
use crate::writes::WriteOp;

/// The awake Model O Wireless of `tests/cli.rs`: active profile 2, stages
/// 400 800 1600 on stage 2, 1000 Hz, 1 mm, 120 s sleep, debounce 10, 35%.
/// Replies are keyed by the `([5], [6])` pair of the last sent report.
struct Mouse {
    replies: RefCell<HashMap<(u8, u8), Report>>,
    sent: RefCell<Vec<Report>>,
    fail_writes: Cell<bool>,
    /// The `([5], [6])` read whose reply fails, as an unplugged dongle does.
    fail_read: Cell<Option<(u8, u8)>>,
}

impl Mouse {
    fn new() -> Self {
        let mouse = Self {
            replies: RefCell::default(),
            sent: RefCell::default(),
            fail_writes: Cell::new(false),
            fail_read: Cell::new(None),
        };
        mouse.reply(0x00, 0x83, &[(1, 0xA1), (8, 35)]);
        mouse.reply(0x00, 0x81, &[(7, 0), (8, 3), (9, 8), (10, 0)]);
        mouse.reply(0x00, 0x85, &[(7, 2)]);
        mouse.reply(
            0x01,
            0x81,
            &[
                (8, 3),
                (9, 0x01),
                (10, 0x90),
                (13, 0x03),
                (14, 0x20),
                (17, 0x06),
                (18, 0x40),
            ],
        );
        mouse.reply(0x01, 0x82, &[(8, 2)]);
        mouse.reply(0x01, 0x80, &[(7, 1)]);
        mouse.reply(0x01, 0x87, &[(7, 0)]);
        mouse.reply(0x00, 0x87, &[(7, 0x00), (8, 0x78)]);
        mouse.reply(0x00, 0x88, &[(8, 10)]);
        mouse
    }

    fn asleep() -> Self {
        let mouse = Self::new();
        mouse.reply(0x00, 0x83, &[(1, 0xA4), (8, 35)]);
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

    /// Every sent report that is not a read request (`[6]` below `0x80`).
    fn writes(&self) -> Vec<Report> {
        self.sent().into_iter().filter(|r| r[6] < 0x80).collect()
    }
}

impl Transport for Mouse {
    fn send(&self, report: &Report) -> Result<(), TransportError> {
        if self.fail_writes.get() && report[6] < 0x80 {
            return Err("write refused by the fake".into());
        }
        self.sent.borrow_mut().push(*report);
        Ok(())
    }

    fn receive(&self, report: &mut Report) -> Result<(), TransportError> {
        let key = self.sent.borrow().last().map_or((0, 0), |r| (r[5], r[6]));
        if self.fail_read.get() == Some(key) {
            return Err("device unplugged".into());
        }
        *report = self.replies.borrow().get(&key).copied().unwrap_or([0; 65]);
        Ok(())
    }
}

fn device(mouse: &Mouse) -> Device<&Mouse> {
    Device::new(mouse, &MODELS[0]).with_io_delay(Duration::ZERO)
}

fn profile(id: u8) -> Profile {
    Profile::new(id).unwrap()
}

fn rgb(hex: &str) -> Rgb {
    hex.parse().unwrap()
}

/// The snapshot the fake mouse reads as, with nothing remembered.
fn expected(edited: u8) -> Snapshot {
    Snapshot {
        model: &MODELS[0],
        battery: Battery {
            state: DeviceState::Awake,
            percent: Some(35),
        },
        firmware: Firmware([0, 3, 8, 0]),
        active: profile(2),
        profile: profile(edited),
        polling_rate: PollingRate::Hz1000,
        lift_off: LiftOff::Mm1,
        sleep: SleepDelay::Seconds(120),
        brightness: None,
        scroll: None,
        dpi: DpiStages::new(vec![
            Dpi::new(400).unwrap(),
            Dpi::new(800).unwrap(),
            Dpi::new(1600).unwrap(),
        ])
        .unwrap(),
        dpi_stage: DpiStage::new(2).unwrap(),
        dpi_colors: None,
        debounce: Debounce::new(10).unwrap(),
        effect: None,
        binds: [
            Button::Left,
            Button::Right,
            Button::Scroll,
            Button::Back,
            Button::Forward,
            Button::ScrollUp,
            Button::ScrollDown,
            Button::Dpi,
        ]
        .iter()
        .map(|button| (*button, None))
        .collect(),
        memory_error: None,
    }
}

fn state() -> TempDir {
    tempfile::tempdir().unwrap()
}

fn write_memory(dir: &Path, text: &str) {
    std::fs::write(dir.join("memory.toml"), text).unwrap();
}

// --- snapshot ---------------------------------------------------------------

#[test]
fn the_snapshot_reads_the_active_profile_when_none_is_named() {
    let dir = state();
    let mouse = Mouse::new();
    let device = device(&mouse);
    assert_eq!(
        read_snapshot(&device, Some(dir.path()), None),
        Ok(expected(2))
    );
}

#[test]
fn the_snapshot_reads_the_named_profile_and_still_reports_the_active_one() {
    let dir = state();
    let mouse = Mouse::new();
    let device = device(&mouse);
    assert_eq!(
        read_snapshot(&device, Some(dir.path()), Some(profile(3))),
        Ok(expected(3))
    );
}

#[test]
fn the_snapshot_carries_remembered_values_for_the_edited_profile() {
    let dir = state();
    write_memory(
        dir.path(),
        r#"
[model."Model O Wireless"]
brightness = "200 50"

[model."Model O Wireless".profile.1.binds]
scroll-up = "mouse:scroll-down"
scroll-down = "mouse:scroll-up"

[model."Model O Wireless".profile.2]
effect = "wave rate=40"
dpi_colors = "ff0000 00ff00 0000ff"

[model."Model O Wireless".profile.2.binds]
back = "mouse:forward"
scroll-up = "mouse:scroll-down"
scroll-down = "mouse:scroll-up"

[model."Model O Wireless".profile.3.binds]
scroll-up = "mouse:scroll-down"
scroll-down = "mouse:scroll-up"
"#,
    );
    let mouse = Mouse::new();
    let device = device(&mouse);
    let snap = read_snapshot(&device, Some(dir.path()), None).unwrap();
    assert_eq!(
        snap.brightness,
        Some(Brightness {
            wired: 200,
            wireless: 50
        })
    );
    assert_eq!(
        snap.effect,
        Some(Effect::Wave {
            rate: Rate::new(40).unwrap()
        })
    );
    assert_eq!(
        snap.dpi_colors,
        Some(vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")])
    );
    assert_eq!(snap.scroll, Some(ScrollDirection::Invert));
    assert_eq!(
        snap.binds[3],
        (Button::Back, Some(Binding::Mouse(MouseFn::Forward)))
    );
    assert_eq!(
        snap.binds[5],
        (Button::ScrollUp, Some(Binding::Mouse(MouseFn::ScrollDown)))
    );
    assert_eq!(snap.binds[0], (Button::Left, None));

    // profile 3 remembers no effect
    let three = read_snapshot(&device, Some(dir.path()), Some(profile(3))).unwrap();
    assert_eq!(three.effect, None);
    assert_eq!(three.dpi_colors, None);
}

#[test]
fn remembered_colours_that_no_longer_match_the_stage_count_read_as_unknown() {
    let dir = state();
    write_memory(
        dir.path(),
        "[model.\"Model O Wireless\".profile.2]\ndpi_colors = \"ff0000 00ff00\"\n",
    );
    let mouse = Mouse::new();
    let device = device(&mouse);
    let snap = read_snapshot(&device, Some(dir.path()), None).unwrap();
    assert_eq!(snap.dpi.as_slice().len(), 3);
    assert_eq!(snap.dpi_colors, None);
}

#[test]
fn an_asleep_mouse_reads_nothing_past_the_status() {
    let dir = state();
    let mouse = Mouse::asleep();
    let device = device(&mouse);
    assert_eq!(
        read_snapshot(&device, Some(dir.path()), None),
        Err(LoadError::Asleep)
    );
    let sent = mouse.sent();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!((sent[0][5], sent[0][6]), (0x00, 0x83));
}

#[test]
fn unreadable_memory_still_reads_the_device_and_leaves_the_file_alone() {
    let dir = state();
    let path = dir.path().join("memory.toml");
    let text = "[model.\"Model O Wireless\".profile.2]\neffect = \"rainbow\"\n";
    write_memory(dir.path(), text);
    let mouse = Mouse::new();
    let device = device(&mouse);
    assert_eq!(
        read_snapshot(&device, Some(dir.path()), None),
        Ok(Snapshot {
            memory_error: Some(format!(
                "cannot read remembered values from {path:?}: unknown effect \"rainbow\"; fix or delete the file"
            )),
            ..expected(2)
        })
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn no_state_dir_still_reads_the_device_and_says_why_nothing_is_remembered() {
    let mouse = Mouse::new();
    let device = device(&mouse);
    assert_eq!(
        read_snapshot(&device, None, None),
        Ok(Snapshot {
            memory_error: Some(
                "cannot find a state directory; set XDG_STATE_HOME or HOME".to_owned()
            ),
            ..expected(2)
        })
    );
}

#[test]
fn a_transport_error_in_any_read_is_told_apart_from_other_failures() {
    // the battery read comes first, the debounce read last
    for read in [(0x00, 0x83), (0x01, 0x81), (0x00, 0x88)] {
        let dir = state();
        let mouse = Mouse::new();
        mouse.fail_read.set(Some(read));
        assert_eq!(
            read_snapshot(&device(&mouse), Some(dir.path()), None),
            Err(LoadError::Transport(
                "HID transport failed: device unplugged".to_owned()
            )),
            "{read:02x?}"
        );
    }
}

#[test]
fn an_implausible_device_value_is_a_failure_that_keeps_the_handle() {
    let dir = state();
    let mouse = Mouse::new();
    // polling-rate interval byte 9 maps to no rate
    mouse.reply(0x01, 0x80, &[(7, 9)]);
    assert!(
        matches!(
            read_snapshot(&device(&mouse), Some(dir.path()), None),
            Err(LoadError::Failed(_))
        ),
        "an implausible value is no transport error"
    );
}

// --- Apply ------------------------------------------------------------------

#[test]
fn apply_writes_through_the_cli_path_and_records_write_only_values() {
    let dir = state();
    let mouse = Mouse::new();
    let device = device(&mouse);
    let ops = [
        WriteOp::PollingRate(PollingRate::Hz500),
        WriteOp::Effect(Effect::Wave {
            rate: Rate::new(60).unwrap(),
        }),
    ];
    assert_eq!(apply(&device, Some(dir.path()), profile(2), &ops), Ok(()));
    let writes = mouse.writes();
    assert_eq!(writes.len(), 2, "{writes:?}");
    // the polling-rate write carries the 2 ms interval
    assert_eq!((writes[0][5], writes[0][6], writes[0][7]), (0x01, 0x00, 2));
    let snap = read_snapshot(&device, Some(dir.path()), None).unwrap();
    assert_eq!(
        snap.effect,
        Some(Effect::Wave {
            rate: Rate::new(60).unwrap()
        })
    );
}

#[test]
fn apply_refuses_an_asleep_mouse_with_nothing_written() {
    let dir = state();
    let mouse = Mouse::asleep();
    let device = device(&mouse);
    assert_eq!(
        apply(
            &device,
            Some(dir.path()),
            profile(2),
            &[WriteOp::Debounce(Debounce::new(4).unwrap())]
        ),
        Err(WriteFailure {
            message: "device is asleep".to_owned(),
            landed: Vec::new(),
        })
    );
    assert!(mouse.writes().is_empty());
}

#[test]
fn a_failed_apply_reports_the_chain_and_forgets_the_value_it_may_have_half_written() {
    let dir = state();
    write_memory(
        dir.path(),
        "[model.\"Model O Wireless\".profile.2]\neffect = \"wave rate=40\"\n",
    );
    let mouse = Mouse::new();
    mouse.fail_writes.set(true);
    let device = device(&mouse);
    assert_eq!(
        apply(
            &device,
            Some(dir.path()),
            profile(2),
            &[WriteOp::Effect(Effect::Off)]
        ),
        Err(WriteFailure {
            message: "HID transport failed: write refused by the fake".to_owned(),
            landed: Vec::new(),
        })
    );
    mouse.fail_writes.set(false);
    let snap = read_snapshot(&device, Some(dir.path()), None).unwrap();
    assert_eq!(snap.effect, None);
}

#[test]
fn make_active_writes_the_profile_and_refuses_an_asleep_mouse() {
    let mouse = Mouse::new();
    assert_eq!(make_active(&device(&mouse), profile(3)), Ok(()));
    let writes = mouse.writes();
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert_eq!(writes[0][7], 3);

    let asleep = Mouse::asleep();
    assert_eq!(
        make_active(&device(&asleep), profile(3)),
        Err("device is asleep".to_owned())
    );
    assert!(asleep.writes().is_empty());
}

// --- import -----------------------------------------------------------------

const PROFILE_ONLY: &str = "format = 1\nmodel = \"Model O Wireless\"\n[profile]\ndebounce = 4\n";

#[test]
fn the_import_check_reports_the_device_table_and_the_model_note() {
    let dir = state();
    let path = dir.path().join("p.toml");
    std::fs::write(
        &path,
        "format = 1\nmodel = \"Model D Wireless\"\n[device]\npolling_rate = 500\n[profile]\ndebounce = 4\n",
    )
    .unwrap();
    assert_eq!(
        check_import(&path, profile(2), "Model O Wireless"),
        Ok(ImportCheck {
            note: Some(format!(
                "note: {path:?} holds settings for \"Model D Wireless\"; writing them to Model O Wireless"
            )),
            device_wide: true,
            // device-wide first, then the profile: the order the import writes in
            writes: vec![
                WriteOp::PollingRate(PollingRate::Hz500),
                WriteOp::Debounce(Debounce::new(4).unwrap()),
            ],
        })
    );

    std::fs::write(&path, PROFILE_ONLY).unwrap();
    assert_eq!(
        check_import(&path, profile(2), "Model O Wireless"),
        Ok(ImportCheck {
            note: None,
            device_wide: false,
            writes: vec![WriteOp::Debounce(Debounce::new(4).unwrap())],
        })
    );
}

#[test]
fn an_import_runs_when_the_file_still_matches_its_check() {
    let dir = state();
    let path = dir.path().join("p.toml");
    std::fs::write(&path, PROFILE_ONLY).unwrap();
    let mouse = Mouse::new();
    let confirmed = ImportCheck {
        note: None,
        device_wide: false,
        writes: vec![WriteOp::Debounce(Debounce::new(4).unwrap())],
    };
    assert_eq!(
        run_import(
            &device(&mouse),
            Some(dir.path()),
            profile(2),
            &path,
            &confirmed
        ),
        // the writes the import made, so the window can end the edits they carried
        Ok(vec![WriteOp::Debounce(Debounce::new(4).unwrap())])
    );
    let writes = mouse.writes();
    assert_eq!(writes.len(), 1, "one debounce write: {writes:?}");
}

#[test]
fn an_import_whose_file_changed_since_its_check_writes_nothing() {
    let dir = state();
    let path = dir.path().join("p.toml");
    std::fs::write(&path, PROFILE_ONLY).unwrap();
    let confirmed = check_import(&path, profile(2), "Model O Wireless").unwrap();
    // the file gains device-wide settings while the user reads the question
    std::fs::write(
        &path,
        "format = 1\nmodel = \"Model O Wireless\"\n[device]\npolling_rate = 500\n[profile]\ndebounce = 4\n",
    )
    .unwrap();
    let mouse = Mouse::new();
    assert_eq!(
        run_import(
            &device(&mouse),
            Some(dir.path()),
            profile(2),
            &path,
            &confirmed
        ),
        Err(WriteFailure {
            message: format!("{path:?} changed since it was checked; import it again"),
            landed: Vec::new(),
        })
    );
    assert!(mouse.writes().is_empty());
}

#[test]
fn an_import_whose_values_changed_since_its_check_writes_nothing() {
    let dir = state();
    let path = dir.path().join("p.toml");
    std::fs::write(&path, PROFILE_ONLY).unwrap();
    let confirmed = check_import(&path, profile(2), "Model O Wireless").unwrap();
    // same tables, same model, another value
    std::fs::write(
        &path,
        "format = 1\nmodel = \"Model O Wireless\"\n[profile]\ndebounce = 9\n",
    )
    .unwrap();
    let mouse = Mouse::new();
    assert_eq!(
        run_import(
            &device(&mouse),
            Some(dir.path()),
            profile(2),
            &path,
            &confirmed
        ),
        Err(WriteFailure {
            message: format!("{path:?} changed since it was checked; import it again"),
            landed: Vec::new(),
        })
    );
    assert!(mouse.writes().is_empty());
}

// --- the lock ---------------------------------------------------------------

fn is_locked(path: &Path) -> bool {
    let probe = File::options().write(true).open(path).unwrap();
    match probe.try_lock() {
        Ok(()) => false,
        Err(TryLockError::WouldBlock) => true,
        Err(TryLockError::Error(error)) => panic!("{error}"),
    }
}

#[test]
fn the_lock_is_held_for_one_transaction_and_released_after() {
    let dir = state();
    let path = dir.path().join("run").join("gctl.lock");
    let waited = Cell::new(false);
    let held = with_lock(Some(&path), || waited.set(true), || is_locked(&path));
    assert_eq!(held, Ok(true));
    assert!(!is_locked(&path));
    assert!(!waited.get(), "a free lock reports no wait");
}

#[test]
fn no_lock_location_refuses_the_transaction_without_running_it() {
    let ran = Cell::new(false);
    assert_eq!(
        with_lock(None, || {}, || ran.set(true)),
        Err("cannot find a place for the lock file; set XDG_RUNTIME_DIR or HOME".to_owned())
    );
    assert!(!ran.get());
}

#[test]
fn a_held_lock_reports_the_wait_then_runs_once_it_is_free() {
    let dir = state();
    let path = dir.path().join("gctl.lock");
    // another gctl: its own open file description, so its flock excludes ours
    let other = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .unwrap();
    other.lock().unwrap();
    let (waited, wait_seen) = std::sync::mpsc::channel();
    let waiter = {
        let path = path.clone();
        std::thread::spawn(move || with_lock(Some(&path), move || waited.send(()).unwrap(), || 7))
    };
    wait_seen
        .recv_timeout(Duration::from_secs(10))
        .expect("the wait is reported while the other process holds the lock");
    assert!(!waiter.is_finished(), "the transaction waits for the lock");
    drop(other);
    assert_eq!(waiter.join().unwrap(), Ok(7));
}
