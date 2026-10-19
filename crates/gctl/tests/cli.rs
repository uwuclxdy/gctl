#![allow(clippy::unwrap_used, clippy::expect_used)]

//! `gctl::run` end to end against a fake mouse and a temp state dir. Expected
//! reports are the ones `crates/gctl-proto/tests/wire.rs` pins from mxw.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gctl::{Env, HostError, Outcome};
use gctl_proto::{Device, Model, Report, Transport, TransportError};
use tempfile::TempDir;

/// An awake Model O Wireless on active profile 2. Replies are keyed by the
/// `([5], [6])` pair of the last sent report and repeat forever; a pair with no
/// reply (every write, every bind) answers all zeros, which a bind reads as
/// acknowledged.
struct Mouse {
    replies: RefCell<HashMap<(u8, u8), Report>>,
    sent: RefCell<Vec<Report>>,
    fail_writes: Cell<bool>,
}

impl Mouse {
    fn new() -> Self {
        let mouse = Self {
            replies: RefCell::default(),
            sent: RefCell::default(),
            fail_writes: Cell::new(false),
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

    /// Sets the reply to `(sub, command)`; byte 6 echoes the command.
    fn reply(&self, sub: u8, command: u8, bytes: &[(usize, u8)]) {
        let mut report = [0u8; 65];
        report[6] = command;
        for &(i, b) in bytes {
            report[i] = b;
        }
        self.replies.borrow_mut().insert((sub, command), report);
    }

    fn asleep() -> Self {
        let mouse = Self::new();
        mouse.reply(0x00, 0x83, &[(1, 0xA4), (8, 35)]);
        mouse
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
        *report = self.replies.borrow().get(&key).copied().unwrap_or([0; 65]);
        Ok(())
    }
}

struct Out {
    outcome: Outcome,
    stdout: String,
    stderr: String,
    /// How often the run opened the mouse and took the device lock.
    opened: u32,
    locked: u32,
}

/// One `gctl` process: `args` excludes the program name. The lock hook asserts
/// that no report went out before it.
fn gctl_with(
    mouse: Option<&Mouse>,
    pid: u16,
    state: &Path,
    stdin: &str,
    interactive: bool,
    args: &[&str],
) -> Out {
    let model = Model::from_pid(pid).unwrap();
    let mut device = mouse.map(|m| Device::new(m, model).with_io_delay(Duration::ZERO));
    let sent_before = mouse.map_or(0, |m| m.sent().len());
    let opened = Cell::new(0);
    let locked = Cell::new(0);
    let mut open = || {
        opened.set(opened.get() + 1);
        Ok::<_, HostError>(device.take())
    };
    let mut lock = || {
        locked.set(locked.get() + 1);
        let sent_now = mouse.map_or(0, |m| m.sent().len());
        assert_eq!(
            sent_now, sent_before,
            "{args:?}: a report went out before the lock"
        );
        Ok::<_, HostError>(())
    };
    let mut input = stdin.as_bytes();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut env = Env {
        open: &mut open,
        lock: &mut lock,
        state_dir: Some(state),
        stdin: &mut input,
        stdout: &mut stdout,
        stderr: &mut stderr,
        interactive,
    };
    let outcome = gctl::run(
        std::iter::once("gctl").chain(args.iter().copied()),
        &mut env,
    );
    Out {
        outcome,
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
        opened: opened.get(),
        locked: locked.get(),
    }
}

fn gctl(mouse: &Mouse, state: &Path, args: &[&str]) -> Out {
    gctl_with(Some(mouse), 0x2022, state, "", false, args)
}

/// Runs `args` and returns stdout, asserting exit 0 and an empty stderr.
fn ok(mouse: &Mouse, state: &Path, args: &[&str]) -> String {
    let out = gctl(mouse, state, args);
    assert_eq!(out.outcome, Outcome::Exit(0), "{args:?}: {}", out.stderr);
    assert_eq!(out.stderr, "", "{args:?}");
    out.stdout
}

fn state() -> TempDir {
    tempfile::tempdir().unwrap()
}

/// A request report: byte 3 = 0x02 plus the given bytes.
fn req(bytes: &[(usize, u8)]) -> Report {
    let mut r = [0u8; 65];
    r[3] = 0x02;
    for &(i, b) in bytes {
        r[i] = b;
    }
    r
}

fn status_request() -> Report {
    req(&[(4, 0x02), (6, 0x83)])
}

fn bind_req(profile: u8, button: u8, payload: &[(usize, u8)]) -> Report {
    let mut all = vec![(4, 0x09), (5, 0x03), (7, profile), (8, button)];
    all.extend_from_slice(payload);
    req(&all)
}

fn scroll_binds(profile: u8, up_target: u8, down_target: u8) -> [Report; 2] {
    [
        bind_req(profile, 16, &[(10, 1), (11, 1), (12, up_target)]),
        bind_req(profile, 17, &[(10, 1), (11, 1), (12, down_target)]),
    ]
}

// ---------------------------------------------------------------- entry

#[test]
fn no_arguments_opens_the_gui() {
    let dir = state();
    let mouse = Mouse::new();
    let out = gctl_with(Some(&mouse), 0x2022, dir.path(), "", false, &[]);
    assert_eq!(out.outcome, Outcome::Gui);
    assert_eq!((out.stdout.as_str(), out.stderr.as_str()), ("", ""));
    assert_eq!(
        (out.opened, out.locked),
        (0, 0),
        "the GUI opens the mouse itself"
    );
}

#[test]
fn tray_flag_starts_in_the_tray() {
    let dir = state();
    let mouse = Mouse::new();
    let out = gctl_with(Some(&mouse), 0x2022, dir.path(), "", false, &["--tray"]);
    assert_eq!(out.outcome, Outcome::Tray);
    assert_eq!((out.stdout.as_str(), out.stderr.as_str()), ("", ""));
    assert_eq!((out.opened, out.locked), (0, 0));
}

#[test]
fn tray_with_a_command_is_a_usage_error() {
    let dir = state();
    let mouse = Mouse::new();
    let out = gctl(&mouse, dir.path(), &["--tray", "get", "profile"]);
    assert_eq!(out.outcome, Outcome::Exit(2));
    assert_eq!(
        out.stderr,
        "error: --tray cannot be combined with a command\n"
    );
    assert_eq!(out.opened, 0);
}

#[test]
fn no_mouse_is_a_runtime_error() {
    let files = tempfile::tempdir().unwrap();
    let import = import_file(&files, PROFILE_ONLY);
    let export = files.path().join("out.toml");
    for args in [
        &["get", "profile"][..],
        &["set", "polling-rate", "500"][..],
        &["export", export.to_str().unwrap()][..],
        &["import", &import][..],
    ] {
        let dir = state();
        let out = gctl_with(None, 0x2022, dir.path(), "", false, args);
        assert_eq!(out.outcome, Outcome::Exit(1), "{args:?}");
        assert_eq!(out.stdout, "", "{args:?}");
        assert_eq!(out.stderr, "error: no Glorious mouse found\n", "{args:?}");
        assert_eq!(out.locked, 0, "{args:?}");
    }
    assert!(!export.exists());
}

#[test]
fn a_usage_error_wins_over_a_missing_mouse() {
    for args in [
        &["set", "polling-rate", "300"][..],
        &["get", "nope"][..],
        &["set", "dpi", "400", "--rate", "40"][..],
    ] {
        let dir = state();
        let out = gctl_with(None, 0x2022, dir.path(), "", false, args);
        assert_eq!(out.outcome, Outcome::Exit(2), "{args:?}: {}", out.stderr);
        assert_eq!(out.opened, 0, "{args:?} opened the mouse");
    }
}

#[test]
fn help_and_version_print_on_stdout_without_the_mouse() {
    for args in [
        &["--help"][..],
        &["-h"][..],
        &["help"][..],
        &["set", "--help"][..],
        &["help", "import"][..],
    ] {
        let dir = state();
        let mouse = Mouse::new();
        let out = gctl(&mouse, dir.path(), args);
        assert_eq!(out.outcome, Outcome::Exit(0), "{args:?}");
        assert!(
            out.stdout.contains("Usage: gctl"),
            "{args:?}: {}",
            out.stdout
        );
        assert_eq!(out.stderr, "", "{args:?}");
        assert_eq!((out.opened, out.locked), (0, 0), "{args:?}");
        assert!(mouse.sent().is_empty(), "{args:?}");
    }
    let dir = state();
    let help = gctl(&Mouse::new(), dir.path(), &["--help"]);
    assert!(
        help.stdout
            .contains("https://github.com/uwuclxdy/gctl/issues"),
        "help names where to report bugs: {}",
        help.stdout
    );
    let version = gctl(&Mouse::new(), dir.path(), &["--version"]);
    assert_eq!(version.outcome, Outcome::Exit(0));
    assert_eq!(
        version.stdout,
        format!("gctl {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(version.opened, 0);
}

/// Runs `args` with the given hooks and stdout, against an awake Model O.
fn gctl_hooks(
    mouse: &Mouse,
    open_result: Option<HostError>,
    lock_result: Option<HostError>,
    stdout: &mut dyn std::io::Write,
    args: &[&str],
) -> (Outcome, String) {
    let dir = state();
    let model = Model::from_pid(0x2022).unwrap();
    let mut device = Some(Device::new(mouse, model).with_io_delay(Duration::ZERO));
    let mut open_result = open_result;
    let mut lock_result = lock_result;
    let mut open = || match open_result.take() {
        Some(error) => Err(error),
        None => Ok(device.take()),
    };
    let mut lock = || match lock_result.take() {
        Some(error) => Err(error),
        None => Ok(()),
    };
    let mut stderr = Vec::new();
    let mut env = Env {
        open: &mut open,
        lock: &mut lock,
        state_dir: Some(dir.path()),
        stdin: &mut "".as_bytes(),
        stdout,
        stderr: &mut stderr,
        interactive: false,
    };
    let outcome = gctl::run(
        std::iter::once("gctl").chain(args.iter().copied()),
        &mut env,
    );
    (outcome, String::from_utf8(stderr).unwrap())
}

#[test]
fn a_mouse_that_cannot_be_opened_is_a_runtime_error() {
    let mouse = Mouse::new();
    let (outcome, stderr) = gctl_hooks(
        &mouse,
        Some("hidraw node not readable".into()),
        None,
        &mut Vec::new(),
        &["get", "profile"],
    );
    assert_eq!(outcome, Outcome::Exit(1));
    assert_eq!(stderr, "error: hidraw node not readable\n");
    assert!(mouse.sent().is_empty());
}

#[test]
fn a_lock_that_cannot_be_taken_is_a_runtime_error_with_nothing_sent() {
    let mouse = Mouse::new();
    let (outcome, stderr) = gctl_hooks(
        &mouse,
        None,
        Some("lock file not writable".into()),
        &mut Vec::new(),
        &["set", "polling-rate", "500"],
    );
    assert_eq!(outcome, Outcome::Exit(1));
    assert_eq!(stderr, "error: lock file not writable\n");
    assert!(mouse.sent().is_empty());
}

/// A stdout whose reader walked away.
struct ClosedPipe;

impl std::io::Write for ClosedPipe {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }
}

#[test]
fn a_closed_stdout_ends_the_run_quietly() {
    for args in [&["get"][..], &["get", "polling-rate"][..], &["--help"][..]] {
        let mouse = Mouse::new();
        let (outcome, stderr) = gctl_hooks(&mouse, None, None, &mut ClosedPipe, args);
        assert_eq!(outcome, Outcome::Exit(0), "{args:?}");
        assert_eq!(stderr, "", "{args:?}");
    }
}

fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let pairs: Vec<(String, OsString)> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
        .collect();
    move |key| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

/// Environment variables as `(name, value)` pairs.
type Vars<'a> = &'a [(&'a str, &'a str)];

/// Command arguments, without the program name.
type Args<'a> = &'a [&'a str];

#[test]
fn state_dir_follows_xdg_and_ignores_empty_or_relative_values() {
    let cases: [(Vars, Option<&str>); 6] = [
        (
            &[("XDG_STATE_HOME", "/x/state"), ("HOME", "/home/c")],
            Some("/x/state/gctl"),
        ),
        (&[("HOME", "/home/c")], Some("/home/c/.local/state/gctl")),
        (
            &[("XDG_STATE_HOME", ""), ("HOME", "/home/c")],
            Some("/home/c/.local/state/gctl"),
        ),
        (
            &[("XDG_STATE_HOME", "rel"), ("HOME", "/home/c")],
            Some("/home/c/.local/state/gctl"),
        ),
        (&[("HOME", "relative")], None),
        (&[], None),
    ];
    for (env, expected) in cases {
        assert_eq!(
            gctl::state_dir(&vars(env)),
            expected.map(PathBuf::from),
            "{env:?}"
        );
    }
}

#[test]
fn lock_path_follows_xdg_runtime_dir_and_ignores_empty_or_relative_values() {
    let state = Some(Path::new("/home/c/.local/state/gctl"));
    let cases: [(Vars, Option<&Path>, Option<&str>); 6] = [
        (
            &[("XDG_RUNTIME_DIR", "/run/user/1000")],
            state,
            Some("/run/user/1000/gctl.lock"),
        ),
        (
            &[("XDG_RUNTIME_DIR", "/run/user/1000")],
            None,
            Some("/run/user/1000/gctl.lock"),
        ),
        (&[], state, Some("/home/c/.local/state/gctl/gctl.lock")),
        (
            &[("XDG_RUNTIME_DIR", "")],
            state,
            Some("/home/c/.local/state/gctl/gctl.lock"),
        ),
        (
            &[("XDG_RUNTIME_DIR", "run")],
            state,
            Some("/home/c/.local/state/gctl/gctl.lock"),
        ),
        (&[("XDG_RUNTIME_DIR", "run")], None, None),
    ];
    for (env, state_dir, expected) in cases {
        assert_eq!(
            gctl::lock_path(&vars(env), state_dir),
            expected.map(PathBuf::from),
            "{env:?} {state_dir:?}"
        );
    }
}

#[test]
fn take_lock_waits_for_another_holder_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run").join("gctl.lock");
    let mut quiet = Vec::new();
    let first = gctl::take_lock(&path, &mut quiet).unwrap();
    assert_eq!(quiet, b"", "a free lock is taken without a word");

    // the holder lets go only once the waiter has spoken, so the wait is certain
    let (said, heard) = std::sync::mpsc::channel::<Vec<u8>>();
    let released = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let holder = {
        let released = std::sync::Arc::clone(&released);
        std::thread::spawn(move || {
            let first_words = heard.recv_timeout(Duration::from_secs(5)).ok();
            released.store(true, std::sync::atomic::Ordering::SeqCst);
            drop(first);
            (first_words, heard)
        })
    };
    let mut stderr = Speaker(said);
    let second = gctl::take_lock(&path, &mut stderr).unwrap();
    assert!(
        released.load(std::sync::atomic::Ordering::SeqCst),
        "the second lock waited for the first holder"
    );
    drop(stderr);
    let (first_words, heard) = holder.join().unwrap();
    let mut words = first_words.expect("the waiter said nothing before blocking");
    words.extend(heard.iter().flatten());
    assert_eq!(
        String::from_utf8(words).unwrap(),
        "waiting for another gctl to finish\n"
    );
    drop(second);
}

/// A stderr that hands every write to another thread.
struct Speaker(std::sync::mpsc::Sender<Vec<u8>>);

impl std::io::Write for Speaker {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let _ = self.0.send(bytes.to_vec());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn take_lock_errors_name_the_lock_file() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("not-a-dir");
    std::fs::write(&blocker, "").unwrap();
    let path = blocker.join("gctl.lock");
    let error = gctl::take_lock(&path, &mut Vec::new()).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with(&format!("could not lock {path:?}: ")),
        "{error}"
    );
}

/// One `gctl` run with no state dir at all (no usable `HOME` or `XDG_STATE_HOME`).
fn gctl_stateless(mouse: &Mouse, args: &[&str]) -> Out {
    let model = Model::from_pid(0x2022).unwrap();
    let mut device = Some(Device::new(mouse, model).with_io_delay(Duration::ZERO));
    let mut open = || Ok::<_, HostError>(device.take());
    let mut lock = || Ok::<_, HostError>(());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut env = Env {
        open: &mut open,
        lock: &mut lock,
        state_dir: None,
        stdin: &mut "".as_bytes(),
        stdout: &mut stdout,
        stderr: &mut stderr,
        interactive: false,
    };
    let outcome = gctl::run(
        std::iter::once("gctl").chain(args.iter().copied()),
        &mut env,
    );
    Out {
        outcome,
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
        opened: 0,
        locked: 0,
    }
}

#[test]
fn without_a_state_dir_only_memory_commands_fail() {
    for (args, stdout) in [
        (&["--help"][..], None),
        (&["--version"][..], None),
        (&["get", "polling-rate"][..], Some("1000\n")),
        (&["set", "polling-rate", "500"][..], Some("")),
    ] {
        let out = gctl_stateless(&Mouse::new(), args);
        assert_eq!(out.outcome, Outcome::Exit(0), "{args:?}: {}", out.stderr);
        assert_eq!(out.stderr, "", "{args:?}");
        if let Some(stdout) = stdout {
            assert_eq!(out.stdout, stdout, "{args:?}");
        }
    }
    let out = gctl_stateless(&Mouse::new(), &["set", "polling-rate", "300"]);
    assert_eq!(
        out.outcome,
        Outcome::Exit(2),
        "usage errors stay usage errors"
    );
    for args in [
        &["get", "effect"][..],
        &["get"][..],
        &["set", "effect", "wave"][..],
    ] {
        let mouse = Mouse::new();
        let out = gctl_stateless(&mouse, args);
        assert_eq!(out.outcome, Outcome::Exit(1), "{args:?}");
        assert_eq!(
            out.stderr, "error: cannot find a state directory; set XDG_STATE_HOME or HOME\n",
            "{args:?}"
        );
        assert!(mouse.writes().is_empty(), "{args:?} wrote the mouse");
    }
}

#[test]
fn a_missing_state_dir_is_created_on_the_first_save() {
    let dir = state();
    let nested = dir.path().join("gctl");
    let mouse = Mouse::new();
    ok(&mouse, &nested, &["set", "brightness", "200", "50"]);
    assert_eq!(
        ok(&mouse, &nested, &["get", "brightness"]),
        "200 50  (set by gctl)\n"
    );
}

#[test]
fn every_device_command_takes_the_lock_once() {
    let files = tempfile::tempdir().unwrap();
    let import = import_file(&files, PROFILE_ONLY);
    let export = files.path().join("out.toml");
    for args in [
        &["get"][..],
        &["get", "battery"][..],
        &["set", "effect", "wave"][..],
        &["export", export.to_str().unwrap()][..],
        &["import", &import][..],
    ] {
        let dir = state();
        let out = gctl(&Mouse::new(), dir.path(), args);
        assert_eq!(out.outcome, Outcome::Exit(0), "{args:?}: {}", out.stderr);
        assert_eq!((out.opened, out.locked), (1, 1), "{args:?}");
    }
}

// ---------------------------------------------------------------- get

#[test]
fn get_prints_bare_values() {
    let cases = [
        ("profile", "2\n"),
        ("battery", "35\n"),
        ("firmware", "0.3.8.0\n"),
        ("dpi", "400 800 1600\n"),
        ("dpi-stage", "2\n"),
        ("polling-rate", "1000\n"),
        ("lift-off", "1\n"),
        ("debounce", "10\n"),
        ("sleep", "2m\n"),
    ];
    for (setting, expected) in cases {
        let dir = state();
        assert_eq!(
            ok(&Mouse::new(), dir.path(), &["get", setting]),
            expected,
            "{setting}"
        );
    }
}

#[test]
fn get_sleep_never() {
    let dir = state();
    let mouse = Mouse::new();
    mouse.reply(0x00, 0x87, &[(7, 0xFF), (8, 0xFF)]);
    assert_eq!(ok(&mouse, dir.path(), &["get", "sleep"]), "never\n");
}

#[test]
fn get_battery_prints_every_power_state() {
    let cases = [
        ((0xA1, 0), "unknown\n", "awake before the first reading"),
        ((0xA4, 35), "asleep\n", "asleep"),
        ((0xA0, 35), "waking up\n", "waking"),
        ((0x55, 35), "unknown\n", "unknown state"),
    ];
    for ((state_byte, percent), expected, why) in cases {
        let dir = state();
        let mouse = Mouse::new();
        mouse.reply(0x00, 0x83, &[(1, state_byte), (8, percent)]);
        assert_eq!(
            ok(&mouse, dir.path(), &["get", "battery"]),
            expected,
            "{why}"
        );
    }
}

#[test]
fn get_without_a_setting_lists_everything() {
    let dir = state();
    let expected = "\
profile  2
battery  35
firmware  0.3.8.0
dpi  400 800 1600
dpi-stage  2
dpi-colors  unknown
polling-rate  1000
lift-off  1
debounce  10
sleep  2m
effect  unknown
brightness  unknown
bind.left  unknown
bind.right  unknown
bind.scroll  unknown
bind.back  unknown
bind.forward  unknown
bind.scroll-up  unknown
bind.scroll-down  unknown
bind.dpi  unknown
scroll  unknown
";
    assert_eq!(ok(&Mouse::new(), dir.path(), &["get"]), expected);
}

#[test]
fn get_listing_marks_a_remembered_bind() {
    let dir = state();
    let mouse = Mouse::new();
    ok(
        &mouse,
        dir.path(),
        &["set", "bind", "back", "key:ctrl+KeyA"],
    );
    let listing = ok(&mouse, dir.path(), &["get"]);
    assert!(
        listing
            .lines()
            .any(|l| l == "bind.back  key:ctrl+KeyA  (set by gctl)"),
        "{listing}"
    );
}

#[test]
fn profile_flag_skips_the_active_profile_read() {
    let dir = state();
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["get", "debounce", "--profile", "3"]);
    let sent = mouse.sent();
    assert!(sent.iter().all(|r| r[6] != 0x85), "no active-profile read");
    assert!(sent.contains(&req(&[(4, 0x02), (6, 0x88), (7, 3)])));
}

#[test]
fn without_a_profile_flag_the_active_profile_is_read() {
    let dir = state();
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["get", "debounce"]);
    let sent = mouse.sent();
    assert!(sent.contains(&req(&[(4, 0x01), (6, 0x85)])));
    assert!(sent.contains(&req(&[(4, 0x02), (6, 0x88), (7, 2)])));
}

#[test]
fn an_untested_model_prints_a_note_for_every_device_command() {
    let files = tempfile::tempdir().unwrap();
    let import = import_file(
        &files,
        "format = 1\nmodel = \"Model D Wireless\"\n[profile]\ndebounce = 4\n",
    );
    let export = files.path().join("out.toml");
    for (args, stdout) in [
        (&["get", "polling-rate"][..], "1000\n"),
        (&["set", "polling-rate", "500"][..], ""),
        (&["export", export.to_str().unwrap()][..], ""),
        (&["import", &import][..], ""),
    ] {
        let dir = state();
        let out = gctl_with(Some(&Mouse::new()), 0x2023, dir.path(), "", false, args);
        assert_eq!(out.outcome, Outcome::Exit(0), "{args:?}: {}", out.stderr);
        assert_eq!(out.stdout, stdout, "{args:?}");
        assert_eq!(
            out.stderr, "note: Model D Wireless is untested with gctl\n",
            "{args:?}"
        );
    }
}

#[test]
fn a_usage_error_on_an_untested_model_prints_no_note() {
    for args in [
        &["set", "polling-rate", "300"][..],
        &["get", "nope"][..],
        &["set", "sleep", "2m", "--profile", "1"][..],
    ] {
        let dir = state();
        let out = gctl_with(Some(&Mouse::new()), 0x2023, dir.path(), "", false, args);
        assert_eq!(out.outcome, Outcome::Exit(2), "{args:?}");
        assert!(!out.stderr.contains("note:"), "{args:?}: {}", out.stderr);
        assert_eq!(out.stderr.lines().count(), 1, "{args:?}: {}", out.stderr);
    }
}

#[test]
fn the_full_listing_reads_the_active_profile_once() {
    let dir = state();
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["get"]);
    let reads = mouse.sent().iter().filter(|r| r[6] == 0x85).count();
    assert_eq!(reads, 1);
}

// ---------------------------------------------------------------- refusals

#[test]
fn an_asleep_mouse_is_refused_with_nothing_read_or_written() {
    let files = tempfile::tempdir().unwrap();
    let import = import_file(&files, PROFILE_ONLY);
    let export = files.path().join("out.toml");
    for args in [
        &["set", "polling-rate", "500"][..],
        &["set", "effect", "wave"][..],
        &["get", "polling-rate"][..],
        &["get"][..],
        &["export", export.to_str().unwrap()][..],
        &["import", &import][..],
    ] {
        let dir = state();
        let mouse = Mouse::asleep();
        let out = gctl(&mouse, dir.path(), args);
        assert_eq!(out.outcome, Outcome::Exit(1), "{args:?}");
        assert_eq!(out.stdout, "", "{args:?}");
        assert_eq!(out.stderr, "error: device is asleep\n", "{args:?}");
        assert_eq!(mouse.sent(), vec![status_request()], "{args:?}");
    }
    assert!(!export.exists(), "no export of an asleep mouse");
}

#[test]
fn bad_values_are_usage_errors_that_touch_nothing() {
    let cases: [&[&str]; 28] = [
        &["set", "polling-rate", "300"],
        &["set", "dpi"],
        &["set", "dpi", "400", "--rate", "40"],
        &["set", "bind", "back", "none", "--rate", "40"],
        &["set", "dpi-colors"],
        &[
            "set",
            "dpi-colors",
            "ff0000",
            "ff0000",
            "ff0000",
            "ff0000",
            "ff0000",
            "ff0000",
            "ff0000",
        ],
        &["set", "dpi-colors", "zzzzzz"],
        &["set", "sleep", "+5s"],
        &["set", "effect", "wave", "--rate", "+40"],
        &["get", "bind"],
        &["set", "dpi", "99"],
        &["set", "dpi", "19001"],
        &[
            "set", "dpi", "400", "400", "400", "400", "400", "400", "400",
        ],
        &["set", "dpi-stage", "7"],
        &["set", "debounce", "17"],
        &["set", "sleep", "0s"],
        &["set", "brightness", "256", "0"],
        &["set", "brightness", "200 50"],
        &["set", "brightness", "200 50", ""],
        &["set", "brightness", "", "200 50"],
        &["set", "lift-off", "3"],
        &["set", "profile", "4"],
        &["set", "effect", "solid", "00ff00", "--rate", "40"],
        &["set", "effect", "rainbow"],
        &["set", "bind", "back", "key:Foo"],
        &["set", "scroll", "sideways"],
        &["get", "nope"],
        &["get", "debounce", "--profile", "0"],
    ];
    for args in cases {
        let dir = state();
        let mouse = Mouse::new();
        let out = gctl(&mouse, dir.path(), args);
        assert_eq!(out.outcome, Outcome::Exit(2), "{args:?}");
        assert_eq!(out.stdout, "", "{args:?}");
        assert!(out.stderr.starts_with("error:"), "{args:?}: {}", out.stderr);
        assert!(mouse.sent().is_empty(), "{args:?} reached the mouse");
        assert_eq!(out.opened, 0, "{args:?} opened the mouse");
    }
}

#[test]
fn profile_flag_on_a_device_wide_setting_is_a_usage_error() {
    let cases: [&[&str]; 14] = [
        &["set", "profile", "2"],
        &["set", "polling-rate", "500"],
        &["set", "lift-off", "1"],
        &["set", "sleep", "2m"],
        &["set", "brightness", "1", "2"],
        &["set", "scroll", "invert"],
        &["get", "profile"],
        &["get", "battery"],
        &["get", "firmware"],
        &["get", "polling-rate"],
        &["get", "lift-off"],
        &["get", "sleep"],
        &["get", "brightness"],
        &["get", "scroll"],
    ];
    for args in cases {
        let dir = state();
        let mouse = Mouse::new();
        let mut with_flag = args.to_vec();
        with_flag.extend(["--profile", "3"]);
        let out = gctl(&mouse, dir.path(), &with_flag);
        assert_eq!(out.outcome, Outcome::Exit(2), "{with_flag:?}");
        assert_eq!(
            out.stderr,
            format!(
                "error: {} is device-wide; --profile does not apply\n",
                args[1]
            ),
            "{with_flag:?}"
        );
        assert!(mouse.sent().is_empty(), "{with_flag:?}");
    }
}

#[test]
fn an_implausible_device_value_is_a_runtime_error() {
    let dir = state();
    let mouse = Mouse::new();
    mouse.reply(0x00, 0x85, &[(7, 0)]);
    let out = gctl(&mouse, dir.path(), &["get", "profile"]);
    assert_eq!(
        out.outcome,
        Outcome::Exit(1),
        "the device's fault, not a usage error"
    );
    assert_eq!(out.stdout, "");
    assert_eq!(
        out.stderr,
        "error: device reported an implausible profile (0)\n"
    );
}

#[test]
fn dpi_colours_must_match_the_stage_count() {
    let dir = state();
    let mouse = Mouse::new();
    let out = gctl(
        &mouse,
        dir.path(),
        &["set", "dpi-colors", "ffff00", "0000ff"],
    );
    assert_eq!(out.outcome, Outcome::Exit(1));
    assert!(out.stderr.starts_with("error:"), "{}", out.stderr);
    assert!(mouse.writes().is_empty());
    assert_eq!(ok(&mouse, dir.path(), &["get", "dpi-colors"]), "unknown\n");
}

// ---------------------------------------------------------------- set

#[test]
fn set_writes_the_expected_reports() {
    let cases: [(&[&str], Vec<Report>); 9] = [
        (
            &["set", "profile", "3"],
            vec![req(&[(4, 0x01), (6, 0x05), (7, 3)])],
        ),
        (
            &["set", "polling-rate", "500"],
            vec![req(&[(4, 0x01), (5, 0x01), (7, 2)])],
        ),
        (
            &["set", "lift-off", "2"],
            vec![req(&[(4, 0x01), (5, 0x01), (6, 0x07), (7, 1)])],
        ),
        (
            &["set", "sleep", "1m30s"],
            vec![req(&[(4, 0x02), (6, 0x07), (7, 0x00), (8, 90)])],
        ),
        (
            &["set", "debounce", "0"],
            vec![req(&[(4, 0x01), (6, 0x08), (7, 2), (8, 0)])],
        ),
        (
            &["set", "dpi", "400", "800"],
            vec![
                req(&[(4, 0x02), (5, 0x01), (6, 0x02), (7, 2), (8, 1)]),
                req(&[
                    (4, 10),
                    (5, 0x01),
                    (6, 0x01),
                    (7, 2),
                    (8, 2),
                    (9, 0x01),
                    (10, 0x90),
                    (11, 0x01),
                    (12, 0x90),
                    (13, 0x03),
                    (14, 0x20),
                    (15, 0x03),
                    (16, 0x20),
                ]),
            ],
        ),
        (
            &["set", "brightness", "200", "50"],
            vec![
                req(&[(4, 0x02), (5, 0x02), (6, 0x02), (7, 0x01), (8, 200)]),
                req(&[(4, 0x02), (5, 0x02), (6, 0x02), (7, 0x00), (8, 50)]),
            ],
        ),
        (
            &["set", "effect", "wave", "--rate", "40"],
            vec![req(&[
                (4, 5),
                (5, 0x02),
                (7, 2),
                (8, 0xFF),
                (9, 8),
                (11, 130),
            ])],
        ),
        (
            &["set", "bind", "back", "key:ctrl+KeyA"],
            vec![bind_req(2, 4, &[(10, 4), (11, 2), (12, 1), (13, 4)])],
        ),
    ];
    for (args, expected) in cases {
        let dir = state();
        let mouse = Mouse::new();
        assert_eq!(ok(&mouse, dir.path(), args), "", "{args:?} prints nothing");
        assert_eq!(mouse.writes(), expected, "{args:?}");
    }
}

#[test]
fn set_dpi_stage_writes_the_active_stage() {
    let dir = state();
    let mouse = Mouse::new();
    ok(
        &mouse,
        dir.path(),
        &["set", "dpi-stage", "3", "--profile", "1"],
    );
    assert_eq!(
        mouse.writes(),
        vec![req(&[(4, 0x02), (5, 0x01), (6, 0x02), (7, 1), (8, 3)])]
    );
}

#[test]
fn set_scroll_rebinds_every_profile() {
    let dir = state();
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["set", "scroll", "invert"]);
    let expected: Vec<Report> = [1, 2, 3]
        .into_iter()
        .flat_map(|p| scroll_binds(p, 17, 16))
        .collect();
    assert_eq!(mouse.writes(), expected);
}

// ---------------------------------------------------------------- memory

#[test]
fn write_only_settings_read_unknown_before_any_write() {
    for args in [
        &["get", "effect"][..],
        &["get", "brightness"][..],
        &["get", "dpi-colors"][..],
        &["get", "scroll"][..],
        &["get", "bind", "back"][..],
    ] {
        let dir = state();
        assert_eq!(ok(&Mouse::new(), dir.path(), args), "unknown\n", "{args:?}");
    }
}

#[test]
fn memory_persists_across_runs() {
    let dir = state();
    let mouse = Mouse::new();
    ok(
        &mouse,
        dir.path(),
        &["set", "effect", "wave", "--rate", "40"],
    );
    ok(&mouse, dir.path(), &["set", "brightness", "200", "50"]);
    ok(
        &mouse,
        dir.path(),
        &["set", "dpi-colors", "ffff00", "0000ff", "ff0000"],
    );
    ok(
        &mouse,
        dir.path(),
        &["set", "bind", "back", "key:ctrl+KeyA"],
    );
    assert_eq!(
        ok(&mouse, dir.path(), &["get", "effect"]),
        "wave rate=40  (set by gctl)\n"
    );
    assert_eq!(
        ok(&mouse, dir.path(), &["get", "brightness"]),
        "200 50  (set by gctl)\n"
    );
    assert_eq!(
        ok(&mouse, dir.path(), &["get", "dpi-colors"]),
        "ffff00 0000ff ff0000  (set by gctl)\n"
    );
    assert_eq!(
        ok(&mouse, dir.path(), &["get", "bind", "back"]),
        "key:ctrl+KeyA  (set by gctl)\n"
    );
}

#[test]
fn memory_is_per_profile() {
    let dir = state();
    let mouse = Mouse::new();
    ok(
        &mouse,
        dir.path(),
        &["set", "effect", "solid", "00ff00", "--profile", "1"],
    );
    assert_eq!(ok(&mouse, dir.path(), &["get", "effect"]), "unknown\n");
    assert_eq!(
        ok(&mouse, dir.path(), &["get", "effect", "--profile", "1"]),
        "solid 00ff00  (set by gctl)\n"
    );
}

#[test]
fn memory_is_keyed_by_model_name() {
    let dir = state();
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["set", "brightness", "200", "50"]);
    let cable = gctl_with(
        Some(&mouse),
        0x2011,
        dir.path(),
        "",
        false,
        &["get", "brightness"],
    );
    assert_eq!(
        cable.stdout, "200 50  (set by gctl)\n",
        "same model, cable PID"
    );
    let other = gctl_with(
        Some(&mouse),
        0x2023,
        dir.path(),
        "",
        false,
        &["get", "brightness"],
    );
    assert_eq!(other.stdout, "unknown\n", "another model");
}

#[test]
fn a_failed_write_is_not_remembered() {
    let dir = state();
    let mouse = Mouse::new();
    mouse.fail_writes.set(true);
    let out = gctl(&mouse, dir.path(), &["set", "effect", "wave"]);
    assert_eq!(out.outcome, Outcome::Exit(1));
    assert_eq!(
        out.stderr, "error: HID transport failed: write refused by the fake\n",
        "the error and its source chain, joined by `: `"
    );
    mouse.fail_writes.set(false);
    assert_eq!(ok(&mouse, dir.path(), &["get", "effect"]), "unknown\n");
}

#[test]
fn a_failed_write_clears_the_remembered_value() {
    let dir = state();
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["set", "brightness", "200", "50"]);
    mouse.fail_writes.set(true);
    let out = gctl(&mouse, dir.path(), &["set", "brightness", "10", "10"]);
    assert_eq!(out.outcome, Outcome::Exit(1));
    mouse.fail_writes.set(false);
    assert_eq!(
        ok(&mouse, dir.path(), &["get", "brightness"]),
        "unknown\n",
        "the mouse may hold part of the new value"
    );
}

#[test]
fn a_failed_write_clears_each_remembered_setting() {
    // (the write that gets remembered, the write that fails, the reads that must turn unknown)
    let cases: [(Args, Args, &[Args]); 6] = [
        (
            &["set", "effect", "solid", "00ff00"],
            &["set", "effect", "wave"],
            &[&["get", "effect"]],
        ),
        (
            &["set", "dpi-colors", "ffff00", "0000ff", "ff0000"],
            &["set", "dpi-colors", "ff0000", "ff0000", "ff0000"],
            &[&["get", "dpi-colors"]],
        ),
        (
            &["set", "dpi-colors", "ffff00", "0000ff", "ff0000"],
            &["set", "dpi", "400", "800"],
            &[&["get", "dpi-colors"]],
        ),
        (
            &["set", "bind", "back", "key:ctrl+KeyA"],
            &["set", "bind", "back", "mouse:forward"],
            &[&["get", "bind", "back"]],
        ),
        (
            &["set", "scroll", "invert"],
            &["set", "scroll", "default"],
            &[
                &["get", "scroll"],
                &["get", "bind", "scroll-up", "--profile", "1"],
                &["get", "bind", "scroll-down", "--profile", "3"],
            ],
        ),
        (
            &["set", "scroll", "invert"],
            &[
                "set",
                "bind",
                "scroll-up",
                "mouse:scroll-down",
                "--profile",
                "1",
            ],
            &[
                &["get", "scroll"],
                &["get", "bind", "scroll-up", "--profile", "1"],
            ],
        ),
    ];
    for (remembered, failing, reads) in cases {
        let dir = state();
        let mouse = Mouse::new();
        ok(&mouse, dir.path(), remembered);
        mouse.fail_writes.set(true);
        let out = gctl(&mouse, dir.path(), failing);
        assert_eq!(out.outcome, Outcome::Exit(1), "{failing:?}");
        assert_eq!(
            out.stderr, "error: HID transport failed: write refused by the fake\n",
            "{failing:?}"
        );
        mouse.fail_writes.set(false);
        for read in reads {
            assert_eq!(
                ok(&mouse, dir.path(), read),
                "unknown\n",
                "{read:?} after a failed {failing:?}"
            );
        }
    }
}

fn memory_file(dir: &TempDir) -> PathBuf {
    dir.path().join("memory.toml")
}

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn a_clear_that_cannot_be_saved_is_reported() {
    let dir = state();
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["set", "brightness", "200", "50"]);
    set_mode(dir.path(), 0o555);
    mouse.fail_writes.set(true);
    let out = gctl(&mouse, dir.path(), &["set", "brightness", "10", "10"]);
    set_mode(dir.path(), 0o755);
    assert_eq!(out.outcome, Outcome::Exit(1));
    let lines: Vec<&str> = out.stderr.lines().collect();
    assert_eq!(lines.len(), 2, "{}", out.stderr);
    assert_eq!(
        lines[0],
        "error: HID transport failed: write refused by the fake"
    );
    assert!(lines[1].starts_with("error: "), "{}", out.stderr);
    assert!(
        lines[1].contains(&format!("{:?}", memory_file(&dir))),
        "names the memory file: {}",
        out.stderr
    );
}

#[test]
fn a_memory_save_failing_after_the_write_names_the_file() {
    let dir = state();
    let mouse = Mouse::new();
    set_mode(dir.path(), 0o555);
    let out = gctl(&mouse, dir.path(), &["set", "brightness", "200", "50"]);
    set_mode(dir.path(), 0o755);
    assert_eq!(out.outcome, Outcome::Exit(1));
    assert_eq!(mouse.writes().len(), 2, "the mouse was written");
    assert_eq!(
        out.stderr,
        format!(
            "error: could not save remembered values to {:?}: Permission denied (os error 13) (the mouse was written)\n",
            memory_file(&dir)
        )
    );
}

#[test]
fn every_remembered_value_is_checked_on_load() {
    // (what the corruption is, the text it replaces, its replacement)
    let corruptions = [
        ("bad brightness", "\"200 50\"", "\"200 fifty\""),
        ("bad colour", "ffff00 0000ff ff0000", "ffff00 0000ff zzzzzz"),
        ("bad bind button", "back = ", "middle = "),
        ("bad binding", "key:ctrl+KeyA", "key:Foo"),
        ("a misspelled key", "brightness = ", "brightnes = "),
        (
            "a stale key",
            "brightness = ",
            "scroll = \"invert\"\nbrightness = ",
        ),
        (
            "a profile key with a leading zero",
            "profile.2",
            "profile.02",
        ),
        ("a profile past 3", "profile.2", "profile.9"),
    ];
    for (why, from, to) in corruptions {
        let dir = state();
        let mouse = Mouse::new();
        ok(&mouse, dir.path(), &["set", "brightness", "200", "50"]);
        ok(
            &mouse,
            dir.path(),
            &["set", "dpi-colors", "ffff00", "0000ff", "ff0000"],
        );
        ok(
            &mouse,
            dir.path(),
            &["set", "bind", "back", "key:ctrl+KeyA"],
        );
        let path = memory_file(&dir);
        let good = std::fs::read_to_string(&path).unwrap();
        assert!(good.contains(from), "{why}: {from:?} not in {good}");
        let bad = good.replacen(from, to, 1);
        std::fs::write(&path, &bad).unwrap();
        for args in [&["get"][..], &["set", "effect", "off"][..]] {
            let before = mouse.writes().len();
            let out = gctl(&mouse, dir.path(), args);
            assert_eq!(out.outcome, Outcome::Exit(1), "{why}: {args:?}");
            assert!(
                out.stderr.starts_with(&format!(
                    "error: cannot read remembered values from {path:?}: "
                )),
                "{why}: {args:?}: {}",
                out.stderr
            );
            assert_eq!(
                mouse.writes().len(),
                before,
                "{why}: {args:?} wrote the mouse"
            );
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            bad,
            "{why}: file kept"
        );
    }
}

#[test]
fn a_memory_file_that_does_not_parse_is_refused_and_kept() {
    // each corruption is applied to a memory holding `wave rate=40` for profile 2
    type Corrupt = fn(&str) -> String;
    let corruptions: [(&str, Corrupt); 4] = [
        ("not toml", |_| "this is [ not toml".to_owned()),
        ("truncated", |text| text[..text.len() / 2].to_owned()),
        ("a value outside the grammar", |text| {
            text.replace("wave rate=40", "rainbow")
        }),
        ("a value smuggling a newline", |text| {
            text.replace("wave rate=40", "wave rate=40\\nprofile  3")
        }),
    ];
    for (why, corrupt) in corruptions {
        let dir = state();
        let mouse = Mouse::new();
        ok(&mouse, dir.path(), &["set", "effect", "wave"]);
        let path = memory_file(&dir);
        let good = std::fs::read_to_string(&path).unwrap();
        assert!(good.contains("wave rate=40"), "{good}");
        let bad = corrupt(&good);
        std::fs::write(&path, &bad).unwrap();

        let files = tempfile::tempdir().unwrap();
        let export = files.path().join("out.toml");
        for args in [
            &["get", "effect"][..],
            &["get"][..],
            &["set", "effect", "off"][..],
            &["set", "bind", "back", "none"][..],
            &["export", export.to_str().unwrap()][..],
        ] {
            let before = mouse.writes().len();
            let out = gctl(&mouse, dir.path(), args);
            assert_eq!(out.outcome, Outcome::Exit(1), "{why}: {args:?}");
            assert_eq!(out.stdout, "", "{why}: {args:?}");
            assert!(
                out.stderr.starts_with(&format!(
                    "error: cannot read remembered values from {path:?}: "
                )),
                "{why}: {args:?}: {}",
                out.stderr
            );
            assert!(
                out.stderr.ends_with("; fix or delete the file\n"),
                "{why}: {args:?}: {}",
                out.stderr
            );
            assert_eq!(
                out.stderr.lines().count(),
                1,
                "{why}: {args:?}: {}",
                out.stderr
            );
            assert_eq!(
                mouse.writes().len(),
                before,
                "{why}: {args:?} wrote the mouse"
            );
        }
        assert!(!export.exists(), "{why}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            bad,
            "{why}: file kept"
        );
        assert_eq!(
            ok(&mouse, dir.path(), &["get", "polling-rate"]),
            "1000\n",
            "{why}: device-only reads need no memory"
        );
    }
}

#[test]
fn an_unreadable_memory_file_is_refused() {
    let dir = state();
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["set", "effect", "wave"]);
    let path = memory_file(&dir);
    set_mode(&path, 0o000);
    let out = gctl(&mouse, dir.path(), &["get", "effect"]);
    set_mode(&path, 0o644);
    assert_eq!(out.outcome, Outcome::Exit(1));
    assert!(
        out.stderr.starts_with(&format!(
            "error: cannot read remembered values from {path:?}: "
        )),
        "{}",
        out.stderr
    );
}

#[test]
fn rebinding_a_scroll_button_to_what_the_direction_implies_keeps_it() {
    let dir = state();
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["set", "scroll", "invert"]);
    ok(
        &mouse,
        dir.path(),
        &[
            "set",
            "bind",
            "scroll-up",
            "mouse:scroll-down",
            "--profile",
            "2",
        ],
    );
    assert_eq!(
        ok(&mouse, dir.path(), &["get", "scroll"]),
        "invert  (set by gctl)\n"
    );
}

#[test]
fn scroll_reads_back_from_six_matching_binds() {
    let dir = state();
    let mouse = Mouse::new();
    for profile in ["1", "2", "3"] {
        ok(
            &mouse,
            dir.path(),
            &[
                "set",
                "bind",
                "scroll-up",
                "mouse:scroll-down",
                "--profile",
                profile,
            ],
        );
        assert_eq!(
            ok(&mouse, dir.path(), &["get", "scroll"]),
            "unknown\n",
            "scroll-down still unknown on profile {profile}"
        );
        ok(
            &mouse,
            dir.path(),
            &[
                "set",
                "bind",
                "scroll-down",
                "mouse:scroll-up",
                "--profile",
                profile,
            ],
        );
    }
    assert_eq!(
        ok(&mouse, dir.path(), &["get", "scroll"]),
        "invert  (set by gctl)\n"
    );
}

#[test]
fn scroll_records_both_scroll_binds_of_every_profile() {
    let dir = state();
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["set", "scroll", "invert"]);
    assert_eq!(
        ok(&mouse, dir.path(), &["get", "scroll"]),
        "invert  (set by gctl)\n"
    );
    assert_eq!(
        ok(
            &mouse,
            dir.path(),
            &["get", "bind", "scroll-up", "--profile", "1"]
        ),
        "mouse:scroll-down  (set by gctl)\n"
    );
    assert_eq!(
        ok(
            &mouse,
            dir.path(),
            &["get", "bind", "scroll-down", "--profile", "3"]
        ),
        "mouse:scroll-up  (set by gctl)\n"
    );
}

#[test]
fn a_scroll_bind_clears_the_scroll_memory() {
    let dir = state();
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["set", "scroll", "invert"]);
    ok(
        &mouse,
        dir.path(),
        &[
            "set",
            "bind",
            "scroll-up",
            "mouse:scroll-up",
            "--profile",
            "1",
        ],
    );
    assert_eq!(ok(&mouse, dir.path(), &["get", "scroll"]), "unknown\n");
    assert_eq!(
        ok(
            &mouse,
            dir.path(),
            &["get", "bind", "scroll-up", "--profile", "1"]
        ),
        "mouse:scroll-up  (set by gctl)\n"
    );
    assert_eq!(
        ok(
            &mouse,
            dir.path(),
            &["get", "bind", "scroll-down", "--profile", "3"]
        ),
        "mouse:scroll-up  (set by gctl)\n",
        "the other scroll binds stay remembered"
    );
}

#[test]
fn a_new_stage_count_drops_that_profiles_dpi_colours() {
    let dir = state();
    let mouse = Mouse::new();
    ok(
        &mouse,
        dir.path(),
        &[
            "set",
            "dpi-colors",
            "ffff00",
            "0000ff",
            "ff0000",
            "--profile",
            "1",
        ],
    );
    ok(
        &mouse,
        dir.path(),
        &["set", "dpi-colors", "ffff00", "0000ff", "ff0000"],
    );
    ok(&mouse, dir.path(), &["set", "dpi", "400", "800"]);
    assert_eq!(ok(&mouse, dir.path(), &["get", "dpi-colors"]), "unknown\n");
    assert_eq!(
        ok(&mouse, dir.path(), &["get", "dpi-colors", "--profile", "1"]),
        "ffff00 0000ff ff0000  (set by gctl)\n",
        "another profile keeps its colours"
    );
}

#[test]
fn the_same_stage_count_keeps_the_dpi_colours() {
    let dir = state();
    let mouse = Mouse::new();
    ok(
        &mouse,
        dir.path(),
        &["set", "dpi-colors", "ffff00", "0000ff", "ff0000"],
    );
    ok(&mouse, dir.path(), &["set", "dpi", "800", "1600", "3200"]);
    assert_eq!(
        ok(&mouse, dir.path(), &["get", "dpi-colors"]),
        "ffff00 0000ff ff0000  (set by gctl)\n"
    );
}

// ---------------------------------------------------------------- export

#[test]
fn export_writes_the_profile_and_device_settings() {
    let dir = state();
    let out_dir = tempfile::tempdir().unwrap();
    let path = out_dir.path().join("p.toml");
    let mouse = Mouse::new();
    ok(
        &mouse,
        dir.path(),
        &["set", "effect", "wave", "--rate", "40"],
    );
    ok(
        &mouse,
        dir.path(),
        &["set", "bind", "back", "key:ctrl+KeyA"],
    );
    ok(&mouse, dir.path(), &["set", "brightness", "200", "50"]);
    ok(&mouse, dir.path(), &["export", path.to_str().unwrap()]);

    let written: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let expected: toml::Table = r#"
        format = 1
        model = "Model O Wireless"

        [device]
        polling_rate = 1000
        lift_off = 1
        sleep = "2m"
        brightness = { wired = 200, wireless = 50 }

        [profile]
        dpi = [400, 800, 1600]
        dpi_stage = 2
        debounce = 10
        effect = "wave rate=40"

        [profile.binds]
        back = "key:ctrl+KeyA"
    "#
    .parse()
    .unwrap();
    assert_eq!(written, expected);
    assert_eq!(
        std::fs::read_dir(out_dir.path()).unwrap().count(),
        1,
        "no temp file left beside the export"
    );
}

#[test]
fn export_refuses_an_existing_path() {
    let dir = state();
    let out_dir = tempfile::tempdir().unwrap();
    let path = out_dir.path().join("p.toml");
    std::fs::write(&path, "keep").unwrap();
    let out = gctl(
        &Mouse::new(),
        dir.path(),
        &["export", path.to_str().unwrap()],
    );
    assert_eq!(out.outcome, Outcome::Exit(1));
    assert_eq!(out.stderr, format!("error: {path:?} exists\n"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep");
    assert_eq!((out.opened, out.locked), (0, 0), "checked before the mouse");
}

#[test]
fn export_into_a_missing_directory_fails_before_the_mouse() {
    let dir = state();
    let out_dir = tempfile::tempdir().unwrap();
    let path = out_dir.path().join("no").join("p.toml");
    let out = gctl(
        &Mouse::new(),
        dir.path(),
        &["export", path.to_str().unwrap()],
    );
    assert_eq!(out.outcome, Outcome::Exit(1));
    let parent = out_dir.path().join("no");
    assert_eq!(
        out.stderr,
        format!("error: could not write {path:?}: {parent:?} does not exist\n")
    );
    assert_eq!((out.opened, out.locked), (0, 0));

    std::fs::write(&parent, "").unwrap();
    let out = gctl(
        &Mouse::new(),
        dir.path(),
        &["export", path.to_str().unwrap()],
    );
    assert_eq!(
        out.stderr,
        format!("error: could not write {path:?}: {parent:?} is not a directory\n")
    );
    assert_eq!((out.opened, out.locked), (0, 0));

    // a parent that cannot be looked at gives the real cause, not "does not exist"
    let locked = out_dir.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    let hidden = locked.join("sub");
    let path = hidden.join("p.toml");
    set_mode(&locked, 0o000);
    let out = gctl(
        &Mouse::new(),
        dir.path(),
        &["export", path.to_str().unwrap()],
    );
    set_mode(&locked, 0o755);
    assert_eq!(
        out.stderr,
        format!("error: could not write {path:?}: {hidden:?}: Permission denied (os error 13)\n")
    );
    assert_eq!((out.opened, out.locked), (0, 0));
}

#[test]
fn export_to_a_maximal_file_name_works() {
    let dir = state();
    let out_dir = tempfile::tempdir().unwrap();
    let name = format!("{}.toml", "a".repeat(250));
    let path = out_dir.path().join(&name);
    ok(
        &Mouse::new(),
        dir.path(),
        &["export", path.to_str().unwrap()],
    );
    assert!(path.exists());
    assert_eq!(std::fs::read_dir(out_dir.path()).unwrap().count(), 1);
}

#[test]
fn export_never_follows_or_deletes_a_file_planted_at_its_temp_name() {
    // the temp file is `.gctl-export.<pid>.tmp` beside the target; `run` runs in this process
    let dir = state();
    let out_dir = tempfile::tempdir().unwrap();
    let path = out_dir.path().join("p.toml");
    let temp = out_dir
        .path()
        .join(format!(".gctl-export.{}.tmp", std::process::id()));
    let victim = out_dir.path().join("victim.txt");
    std::fs::write(&victim, "precious").unwrap();
    std::os::unix::fs::symlink(&victim, &temp).unwrap();

    let out = gctl(
        &Mouse::new(),
        dir.path(),
        &["export", path.to_str().unwrap()],
    );
    assert_eq!(out.outcome, Outcome::Exit(1));
    assert!(
        out.stderr
            .starts_with(&format!("error: could not write {path:?}: ")),
        "{}",
        out.stderr
    );
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "precious");
    assert!(
        temp.symlink_metadata().is_ok(),
        "a file this run did not create stays"
    );
    assert!(path.symlink_metadata().is_err(), "no export written");
}

#[test]
fn export_refuses_a_dangling_symlink_and_a_path_with_a_newline() {
    let out_dir = tempfile::tempdir().unwrap();
    let link = out_dir.path().join("link.toml");
    std::os::unix::fs::symlink(out_dir.path().join("missing"), &link).unwrap();
    let dir = state();
    let out = gctl(
        &Mouse::new(),
        dir.path(),
        &["export", link.to_str().unwrap()],
    );
    assert_eq!(out.outcome, Outcome::Exit(1));
    assert_eq!(out.stderr, format!("error: {link:?} exists\n"));
    assert_eq!((out.opened, out.locked), (0, 0), "checked before the mouse");
    assert!(
        !out_dir.path().join("missing").exists(),
        "nothing written through the link"
    );

    let forged = out_dir.path().join("a\nerror: forged");
    std::fs::write(&forged, "keep").unwrap();
    let out = gctl(
        &Mouse::new(),
        dir.path(),
        &["export", forged.to_str().unwrap()],
    );
    assert_eq!(out.stderr.lines().count(), 1, "{}", out.stderr);
    assert!(out.stderr.contains("a\\nerror: forged"), "{}", out.stderr);
}

#[test]
fn export_of_another_profile_reads_and_remembers_that_profile() {
    let dir = state();
    let out_dir = tempfile::tempdir().unwrap();
    let path = out_dir.path().join("p.toml");
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["set", "effect", "wave"]);
    ok(
        &mouse,
        dir.path(),
        &["set", "effect", "solid", "00ff00", "--profile", "1"],
    );
    ok(
        &mouse,
        dir.path(),
        &[
            "set",
            "dpi-colors",
            "ffff00",
            "0000ff",
            "ff0000",
            "--profile",
            "1",
        ],
    );
    ok(&mouse, dir.path(), &["set", "scroll", "invert"]);
    let before = mouse.sent().len();
    ok(
        &mouse,
        dir.path(),
        &["export", path.to_str().unwrap(), "--profile", "1"],
    );
    let sent = &mouse.sent()[before..];
    assert!(sent.iter().all(|r| r[6] != 0x85), "no active-profile read");
    assert!(
        sent.contains(&req(&[(4, 0x02), (6, 0x88), (7, 1)])),
        "debounce of profile 1"
    );
    assert!(
        sent.iter()
            .any(|r| r[5] == 0x01 && r[6] == 0x81 && r[7] == 1),
        "DPI of profile 1"
    );

    let written: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let expected: toml::Table = r#"
        format = 1
        model = "Model O Wireless"

        [device]
        polling_rate = 1000
        lift_off = 1
        sleep = "2m"
        scroll = "invert"

        [profile]
        dpi = [400, 800, 1600]
        dpi_stage = 2
        dpi_colors = ["ffff00", "0000ff", "ff0000"]
        debounce = 10
        effect = "solid 00ff00"

        [profile.binds]
        scroll-up = "mouse:scroll-down"
        scroll-down = "mouse:scroll-up"
    "#
    .parse()
    .unwrap();
    assert_eq!(written, expected);
}

#[test]
fn an_exported_profile_imports_back_with_its_memory() {
    let dir = state();
    let out_dir = tempfile::tempdir().unwrap();
    let path = out_dir.path().join("p.toml");
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["set", "scroll", "invert"]);
    ok(&mouse, dir.path(), &["set", "effect", "wave"]);
    ok(&mouse, dir.path(), &["set", "brightness", "200", "50"]);
    ok(
        &mouse,
        dir.path(),
        &["set", "bind", "back", "key:ctrl+KeyA"],
    );
    ok(&mouse, dir.path(), &["export", path.to_str().unwrap()]);

    let fresh = state();
    ok(
        &mouse,
        fresh.path(),
        &["import", path.to_str().unwrap(), "--yes"],
    );
    for (args, expected) in [
        (&["get", "scroll"][..], "invert  (set by gctl)\n"),
        (&["get", "effect"][..], "wave rate=40  (set by gctl)\n"),
        (&["get", "brightness"][..], "200 50  (set by gctl)\n"),
        (
            &["get", "bind", "back"][..],
            "key:ctrl+KeyA  (set by gctl)\n",
        ),
    ] {
        assert_eq!(ok(&mouse, fresh.path(), args), expected, "{args:?}");
    }
}

// ---------------------------------------------------------------- import

fn import_file(dir: &TempDir, body: &str) -> String {
    let path = dir.path().join("in.toml");
    std::fs::write(&path, body).unwrap();
    path.to_str().unwrap().to_owned()
}

const PROFILE_ONLY: &str = r#"
format = 1
model = "Model O Wireless"

[profile]
dpi = [400, 800]
dpi_stage = 2
debounce = 0
effect = "solid 00ff00"

[profile.binds]
back = "mouse:forward"
"#;

const WITH_DEVICE: &str = r#"
format = 1
model = "Model O Wireless"

[device]
polling_rate = 500

[profile]
debounce = 4
"#;

#[test]
fn import_writes_into_the_given_profile() {
    let dir = state();
    let files = tempfile::tempdir().unwrap();
    let path = import_file(&files, PROFILE_ONLY);
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["import", &path, "--profile", "3"]);
    assert_eq!(
        mouse.writes(),
        vec![
            req(&[(4, 0x02), (5, 0x01), (6, 0x02), (7, 3), (8, 1)]),
            req(&[
                (4, 10),
                (5, 0x01),
                (6, 0x01),
                (7, 3),
                (8, 2),
                (9, 0x01),
                (10, 0x90),
                (11, 0x01),
                (12, 0x90),
                (13, 0x03),
                (14, 0x20),
                (15, 0x03),
                (16, 0x20),
            ]),
            req(&[(4, 0x02), (5, 0x01), (6, 0x02), (7, 3), (8, 2)]),
            req(&[(4, 0x01), (6, 0x08), (7, 3), (8, 0)]),
            req(&[(4, 8), (5, 0x02), (7, 3), (8, 0xFF), (9, 4), (13, 0xFF)]),
            bind_req(3, 4, &[(10, 1), (11, 1), (12, 5)]),
        ]
    );
    assert_eq!(
        ok(&mouse, dir.path(), &["get", "effect", "--profile", "3"]),
        "solid 00ff00  (set by gctl)\n"
    );
}

#[test]
fn import_applies_device_settings_then_the_profile() {
    let dir = state();
    let files = tempfile::tempdir().unwrap();
    let path = import_file(
        &files,
        r#"
format = 1
model = "Model O Wireless"

[device]
polling_rate = 500
lift_off = 2
sleep = "90s"
brightness = { wired = 200, wireless = 50 }
scroll = "invert"

[profile]
dpi = [400, 800, 1600]
dpi_stage = 2
dpi_colors = ["ff0000", "00ff00", "0000ff"]
debounce = 4
effect = "wave rate=40"

[profile.binds]
scroll-up = "mouse:scroll-up"
"#,
    );
    let mouse = Mouse::new();
    ok(
        &mouse,
        dir.path(),
        &["import", &path, "--profile", "1", "--yes"],
    );

    let mut expected = vec![
        req(&[(4, 0x01), (5, 0x01), (7, 2)]),
        req(&[(4, 0x01), (5, 0x01), (6, 0x07), (7, 1)]),
        req(&[(4, 0x02), (6, 0x07), (7, 0x00), (8, 90)]),
        req(&[(4, 0x02), (5, 0x02), (6, 0x02), (7, 0x01), (8, 200)]),
        req(&[(4, 0x02), (5, 0x02), (6, 0x02), (7, 0x00), (8, 50)]),
    ];
    expected.extend(scroll_binds(1, 17, 16));
    expected.extend(scroll_binds(2, 17, 16));
    expected.extend(scroll_binds(3, 17, 16));
    expected.extend([
        req(&[(4, 0x02), (5, 0x01), (6, 0x02), (7, 1), (8, 1)]),
        req(&[
            (4, 14),
            (5, 0x01),
            (6, 0x01),
            (7, 1),
            (8, 3),
            (9, 0x01),
            (10, 0x90),
            (11, 0x01),
            (12, 0x90),
            (13, 0x03),
            (14, 0x20),
            (15, 0x03),
            (16, 0x20),
            (17, 0x06),
            (18, 0x40),
            (19, 0x06),
            (20, 0x40),
        ]),
        req(&[(4, 0x02), (5, 0x01), (6, 0x02), (7, 1), (8, 2)]),
        req(&[
            (4, 10),
            (5, 0x02),
            (6, 0x01),
            (7, 1),
            (8, 0xFF),
            (12, 0xFF),
            (16, 0xFF),
        ]),
        req(&[(4, 0x01), (6, 0x08), (7, 1), (8, 4)]),
        req(&[(4, 5), (5, 0x02), (7, 1), (8, 0xFF), (9, 8), (11, 130)]),
        bind_req(1, 16, &[(10, 1), (11, 1), (12, 16)]),
    ]);
    assert_eq!(mouse.writes(), expected);
}

#[test]
fn import_validates_the_whole_file_before_writing() {
    let bad = [
        (
            "format = 2\nmodel = \"Model O Wireless\"\n[profile]\ndebounce = 4\n",
            "unknown format",
        ),
        (
            "format = 1\nmodel = \"Model O Wireless\"\n[profile]\neffect = \"rainbow\"\n",
            "bad effect",
        ),
        (
            "format = 1\nmodel = \"Model O Wireless\"\n[profile]\ndpi = [400, 800]\ndpi_colors = [\"ff0000\", \"00ff00\", \"0000ff\"]\n",
            "colour count differs from the stage count",
        ),
        (
            "format = 1\nmodel = \"Model O Wireless\"\n[device]\npolling_rate = 1000\n[profile]\ndebounce = 99\n",
            "a bad profile value after a good device value",
        ),
        (
            "format = 1\nmodel = \"Model O Wireless\"\n[profile.binds]\nback = \"key:Foo\"\n",
            "bad bind",
        ),
    ];
    let o = "format = 1\nmodel = \"Model O Wireless\"\n";
    let more = [
        (
            "[profile]\ndpi = [400, 800]\ndpi_stage = 3\n",
            "a stage past the file's own stages",
        ),
        (
            "[profile]\ndpi_colours = [\"ff0000\"]\n",
            "a misspelled key",
        ),
        ("[profile.bind]\nback = \"none\"\n", "a misspelled table"),
        ("colour = 1\n", "an unknown top-level key"),
        ("[device]\nsleep = \"0s\"\n", "zero sleep"),
        ("[device]\nscroll = \"sideways\"\n", "bad scroll"),
        ("[device]\npolling_rate = 300\n", "bad polling rate"),
        ("[device]\nlift_off = 3\n", "bad lift-off"),
        (
            "[device]\nbrightness = { wired = 256, wireless = 0 }\n",
            "brightness past 255",
        ),
        ("[profile]\ndpi = [50]\n", "DPI below 100"),
        (
            "[profile]\ndpi = [400, 800, 1600]\ndpi_colors = [\"ff0000\", \"zzzzzz\", \"0000ff\"]\n",
            "bad colour",
        ),
        (
            "[profile]\neffect = \"pulse ff0000\"\n",
            "pulse with one colour",
        ),
    ];
    let bad: Vec<(String, &str)> = bad
        .iter()
        .map(|(body, why)| ((*body).to_owned(), *why))
        .chain(more.iter().map(|(body, why)| (format!("{o}{body}"), *why)))
        .collect();
    for (body, why) in bad {
        let dir = state();
        let files = tempfile::tempdir().unwrap();
        let path = import_file(&files, &body);
        let mouse = Mouse::new();
        let out = gctl(&mouse, dir.path(), &["import", &path, "--yes"]);
        assert_eq!(out.outcome, Outcome::Exit(1), "{why}");
        assert!(
            out.stderr.starts_with(&format!("error: {path:?}: ")),
            "{why}: names the file: {}",
            out.stderr
        );
        assert_eq!(
            out.stderr.lines().count(),
            1,
            "{why}: one line: {}",
            out.stderr
        );
        assert!(mouse.writes().is_empty(), "{why}: wrote before validating");
        assert_eq!(out.locked, 0, "{why}: the file is checked before the mouse");
    }
}

#[test]
fn import_errors_name_the_key() {
    let o = "format = 1\nmodel = \"Model O Wireless\"\n";
    let cases = [
        (
            "[profile]\ndpi = [400, 800]\ndpi_stage = 3\n",
            "profile.dpi_stage: DPI stage 3 is out of range 1..=2",
        ),
        (
            "[profile]\ndpi = [400, 800]\ndpi_colors = [\"ff0000\", \"00ff00\", \"0000ff\"]\n",
            "profile.dpi_colors: got 3 colour(s) for 2 DPI stage(s)",
        ),
        (
            "[profile]\ndpi_stage = 4\n",
            "profile.dpi_stage: DPI stage 4 is out of range 1..=3",
        ),
        (
            "[profile]\ndpi_colors = [\"ff0000\", \"00ff00\"]\n",
            "profile.dpi_colors: got 2 colour(s) for 3 DPI stage(s)",
        ),
        (
            "[profile.binds]\nmiddle = \"none\"\n",
            "profile.binds.middle: unknown button \"middle\"",
        ),
    ];
    for (body, message) in cases {
        let dir = state();
        let files = tempfile::tempdir().unwrap();
        let path = import_file(&files, &format!("{o}{body}"));
        let mouse = Mouse::new();
        let out = gctl(&mouse, dir.path(), &["import", &path, "--yes"]);
        assert_eq!(out.outcome, Outcome::Exit(1), "{body}");
        assert_eq!(
            out.stderr,
            format!("error: {path:?}: {message}\n"),
            "{body}"
        );
        assert!(mouse.writes().is_empty(), "{body}");
    }

    // a value that fails its own parse names its key
    let keyed = [
        ("[device]\npolling_rate = 300\n", "device.polling_rate: "),
        ("[device]\nlift_off = 3\n", "device.lift_off: "),
        ("[device]\nsleep = \"0s\"\n", "device.sleep: "),
        ("[device]\nscroll = \"sideways\"\n", "device.scroll: "),
        ("[profile]\ndpi = [50]\n", "profile.dpi: "),
        ("[profile]\ndpi_stage = 7\n", "profile.dpi_stage: "),
        (
            "[profile]\ndpi_colors = [\"zzzzzz\"]\n",
            "profile.dpi_colors: ",
        ),
        ("[profile]\ndebounce = 99\n", "profile.debounce: "),
        ("[profile]\neffect = \"rainbow\"\n", "profile.effect: "),
        (
            "[profile.binds]\nback = \"key:Foo\"\n",
            "profile.binds.back: ",
        ),
    ];
    for (body, key) in keyed {
        let dir = state();
        let files = tempfile::tempdir().unwrap();
        let path = import_file(&files, &format!("{o}{body}"));
        let out = gctl(&Mouse::new(), dir.path(), &["import", &path, "--yes"]);
        assert_eq!(out.outcome, Outcome::Exit(1), "{body}");
        assert!(
            out.stderr.starts_with(&format!("error: {path:?}: {key}")),
            "{body}: {}",
            out.stderr
        );
    }

    // a toml-level error names its position
    let dir = state();
    let files = tempfile::tempdir().unwrap();
    let path = import_file(
        &files,
        &format!("{o}[profile]\ndpi_colours = [\"ff0000\"]\n"),
    );
    let out = gctl(&Mouse::new(), dir.path(), &["import", &path, "--yes"]);
    assert!(
        out.stderr.starts_with(&format!(
            "error: {path:?}: line 4, column 1: unknown field `dpi_colours`"
        )),
        "{}",
        out.stderr
    );

    // a message serde already quotes is not escaped twice
    let path = import_file(&files, &format!("{o}[profile]\ndebounce = \"a\\nb\"\n"));
    let out = gctl(&Mouse::new(), dir.path(), &["import", &path, "--yes"]);
    assert!(
        out.stderr
            .contains(r#"invalid type: string "a\nb", expected u8"#),
        "{}",
        out.stderr
    );
}

#[test]
fn a_mixed_import_over_unreadable_memory_writes_nothing() {
    let dir = state();
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["set", "effect", "wave"]);
    std::fs::write(memory_file(&dir), "this is [ not toml").unwrap();
    let files = tempfile::tempdir().unwrap();
    let path = import_file(
        &files,
        "format = 1\nmodel = \"Model O Wireless\"\n[device]\npolling_rate = 500\n[profile]\neffect = \"off\"\n",
    );
    let before = mouse.writes().len();
    let out = gctl(&mouse, dir.path(), &["import", &path, "--yes"]);
    assert_eq!(out.outcome, Outcome::Exit(1), "{}", out.stderr);
    assert!(
        out.stderr.starts_with(&format!(
            "error: cannot read remembered values from {:?}: ",
            memory_file(&dir)
        )),
        "{}",
        out.stderr
    );
    assert_eq!(
        mouse.writes().len(),
        before,
        "the polling rate went out before the refusal"
    );
}

#[test]
fn an_import_records_each_remembered_setting_on_its_own() {
    // (the file's only setting, the read that must show it, the expected value)
    let o = "format = 1\nmodel = \"Model O Wireless\"\n";
    let cases = [
        (
            "[profile]\neffect = \"solid 00ff00\"\n",
            &["get", "effect"][..],
            "solid 00ff00  (set by gctl)\n",
        ),
        (
            "[profile.binds]\nback = \"mouse:forward\"\n",
            &["get", "bind", "back"][..],
            "mouse:forward  (set by gctl)\n",
        ),
        (
            "[device]\nbrightness = { wired = 10, wireless = 20 }\n",
            &["get", "brightness"][..],
            "10 20  (set by gctl)\n",
        ),
        (
            "[device]\nscroll = \"default\"\n",
            &["get", "scroll"][..],
            "default  (set by gctl)\n",
        ),
        (
            "[profile]\ndpi_colors = [\"112233\", \"445566\", \"778899\"]\n",
            &["get", "dpi-colors"][..],
            "112233 445566 778899  (set by gctl)\n",
        ),
        (
            "[profile]\ndpi = [400, 800]\n",
            &["get", "dpi-colors"][..],
            "unknown\n",
        ),
    ];
    for (body, read, expected) in cases {
        let dir = state();
        let mouse = Mouse::new();
        // remember a different value of every setting first
        ok(&mouse, dir.path(), &["set", "effect", "wave"]);
        ok(&mouse, dir.path(), &["set", "bind", "back", "none"]);
        ok(&mouse, dir.path(), &["set", "brightness", "200", "50"]);
        ok(&mouse, dir.path(), &["set", "scroll", "invert"]);
        ok(
            &mouse,
            dir.path(),
            &["set", "dpi-colors", "ffff00", "0000ff", "ff0000"],
        );
        let files = tempfile::tempdir().unwrap();
        let path = import_file(&files, &format!("{o}{body}"));
        ok(&mouse, dir.path(), &["import", &path, "--yes"]);
        assert_eq!(ok(&mouse, dir.path(), read), expected, "{body}");
    }
}

#[test]
fn an_import_of_device_values_only_needs_no_memory() {
    let dir = state();
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["set", "effect", "wave"]);
    std::fs::write(memory_file(&dir), "this is [ not toml").unwrap();
    let files = tempfile::tempdir().unwrap();
    let path = import_file(
        &files,
        "format = 1\nmodel = \"Model O Wireless\"\n[device]\npolling_rate = 500\nlift_off = 2\nsleep = \"90s\"\n[profile]\ndpi_stage = 2\ndebounce = 4\n",
    );
    let before = mouse.writes().len();
    let out = gctl(&mouse, dir.path(), &["import", &path, "--yes"]);
    assert_eq!(out.outcome, Outcome::Exit(0), "{}", out.stderr);
    assert_eq!(
        mouse.writes().len() - before,
        5,
        "polling rate, lift-off, sleep, DPI stage, debounce"
    );
}

#[test]
fn import_of_device_settings_needs_yes_when_not_interactive() {
    let dir = state();
    let files = tempfile::tempdir().unwrap();
    let path = import_file(&files, WITH_DEVICE);
    let mouse = Mouse::new();
    let out = gctl(&mouse, dir.path(), &["import", &path]);
    assert_eq!(out.outcome, Outcome::Exit(1));
    assert_eq!(
        out.stderr,
        "error: import changes device-wide settings; pass --yes to confirm\n"
    );
    assert!(mouse.writes().is_empty());
}

#[test]
fn import_of_device_settings_asks_first_when_interactive() {
    let prompt = "overwrite device-wide settings (polling rate, lift-off, sleep, brightness, scroll)? [y/N] ";
    let cases = [
        ("n\n", Outcome::Exit(1), 0),
        ("\n", Outcome::Exit(1), 0),
        ("", Outcome::Exit(1), 0),
        ("y\n", Outcome::Exit(0), 2),
    ];
    for (answer, outcome, writes) in cases {
        let dir = state();
        let files = tempfile::tempdir().unwrap();
        let path = import_file(&files, WITH_DEVICE);
        let mouse = Mouse::new();
        let out = gctl_with(
            Some(&mouse),
            0x2022,
            dir.path(),
            answer,
            true,
            &["import", &path],
        );
        assert!(out.stderr.contains(prompt), "{answer:?}: {}", out.stderr);
        assert_eq!(out.outcome, outcome, "{answer:?}");
        assert_eq!(
            mouse.writes().len(),
            writes,
            "{answer:?}: polling rate + debounce"
        );
    }
}

#[test]
fn import_from_another_model_notes_it_and_continues() {
    let dir = state();
    let files = tempfile::tempdir().unwrap();
    let path = import_file(
        &files,
        "format = 1\nmodel = \"Model D Wireless\"\n[profile]\ndebounce = 4\n",
    );
    let mouse = Mouse::new();
    let out = gctl(&mouse, dir.path(), &["import", &path]);
    assert_eq!(out.outcome, Outcome::Exit(0), "{}", out.stderr);
    assert_eq!(
        out.stderr,
        format!(
            "note: {path:?} holds settings for \"Model D Wireless\"; writing them to Model O Wireless\n"
        )
    );
    assert_eq!(
        mouse.writes(),
        vec![req(&[(4, 0x01), (6, 0x08), (7, 2), (8, 4)])]
    );
}

#[test]
fn a_toml_error_cannot_carry_raw_file_text_to_the_terminal() {
    for body in [
        "format = 1\nmodel = \"Model O Wireless\"\n[profile]\n\"\\u001b[31mx\" = 1\n",
        "format = 1\nmodel = \"Model O Wireless\"\n[profile]\ndebounce = \"\u{1b}[31m\"\n",
        "format = 1\nmodel = \"Model O Wireless\"\n\u{1b}[31m\n",
        "format = 1\nmodel = \"Model O Wireless\"\n[profile.binds]\n\"x\\u001b[31m\\nerror: forged\" = \"none\"\n",
        "format = 1\nmodel = \"Model O Wireless\"\n[profile.binds]\nback = \"key:\\u001b[31m\\nerror: forged\"\n",
        "format = 1\nmodel = \"Model O Wireless\"\n[device]\nsleep = \"\\u001b[31m\\nerror: forged\"\n",
        "format = 1\nmodel = \"Model O Wireless\"\n[profile.binds]\n\"x\\u202eevil\\u2066y\" = \"none\"\n",
        "format = 1\nmodel = \"Model O Wireless\"\n[profile]\n\"x\\u202eevil\\u2028y\" = 1\n",
        "format = 1\nmodel = \"Model O Wireless\"\n[profile]\neffect = \"wave\\u202e\"\n",
    ] {
        let dir = state();
        let files = tempfile::tempdir().unwrap();
        let path = import_file(&files, body);
        let out = gctl(&Mouse::new(), dir.path(), &["import", &path, "--yes"]);
        assert_eq!(out.outcome, Outcome::Exit(1), "{body:?}");
        assert_eq!(out.stderr.lines().count(), 1, "{body:?}: {}", out.stderr);
        assert_no_raw_file_text(&out.stderr, body);
    }

    // the memory file goes through the same escaper
    let dir = state();
    let mouse = Mouse::new();
    ok(&mouse, dir.path(), &["set", "effect", "wave"]);
    let memory = memory_file(&dir);
    let text = std::fs::read_to_string(&memory).unwrap();
    std::fs::write(&memory, format!("{text}\"x\\u202eevil\\u2028y\" = 1\n")).unwrap();
    let out = gctl(&mouse, dir.path(), &["get", "effect"]);
    assert_eq!(out.outcome, Outcome::Exit(1), "{}", out.stderr);
    assert_eq!(out.stderr.lines().count(), 1, "{}", out.stderr);
    assert_no_raw_file_text(&out.stderr, "memory");
}

/// No escape, bidi override, isolate or line separator from a file reaches stderr raw.
fn assert_no_raw_file_text(stderr: &str, why: &str) {
    for c in [
        '\u{1b}', '\u{202e}', '\u{202d}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
        '\u{200e}', '\u{200f}', '\u{2028}', '\u{2029}', '\u{feff}',
    ] {
        assert!(!stderr.contains(c), "{why:?}: raw {c:?} in {stderr:?}");
    }
}

#[test]
fn a_model_name_from_the_file_cannot_forge_lines() {
    let dir = state();
    let files = tempfile::tempdir().unwrap();
    let path = import_file(
        &files,
        "format = 1\nmodel = \"X\\nerror: forged\\u001b[31m\"\n[profile]\ndebounce = 4\n",
    );
    let out = gctl(&Mouse::new(), dir.path(), &["import", &path]);
    assert_eq!(out.outcome, Outcome::Exit(0), "{}", out.stderr);
    assert_eq!(out.stderr.lines().count(), 1, "{}", out.stderr);
    assert!(
        !out.stderr.contains('\u{1b}'),
        "no raw escape: {:?}",
        out.stderr
    );
}

#[test]
fn the_model_note_comes_before_the_prompt() {
    let dir = state();
    let files = tempfile::tempdir().unwrap();
    let path = import_file(
        &files,
        "format = 1\nmodel = \"Model D Wireless\"\n[device]\npolling_rate = 500\n",
    );
    let mouse = Mouse::new();
    let out = gctl_with(
        Some(&mouse),
        0x2022,
        dir.path(),
        "n\n",
        true,
        &["import", &path],
    );
    assert_eq!(out.outcome, Outcome::Exit(1));
    assert_eq!(
        out.stderr,
        format!(
            "note: {path:?} holds settings for \"Model D Wireless\"; writing them to Model O Wireless\n\
             overwrite device-wide settings (polling rate, lift-off, sleep, brightness, scroll)? [y/N] \
             error: import cancelled\n"
        )
    );
    assert_eq!(out.locked, 0, "the lock waits until the answer is yes");
    assert!(mouse.sent().is_empty());
}

#[test]
fn import_accepts_yes_in_any_case() {
    for (answer, accepted) in [
        ("y\n", true),
        ("Y\n", true),
        ("yes\n", true),
        ("YES\n", true),
        (" y \n", true),
        ("n\n", false),
        ("yep\n", false),
        ("\n", false),
        ("", false),
    ] {
        let dir = state();
        let files = tempfile::tempdir().unwrap();
        let path = import_file(&files, WITH_DEVICE);
        let mouse = Mouse::new();
        let out = gctl_with(
            Some(&mouse),
            0x2022,
            dir.path(),
            answer,
            true,
            &["import", &path],
        );
        let expected = if accepted {
            Outcome::Exit(0)
        } else {
            Outcome::Exit(1)
        };
        assert_eq!(out.outcome, expected, "{answer:?}: {}", out.stderr);
        assert_eq!(mouse.writes().is_empty(), !accepted, "{answer:?}");
    }
}

#[test]
fn import_validates_values_that_need_the_device_before_writing() {
    // the fake mouse has 3 DPI stages on every profile
    let bad = [
        (
            "[device]\npolling_rate = 500\n[profile]\ndpi_colors = [\"ff0000\", \"00ff00\"]\n",
            "two colours for three configured stages",
        ),
        (
            "[device]\npolling_rate = 500\n[profile]\ndpi_stage = 4\n",
            "a stage past the three configured stages",
        ),
    ];
    for (body, why) in bad {
        let dir = state();
        let files = tempfile::tempdir().unwrap();
        let path = import_file(
            &files,
            &format!("format = 1\nmodel = \"Model O Wireless\"\n{body}"),
        );
        let mouse = Mouse::new();
        let out = gctl(&mouse, dir.path(), &["import", &path, "--yes"]);
        assert_eq!(out.outcome, Outcome::Exit(1), "{why}");
        assert!(out.stderr.starts_with("error:"), "{why}: {}", out.stderr);
        assert!(mouse.writes().is_empty(), "{why}: wrote before validating");
    }
}
