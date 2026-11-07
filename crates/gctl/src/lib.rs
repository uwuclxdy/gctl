//! gctl: GUI, CLI and tray configurator for Glorious Core v1 wireless mice.

pub mod parse;

mod export;
mod get;
pub mod gui;
mod import;
mod memory;
mod set;
mod writes;

use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use gctl_proto::{Device, DeviceState, Profile, Transport};

/// What `main` does after [`run`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Exit with this code: 0 success, 1 runtime failure, 2 usage error.
    Exit(u8),
    /// No subcommand given: open the GUI.
    Gui,
    /// `--tray`: start the GUI hidden in the tray.
    Tray,
}

/// A failure of the host side `main` injects: opening the mouse, taking the lock.
pub type HostError = Box<dyn std::error::Error + Send + Sync>;

/// Everything a CLI run touches, injected so tests never reach real devices or paths.
pub struct Env<'a, T> {
    /// Finds and opens the mouse, `Ok(None)` when none is plugged in. Called at
    /// most once, only after the arguments are valid; never for the GUI outcomes.
    pub open: &'a mut dyn FnMut() -> Result<Option<Device<T>>, HostError>,
    /// Takes the cross-process device lock. Called at most once, right before
    /// the first report is sent; held until the process exits.
    pub lock: &'a mut dyn FnMut() -> Result<(), HostError>,
    /// Where gctl remembers values the mouse cannot read back; `None` when no
    /// state dir resolves, which fails only the commands that use memory.
    pub state_dir: Option<&'a Path>,
    pub stdin: &'a mut dyn BufRead,
    pub stdout: &'a mut dyn Write,
    pub stderr: &'a mut dyn Write,
    /// stdin is a terminal, so gctl may prompt.
    pub interactive: bool,
}

/// A command failure, classified by its exit code and how it renders.
#[derive(Debug)]
pub(crate) enum CmdError {
    /// A bad argument: exit 2, `error: <message>`.
    Usage(String),
    /// A device error: exit 1, printed with its source chain.
    Runtime(gctl_proto::Error),
    /// A runtime failure with a plain message: exit 1.
    Message(String),
    /// A local I/O failure (memory, file): exit 1.
    Io(std::io::Error),
    /// A device write failed and clearing its memory also failed: two lines.
    WriteThenSave {
        device_error: gctl_proto::Error,
        save_message: String,
    },
    /// stdout's reader walked away: exit 0, silent.
    BrokenPipe,
}

impl From<gctl_proto::Error> for CmdError {
    fn from(error: gctl_proto::Error) -> Self {
        CmdError::Runtime(error)
    }
}

impl From<parse::ParseError> for CmdError {
    fn from(error: parse::ParseError) -> Self {
        CmdError::Usage(error.0)
    }
}

impl From<std::io::Error> for CmdError {
    fn from(error: std::io::Error) -> Self {
        CmdError::Io(error)
    }
}

impl CmdError {
    /// The lines this failure renders after `error: `, in order, shared by the
    /// CLI's `report` and the GUI's error banner. `BrokenPipe` renders nothing.
    pub(crate) fn message(&self) -> Vec<String> {
        match self {
            CmdError::Usage(message) | CmdError::Message(message) => vec![message.clone()],
            CmdError::Runtime(error) => vec![error_chain(error)],
            CmdError::Io(error) => vec![error.to_string()],
            CmdError::WriteThenSave {
                device_error,
                save_message,
            } => vec![error_chain(device_error), save_message.clone()],
            CmdError::BrokenPipe => Vec::new(),
        }
    }
}

#[derive(Parser)]
#[command(
    name = "gctl",
    version,
    about = "Configure Glorious Core v1 wireless mice on Linux",
    color = clap::ColorChoice::Never,
    after_help = "Report bugs: https://github.com/uwuclxdy/gctl/issues"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    /// Start the GUI hidden in the tray.
    #[arg(long)]
    tray: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print one setting, or every setting of the profile.
    Get {
        setting: Option<String>,
        button: Option<String>,
        #[arg(long)]
        profile: Option<u8>,
    },
    /// Write a setting.
    Set {
        setting: String,
        #[arg(value_name = "VALUE", num_args = 0..)]
        args: Vec<String>,
        #[arg(long)]
        rate: Option<String>,
        #[arg(long)]
        profile: Option<u8>,
    },
    /// Export one profile plus the device-wide settings to a TOML file.
    Export {
        path: PathBuf,
        #[arg(long)]
        profile: Option<u8>,
    },
    /// Import a profile from a TOML file.
    Import {
        path: PathBuf,
        #[arg(long)]
        profile: Option<u8>,
        #[arg(long)]
        yes: bool,
    },
}

/// The error when neither `XDG_RUNTIME_DIR` nor the state dir can hold the
/// lock file, shared by `main` and the GUI.
pub const NO_LOCK_LOCATION: &str =
    "cannot find a place for the lock file; set XDG_RUNTIME_DIR or HOME";

/// The state dir: `$XDG_STATE_HOME/gctl`, else `$HOME/.local/state/gctl`. An
/// empty or relative variable counts as unset; `None` when neither is usable.
/// `var` is `std::env::var_os` in `main`.
pub fn state_dir(var: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let xdg = var("XDG_STATE_HOME").and_then(absolute);
    if let Some(home) = xdg {
        return Some(home.join("gctl"));
    }
    let home = var("HOME").and_then(absolute)?;
    Some(home.join(".local").join("state").join("gctl"))
}

/// The lock file: `$XDG_RUNTIME_DIR/gctl.lock`, else `<state_dir>/gctl.lock`. An
/// empty or relative variable counts as unset; `None` when neither is usable.
pub fn lock_path(
    var: &dyn Fn(&str) -> Option<OsString>,
    state_dir: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(runtime) = var("XDG_RUNTIME_DIR").and_then(absolute) {
        return Some(runtime.join("gctl.lock"));
    }
    let state_dir = state_dir?;
    Some(state_dir.join("gctl.lock"))
}

/// The KDE configuration file the window's animation factor lives in:
/// `$XDG_CONFIG_HOME/kdeglobals`, else `$HOME/.config/kdeglobals`. An empty or
/// relative variable counts as unset; `None` when neither is usable. `var` is
/// `std::env::var_os` in `main`.
pub fn kdeglobals_path(var: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(config) = var("XDG_CONFIG_HOME").and_then(absolute) {
        return Some(config.join("kdeglobals"));
    }
    let home = var("HOME").and_then(absolute)?;
    Some(home.join(".config").join("kdeglobals"))
}

/// Opens (creating it and its dir) and exclusively locks the lock file at
/// `path`. While another process holds it, prints `waiting for another gctl to
/// finish` on `stderr` and blocks. Every error reads `could not lock "<path>": <cause>`.
pub fn take_lock(path: &Path, stderr: &mut dyn Write) -> Result<std::fs::File, HostError> {
    use std::fs::TryLockError;

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| lock_error(path, error))?;
    }
    let file = std::fs::File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(|error| lock_error(path, error))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            let _ = stderr.write_all(b"waiting for another gctl to finish\n");
            let _ = stderr.flush();
            file.lock().map_err(|error| lock_error(path, error))?;
        }
        Err(TryLockError::Error(error)) => return Err(lock_error(path, error)),
    }
    Ok(file)
}

fn lock_error(path: &Path, error: std::io::Error) -> HostError {
    HostError::from(std::io::Error::other(format!(
        "could not lock {path:?}: {error}"
    )))
}

fn absolute(value: OsString) -> Option<PathBuf> {
    let path = PathBuf::from(value);
    if path.as_os_str().is_empty() || !path.is_absolute() {
        None
    } else {
        Some(path)
    }
}

/// The library entry point. `args` includes the program name, as `main` passes
/// `std::env::args_os()`.
pub fn run<T, I, S>(args: I, env: &mut Env<'_, T>) -> Outcome
where
    T: Transport,
    I: IntoIterator<Item = S>,
    S: Into<OsString> + Clone,
{
    let Env {
        open,
        lock,
        state_dir,
        stdin,
        stdout,
        stderr,
        interactive,
    } = env;
    let interactive = *interactive;
    let stdin: &mut dyn BufRead = &mut **stdin;
    let stdout: &mut dyn Write = &mut **stdout;
    let stderr: &mut dyn Write = &mut **stderr;

    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => return clap_error(stdout, stderr, error),
    };
    dispatch(
        cli,
        open,
        lock,
        *state_dir,
        stdin,
        stdout,
        stderr,
        interactive,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "threads the injected Env through unchanged"
)]
fn dispatch<T: Transport>(
    cli: Cli,
    open: &mut dyn FnMut() -> Result<Option<Device<T>>, HostError>,
    lock: &mut dyn FnMut() -> Result<(), HostError>,
    state_dir: Option<&Path>,
    stdin: &mut dyn BufRead,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    interactive: bool,
) -> Outcome {
    enum Prepared {
        Get(get::GetCmd, Option<Profile>),
        Set(set::SetCmd, Option<Profile>),
        Export(PathBuf, Option<Profile>),
        Import(import::PreparedImport),
    }

    let Some(command) = cli.command else {
        return if cli.tray {
            Outcome::Tray
        } else {
            Outcome::Gui
        };
    };
    if cli.tray {
        return report(
            stderr,
            CmdError::Usage("--tray cannot be combined with a command".to_owned()),
        );
    }

    // Validate every argument and do the file-only work before the mouse opens.
    let prepared = match command {
        Cmd::Get {
            setting,
            button,
            profile,
        } => match get::prepare(setting, button, profile) {
            Ok((cmd, profile)) => Prepared::Get(cmd, profile),
            Err(error) => return report(stderr, error),
        },
        Cmd::Set {
            setting,
            args,
            rate,
            profile,
        } => match set::prepare(setting, args, rate, profile) {
            Ok((cmd, profile)) => Prepared::Set(cmd, profile),
            Err(error) => return report(stderr, error),
        },
        Cmd::Export { path, profile } => match export::prepare(path, profile) {
            Ok((path, profile)) => Prepared::Export(path, profile),
            Err(error) => return report(stderr, error),
        },
        Cmd::Import { path, profile, yes } => match import::prepare(path, profile, yes) {
            Ok(prepared) => Prepared::Import(prepared),
            Err(error) => return report(stderr, error),
        },
    };

    let device = match open() {
        Ok(Some(device)) => device,
        Ok(None) => {
            return report(
                stderr,
                CmdError::Message("no Glorious mouse found".to_owned()),
            );
        }
        Err(error) => return report(stderr, CmdError::Message(error_chain(&*error))),
    };
    let device = &device;

    if !device.model().tested {
        let _ = writeln!(
            stderr,
            "note: {} is untested with gctl",
            device.model().name
        );
    }

    // Import's model note and confirmation prompt run before the lock.
    if let Prepared::Import(prepared) = &prepared
        && let Err(error) = prepared.confirm(device.model().name, stdin, stderr, interactive)
    {
        return report(stderr, error);
    }

    if let Err(error) = lock() {
        return report(stderr, CmdError::Message(error_chain(&*error)));
    }

    let result = match prepared {
        Prepared::Get(cmd, profile) => get::get(device, state_dir, stdout, cmd, profile),
        Prepared::Set(cmd, profile) => set::set(device, state_dir, cmd, profile),
        Prepared::Export(path, profile) => export::export(device, state_dir, profile, &path),
        Prepared::Import(prepared) => import::execute(device, state_dir, prepared).map(|_| ()),
    };
    match result {
        Ok(_) => Outcome::Exit(0),
        Err(error) => report(stderr, error),
    }
}

/// Validates the `--profile` flag value (1-3) before any device access.
pub(crate) fn validate_profile(flag: Option<u8>) -> Result<Option<Profile>, CmdError> {
    match flag {
        None => Ok(None),
        Some(id) => parse::profile_number(id)
            .map(Some)
            .map_err(|error| CmdError::Usage(error.0)),
    }
}

/// The profile to act on: the flag value, or the device's active profile.
pub(crate) fn resolve_profile<T: Transport>(
    device: &Device<T>,
    arg: Option<Profile>,
) -> Result<Profile, CmdError> {
    match arg {
        Some(profile) => Ok(profile),
        None => device.profile().map_err(CmdError::from),
    }
}

/// Refuses an asleep mouse; every device command but `get battery` calls this.
pub(crate) fn check_awake<T: Transport>(device: &Device<T>) -> Result<(), CmdError> {
    match device.status() {
        Ok(DeviceState::Asleep) => Err(CmdError::Runtime(gctl_proto::Error::Asleep)),
        Ok(
            DeviceState::Awake
            | DeviceState::WakingUp
            | DeviceState::Unknown(_)
            | DeviceState::Invalid,
        ) => Ok(()),
        Err(error) => Err(CmdError::Runtime(error)),
    }
}

/// Writes a command's payload to stdout; a reader that walked away is exit 0.
pub(crate) fn write_out(stdout: &mut dyn Write, text: &str) -> Result<(), CmdError> {
    match stdout.write_all(text.as_bytes()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Err(CmdError::BrokenPipe),
        Err(error) => Err(CmdError::Io(error)),
    }
}

fn report(stderr: &mut dyn Write, error: CmdError) -> Outcome {
    let code = match &error {
        CmdError::Usage(_) => 2,
        CmdError::BrokenPipe => 0,
        CmdError::Runtime(_)
        | CmdError::Message(_)
        | CmdError::Io(_)
        | CmdError::WriteThenSave { .. } => 1,
    };
    for line in error.message() {
        let _ = writeln!(stderr, "error: {line}");
    }
    Outcome::Exit(code)
}

fn clap_error<'a>(
    stdout: &'a mut dyn Write,
    stderr: &'a mut dyn Write,
    error: clap::Error,
) -> Outcome {
    let use_stdout = matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    );
    let rendered = error.render().to_string();
    let target = if use_stdout { stdout } else { stderr };
    match target.write_all(rendered.as_bytes()) {
        Ok(()) => {}
        Err(cause) if use_stdout && cause.kind() == std::io::ErrorKind::BrokenPipe => {
            return Outcome::Exit(0);
        }
        Err(_) => {}
    }
    if use_stdout {
        Outcome::Exit(0)
    } else {
        Outcome::Exit(2)
    }
}

/// An error and every `source` below it, joined by `: `.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut out = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        out.push_str(": ");
        out.push_str(&cause.to_string());
        source = cause.source();
    }
    out
}

/// A toml deserialize error as one line: `line L, column C: <message>`, with
/// every control character in the message escaped so a byte from the file can
/// never reach the terminal. The position comes from the error's span.
pub(crate) fn toml_error(source: &str, error: &toml::de::Error) -> String {
    let message = parse::escape_for_terminal(error.message());
    let Some(span) = error.span() else {
        return message.to_string();
    };
    let (line, column) = line_column(source, span.start);
    format!("line {line}, column {column}: {message}")
}

/// The 1-based line and column of a byte offset into `source`.
fn line_column(source: &str, index: usize) -> (usize, usize) {
    let index = index.min(source.len());
    let bytes = source.as_bytes();
    let line_start = bytes[..index]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |at| at + 1);
    let line = bytes[..line_start]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count();
    let column = source[line_start..index].chars().count();
    (line + 1, column + 1)
}
