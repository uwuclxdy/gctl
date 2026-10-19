use std::fs::File;
use std::io::IsTerminal;

use gctl::{Env, Outcome};

fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(run_main())
}

fn run_main() -> u8 {
    let state_dir = gctl::state_dir(&|key| std::env::var_os(key));
    let lock_path = gctl::lock_path(&|key| std::env::var_os(key), state_dir.as_deref());

    // The device lock, taken lazily and held until exit.
    let mut lock_file: Option<File> = None;
    let mut lock = || -> Result<(), gctl::HostError> {
        let Some(path) = lock_path.clone() else {
            return Err(gctl::HostError::from(std::io::Error::other(
                "cannot find a place for the lock file; set XDG_RUNTIME_DIR or HOME",
            )));
        };
        let file = gctl::take_lock(&path, &mut std::io::stderr().lock())?;
        lock_file = Some(file);
        Ok(())
    };

    let mut open = || {
        let api = hidapi::HidApi::new().map_err(|error| {
            gctl::HostError::from(std::io::Error::other(format!(
                "cannot list HID devices: {error}"
            )))
        })?;
        gctl_proto::Device::open(&api).map_err(gctl::HostError::from)
    };

    let stdin = std::io::stdin();
    let interactive = stdin.is_terminal();
    let mut stdin = stdin.lock();
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();

    let mut env = Env {
        open: &mut open,
        lock: &mut lock,
        state_dir: state_dir.as_deref(),
        stdin: &mut stdin,
        stdout: &mut stdout,
        stderr: &mut stderr,
        interactive,
    };
    let outcome = gctl::run(std::env::args_os(), &mut env);

    match outcome {
        Outcome::Exit(code) => code,
        Outcome::Gui | Outcome::Tray => {
            eprintln!("error: the GUI is not built yet");
            1
        }
    }
}
