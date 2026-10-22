//! The iced configurator window over [`state`] and [`io`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use iced::futures::StreamExt;
use iced::futures::channel::mpsc;
use iced::theme::Mode;
use iced::widget::{
    button, center, checkbox, column, container, pick_list, row, rule, scrollable, slider, space,
    stack, text, text_input, toggler,
};
use iced::{Background, Border, Color, Element, Length, Size, Subscription, Task};

use gctl_proto::{Binding, Button, Device, Profile};

use super::io;
use super::state::{
    Connection, Edit, EffectKind, ImportCheck, Leave, LoadError, Msg, Notice, Prompt, Request,
    Section, Snapshot, State, WriteFailure, button_label,
};
use super::theme::{self, CONTROL_HEIGHT, MONO_FONT, Scheme, UI_FONT};
use crate::memory::Memory;
use crate::parse;
use crate::writes::WriteOp;

/// The window's own message type, wrapping the state machine's `Msg`.
#[derive(Debug, Clone)]
enum Message {
    Machine(Msg),
    /// The desktop's colour scheme mode answered the boot query or changed.
    Theme(Mode),
    Loaded {
        device: Option<DeviceHandle>,
        result: Result<Snapshot, LoadError>,
    },
    Applied {
        device: Option<DeviceHandle>,
        error: Option<WriteFailure>,
        snapshot: Result<Snapshot, LoadError>,
        /// The memory re-loaded after the write, when the re-read failed.
        remembered: Option<Result<Memory, String>>,
    },
    Exported {
        device: Option<DeviceHandle>,
        result: Result<PathBuf, String>,
    },
    Imported {
        device: Option<DeviceHandle>,
        error: Option<String>,
        /// The writes that landed, on failure too.
        written: Vec<WriteOp>,
        /// The import's failing op handed at least one write report to the
        /// transport; `false` on success and on every failure before any
        /// report.
        reached: bool,
        /// `None` when `io::run_import` never ran under the lock (no mouse, a
        /// failed open, a lock failure): an import that ran always re-reads. A
        /// task that died after it may have reached the mouse answers
        /// `Some(Err(..))`.
        snapshot: Option<Result<Snapshot, LoadError>>,
        /// The memory re-loaded after the write, when the re-read failed.
        remembered: Option<Result<Memory, String>>,
    },
    ExportPath(String),
    ImportPath(String),
    ExportClicked,
    ImportClicked,
}

/// The opened device handle, shared with the blocking transaction threads.
type DeviceHandle = Arc<Mutex<Device<hidapi::HidDevice>>>;

/// Everything the window carries besides the state machine: the opened device,
/// the export/import path text, and the current colour scheme.
struct Shell {
    machine: State,
    device: Option<DeviceHandle>,
    state_dir: Option<PathBuf>,
    lock_path: Option<PathBuf>,
    export_path: String,
    import_path: String,
    scheme: Scheme,
}

#[derive(Clone)]
struct Flags {
    state_dir: Option<PathBuf>,
    lock_path: Option<PathBuf>,
}

impl Shell {
    fn boot(flags: Flags) -> (Shell, Task<Message>) {
        let (machine, requests) = State::new();
        let mut shell = Shell {
            machine,
            device: None,
            state_dir: flags.state_dir,
            lock_path: flags.lock_path,
            export_path: String::new(),
            import_path: String::new(),
            scheme: Scheme::Dark,
        };
        let mut task = shell.run_requests(requests);
        // The desktop's mode answers asynchronously; dark holds until it does.
        task = Task::batch([task, iced::system::theme().map(Message::Theme)]);
        (shell, task)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Machine(message) => {
                let requests = self.machine.update(message);
                self.run_requests(requests)
            }
            Message::Theme(mode) => {
                self.scheme = Scheme::from_mode(mode);
                Task::none()
            }
            Message::Loaded { device, result } => {
                self.device = device;
                let requests = self.machine.update(Msg::Loaded(result));
                self.run_requests(requests)
            }
            Message::Applied {
                device,
                error,
                snapshot,
                remembered,
            } => {
                self.device = device;
                let requests = self.machine.update(Msg::Applied {
                    error,
                    snapshot,
                    remembered,
                });
                self.run_requests(requests)
            }
            Message::Exported { device, result } => {
                self.device = device;
                let requests = self.machine.update(Msg::Exported(result));
                self.run_requests(requests)
            }
            Message::Imported {
                device,
                error,
                written,
                reached,
                snapshot,
                remembered,
            } => {
                self.device = device;
                let requests = self.machine.update(Msg::Imported {
                    error,
                    written,
                    reached,
                    snapshot,
                    remembered,
                });
                self.run_requests(requests)
            }
            Message::ExportPath(text) => {
                self.export_path = text;
                Task::none()
            }
            Message::ImportPath(text) => {
                self.import_path = text;
                Task::none()
            }
            Message::ExportClicked => {
                let path = if self.export_path.trim().is_empty() {
                    PathBuf::new()
                } else {
                    PathBuf::from(self.export_path.clone())
                };
                Task::done(Message::Machine(Msg::Export(path)))
            }
            Message::ImportClicked => {
                let path = if self.import_path.trim().is_empty() {
                    PathBuf::new()
                } else {
                    PathBuf::from(self.import_path.clone())
                };
                Task::done(Message::Machine(Msg::Import(path)))
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(tick_interval(self.machine.connection()))
                .map(|_| Message::Machine(Msg::Tick)),
            iced::system::theme_changes().map(Message::Theme),
        ])
    }

    fn run_requests(&mut self, requests: Vec<Request>) -> Task<Message> {
        let tasks: Vec<Task<Message>> = requests
            .into_iter()
            .map(|request| self.run_request(request))
            .collect();
        if tasks.is_empty() {
            Task::none()
        } else {
            Task::batch(tasks)
        }
    }

    fn run_request(&mut self, request: Request) -> Task<Message> {
        match request {
            Request::Load(profile) => self.load(profile),
            Request::Apply { profile, ops } => self.apply(profile, ops),
            Request::MakeActive(profile) => self.make_active(profile),
            Request::Export { profile, path } => self.export(profile, path),
            Request::PrepareImport { path } => self.prepare_import(path),
            Request::Import {
                profile,
                path,
                check,
            } => self.import(profile, path, check),
        }
    }

    /// Runs one device transaction off the UI thread: re-opens the handle when
    /// the state asks or none is held, runs `work` on the blocking pool, emits
    /// `Msg::LockWait` if another gctl holds the lock, and answers with `map`.
    /// `no_mouse` answers when no mouse is found; `failed` an open failure, so
    /// the mouse was never reached; `lost` a blocking task that died (a panic
    /// or cancel) after `work` may already have reached the mouse. Every
    /// transaction shares this one helper, so the re-open policy and the wait
    /// bridge cannot drift between them.
    fn device_task<T>(
        &mut self,
        no_mouse: impl Fn() -> T + Send + 'static,
        failed: impl Fn(String) -> T + Send + 'static,
        lost: impl Fn(String) -> T + Send + 'static,
        work: impl FnOnce(&DeviceHandle, &mpsc::UnboundedSender<Message>) -> T + Send + 'static,
        map: impl Fn(Option<DeviceHandle>, T) -> Message + Send + 'static,
    ) -> Task<Message>
    where
        T: Send + 'static,
    {
        let reopen = self.machine.reopen() || self.device.is_none();
        let device = self.device.clone();
        let (tx, rx) = mpsc::unbounded::<Message>();
        let work_tx = tx.clone();
        let runner = iced::futures::stream::once(async move {
            let outcome = spawn(move || -> (Option<DeviceHandle>, Result<T, String>) {
                let mut device = device;
                if reopen {
                    match open_device() {
                        Ok(Some(opened)) => device = Some(opened),
                        Ok(None) => return (None, Ok(no_mouse())),
                        Err(message) => return (None, Err(message)),
                    }
                }
                let Some(device) = device else {
                    return (None, Ok(no_mouse()));
                };
                let payload = work(&device, &work_tx);
                (Some(device), Ok(payload))
            })
            .await;
            let message = match outcome {
                Ok((device, Ok(payload))) => map(device, payload),
                // The open failed: the mouse was never reached.
                Ok((device, Err(message))) => map(device, failed(message)),
                // The blocking task died: `work` may already have sent reports.
                Err(error) => map(None, lost(error)),
            };
            let _ = tx.unbounded_send(message);
        })
        .filter_map(|_| async { None });
        iced::Task::run(iced::futures::stream::select(rx, runner), |message| message)
    }

    fn prepare_import(&mut self, path: PathBuf) -> Task<Message> {
        let model = self
            .machine
            .snapshot()
            .map(|snap| snap.model.name.to_owned());
        let profile = self.machine.edit_profile();
        Task::perform(
            async move {
                let result = spawn(move || {
                    let Some(profile) = profile else {
                        return Err("no mouse read yet".to_owned());
                    };
                    let Some(model) = model else {
                        return Err("no mouse read yet".to_owned());
                    };
                    io::check_import(&path, profile, &model)
                })
                .await;
                let check = match result {
                    Ok(check) => check,
                    Err(message) => Err(message),
                };
                Message::Machine(Msg::ImportChecked(check))
            },
            |message| message,
        )
    }

    fn load(&mut self, profile: Option<Profile>) -> Task<Message> {
        let state_dir = self.state_dir.clone();
        let lock_path = self.lock_path.clone();
        self.device_task(
            || Err(LoadError::NoMouse),
            |message| Err(LoadError::Failed(message)),
            |message| Err(LoadError::Failed(message)),
            move |device, sender| {
                let guard = lock_device(device);
                reload(
                    &guard,
                    state_dir.as_deref(),
                    lock_path.as_deref(),
                    sender,
                    profile,
                )
            },
            |device, result| Message::Loaded { device, result },
        )
    }

    fn apply(&mut self, profile: Profile, ops: Vec<WriteOp>) -> Task<Message> {
        let state_dir = self.state_dir.clone();
        let lock_path = self.lock_path.clone();
        self.device_task(
            || {
                (
                    Some(WriteFailure {
                        message: "no mouse found".to_owned(),
                        landed: Vec::new(),
                        reached: false,
                    }),
                    Err(LoadError::NoMouse),
                    None,
                )
            },
            |message| failed_write(message, false),
            |message| failed_write(message, true),
            move |device, sender| {
                let error = {
                    let guard = lock_device(device);
                    match io::with_lock(lock_path.as_deref(), wait(sender), || {
                        io::apply(&guard, state_dir.as_deref(), profile, &ops)
                    }) {
                        Ok(Ok(())) => None,
                        Ok(Err(failure)) => Some(failure),
                        Err(message) => Some(WriteFailure {
                            message,
                            landed: Vec::new(),
                            reached: false,
                        }),
                    }
                };
                let snapshot = {
                    let guard = lock_device(device);
                    reload(
                        &guard,
                        state_dir.as_deref(),
                        lock_path.as_deref(),
                        sender,
                        Some(profile),
                    )
                };
                let remembered = if snapshot.is_ok() {
                    None
                } else {
                    Some(io::read_memory(state_dir.as_deref()))
                };
                (error, snapshot, remembered)
            },
            |device, (error, snapshot, remembered)| Message::Applied {
                device,
                error,
                snapshot,
                remembered,
            },
        )
    }

    fn make_active(&mut self, profile: Profile) -> Task<Message> {
        let state_dir = self.state_dir.clone();
        let lock_path = self.lock_path.clone();
        self.device_task(
            || {
                (
                    Some(WriteFailure {
                        message: "no mouse found".to_owned(),
                        landed: Vec::new(),
                        reached: false,
                    }),
                    Err(LoadError::NoMouse),
                    None,
                )
            },
            |message| failed_write(message, false),
            |message| failed_write(message, true),
            move |device, sender| {
                let error = {
                    let guard = lock_device(device);
                    match io::with_lock(lock_path.as_deref(), wait(sender), || {
                        io::make_active(&guard, profile)
                    }) {
                        Ok(Ok(())) => None,
                        // `io::make_active` fails before its one report on the
                        // awake check and after it on the profile write; the
                        // plan carries no ops, so either value marks nothing.
                        Ok(Err(message)) => Some(WriteFailure {
                            message,
                            landed: Vec::new(),
                            reached: true,
                        }),
                        Err(message) => Some(WriteFailure {
                            message,
                            landed: Vec::new(),
                            reached: false,
                        }),
                    }
                };
                let snapshot = {
                    let guard = lock_device(device);
                    reload(
                        &guard,
                        state_dir.as_deref(),
                        lock_path.as_deref(),
                        sender,
                        Some(profile),
                    )
                };
                let remembered = if snapshot.is_ok() {
                    None
                } else {
                    Some(io::read_memory(state_dir.as_deref()))
                };
                (error, snapshot, remembered)
            },
            |device, (error, snapshot, remembered)| Message::Applied {
                device,
                error,
                snapshot,
                remembered,
            },
        )
    }

    fn export(&mut self, profile: Profile, path: PathBuf) -> Task<Message> {
        let state_dir = self.state_dir.clone();
        let lock_path = self.lock_path.clone();
        self.device_task(
            || Err("no mouse found".to_owned()),
            Err,
            Err,
            move |device, sender| {
                let path = match std::path::absolute(&path) {
                    Ok(path) => path,
                    Err(error) => return Err(format!("cannot resolve {path:?}: {error}")),
                };
                crate::export::prepare(path.clone(), Some(profile.get()))
                    .map_err(io::render)
                    .and_then(|(path, profile)| {
                        let guard = lock_device(device);
                        io::with_lock(lock_path.as_deref(), wait(sender), || {
                            crate::export::export(&guard, state_dir.as_deref(), profile, &path)
                        })
                        .and_then(|result| result.map_err(io::render))
                        .map(|()| path)
                    })
            },
            |device, result| Message::Exported { device, result },
        )
    }

    fn import(&mut self, profile: Profile, path: PathBuf, check: ImportCheck) -> Task<Message> {
        let state_dir = self.state_dir.clone();
        let lock_path = self.lock_path.clone();
        self.device_task(
            || {
                (
                    Some("no mouse found".to_owned()),
                    Vec::new(),
                    false,
                    None,
                    None,
                )
            },
            |message| (Some(message.clone()), Vec::new(), false, None, None),
            // The task died after it may have reached the mouse: the import is
            // reached-but-failed, so its check's fields stay this window's own.
            |message| {
                (
                    Some(message.clone()),
                    Vec::new(),
                    true,
                    Some(Err(LoadError::Failed(message))),
                    None,
                )
            },
            move |device, sender| {
                // Once `io::run_import` ran under the lock, the answer always
                // re-reads: the re-read carries the saved memory, the link
                // state and the next baseline. `reached` reports only whether
                // the failing op handed a write report to the transport, which
                // decides the check's fields' ownership.
                let (error, written, reached, ran) = {
                    let guard = lock_device(device);
                    match io::with_lock(lock_path.as_deref(), wait(sender), || {
                        io::run_import(&guard, state_dir.as_deref(), profile, &path, &check)
                    }) {
                        // Every write report went out and none failed.
                        Ok(Ok(written)) => (None, written, false, true),
                        Ok(Err(failure)) => {
                            (Some(failure.message), failure.landed, failure.reached, true)
                        }
                        // The lock was never taken: nothing was written.
                        Err(message) => (Some(message), Vec::new(), false, false),
                    }
                };
                let snapshot = if ran {
                    let guard = lock_device(device);
                    Some(reload(
                        &guard,
                        state_dir.as_deref(),
                        lock_path.as_deref(),
                        sender,
                        Some(profile),
                    ))
                } else {
                    None
                };
                let remembered = match &snapshot {
                    Some(Err(_)) => Some(io::read_memory(state_dir.as_deref())),
                    Some(Ok(_)) | None => None,
                };
                (error, written, reached, snapshot, remembered)
            },
            |device, (error, written, reached, snapshot, remembered)| Message::Imported {
                device,
                error,
                written,
                reached,
                snapshot,
                remembered,
            },
        )
    }
}

/// Runs a blocking closure on tokio's blocking pool; a join error becomes a
/// failed answer message, so a panic can never leave the line held.
async fn spawn<T>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|error| format!("device task failed: {error}"))
}

/// One Apply-shaped answer: the failure, the re-read, and the memory re-loaded
/// when the re-read failed.
type AppliedAnswer = (
    Option<WriteFailure>,
    Result<Snapshot, LoadError>,
    Option<Result<Memory, String>>,
);

/// An Apply-shaped answer for a failure that carried no landed ops: the open
/// failed (no report sent, `reached` false), or the blocking task died after it
/// may have sent one (`reached` true).
fn failed_write(message: String, reached: bool) -> AppliedAnswer {
    (
        Some(WriteFailure {
            message: message.clone(),
            landed: Vec::new(),
            reached,
        }),
        Err(LoadError::Failed(message)),
        None,
    )
}

/// Opens the mouse, refreshing the device list first. `Ok(None)` is "no mouse".
fn open_device() -> Result<Option<DeviceHandle>, String> {
    let mut api =
        hidapi::HidApi::new().map_err(|error| format!("cannot list HID devices: {error}"))?;
    api.refresh_devices()
        .map_err(|error| format!("cannot list HID devices: {error}"))?;
    Device::open(&api)
        .map_err(|error| io::render(crate::CmdError::from(error)))
        .map(|device| device.map(|device| Arc::new(Mutex::new(device))))
}

/// Locks the shared device handle for the duration of one transaction.
fn lock_device(device: &DeviceHandle) -> MutexGuard<'_, Device<hidapi::HidDevice>> {
    match device.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// The `on_wait` for one `io::with_lock`: a fresh `FnOnce` that forwards the
/// lock wait to the window as `Msg::LockWait`.
fn wait(sender: &mpsc::UnboundedSender<Message>) -> impl FnOnce() {
    let sender = sender.clone();
    move || {
        let _ = sender.unbounded_send(Message::Machine(Msg::LockWait));
    }
}

/// One snapshot read under the lock, on whatever thread calls it.
fn reload(
    device: &Device<hidapi::HidDevice>,
    state_dir: Option<&Path>,
    lock_path: Option<&Path>,
    sender: &mpsc::UnboundedSender<Message>,
    profile: Option<Profile>,
) -> Result<Snapshot, LoadError> {
    match io::with_lock(lock_path, wait(sender), || {
        io::read_snapshot(device, state_dir, profile)
    }) {
        Ok(result) => result,
        Err(message) => Err(LoadError::Failed(message)),
    }
}

/// The window settings: 1024x680, app id `gctl` on Linux.
pub(super) fn window_settings() -> iced::window::Settings {
    let mut settings = iced::window::Settings {
        size: Size::new(1024.0, 680.0),
        ..iced::window::Settings::default()
    };
    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id = "gctl".to_owned();
    }
    settings
}

/// Starts the window. `state_dir` and `lock_path` come from `main`.
pub fn run(state_dir: Option<&Path>, lock_path: Option<PathBuf>) -> iced::Result {
    let flags = Flags {
        state_dir: state_dir.map(Path::to_path_buf),
        lock_path,
    };
    let boot = move || Shell::boot(flags.clone());
    iced::application(boot, Shell::update, Shell::view)
        .theme(|shell: &Shell| shell.theme())
        .subscription(Shell::subscription)
        .window(window_settings())
        .title("gctl")
        .run()
}

/// The tick interval for the current connection.
fn tick_interval(connection: Connection) -> Duration {
    match connection {
        Connection::Ready => Duration::from_secs(60),
        Connection::Asleep => Duration::from_secs(5),
        Connection::Loading | Connection::NoMouse | Connection::Failed => Duration::from_secs(3),
    }
}

// --- the view -----------------------------------------------------------------

impl Shell {
    /// The iced theme this shell's scheme carries; `run` hands it to iced.
    fn theme(&self) -> iced::Theme {
        self.scheme.theme()
    }

    fn view(&self) -> Element<'_, Message> {
        self.render(self.scheme)
    }

    /// The whole window under one scheme, shared by the live window and the
    /// two-theme render tests.
    fn render(&self, scheme: Scheme) -> Element<'_, Message> {
        let base: Element<'_, Message> = match self.machine.connection() {
            Connection::Loading => center(ui_text("Loading")).into(),
            Connection::NoMouse => empty_state(
                scheme,
                "No mouse found",
                "Plug in a Glorious mouse, or install gctl's udev rule (70-gctl.rules) so your user can open its hidraw node."
                    .to_owned(),
            ),
            Connection::Asleep | Connection::Ready | Connection::Failed
                if self.machine.snapshot().is_none() =>
            {
                match self.machine.connection() {
                    Connection::Asleep => empty_state(
                        scheme,
                        "The mouse is asleep. Move it to wake it.",
                        String::new(),
                    ),
                    Connection::Failed => empty_state(
                        scheme,
                        "Could not read the mouse",
                        capitalize(self.machine.error().unwrap_or_default()),
                    ),
                    Connection::Loading | Connection::NoMouse | Connection::Ready => {
                        center(ui_text("Loading")).into()
                    }
                }
            }
            Connection::Asleep | Connection::Ready | Connection::Failed => {
                self.main_view(scheme)
            }
        };
        let overlay = self.modal(scheme);
        match overlay {
            Some(modal) => stack![base, modal].into(),
            None => base,
        }
    }

    fn modal(&self, scheme: Scheme) -> Option<Element<'_, Message>> {
        let prompt = self.machine.prompt()?;
        let content = match &prompt {
            Prompt::Discard(_) => discard_prompt(scheme, prompt, self.machine.section()),
            Prompt::Import(_) => import_prompt(scheme, prompt),
        };
        Some(opaque_modal(scheme, content))
    }

    fn main_view(&self, scheme: Scheme) -> Element<'_, Message> {
        let machine = &self.machine;
        let t = scheme.tokens();
        let error_banner = machine.error().map(|error| {
            container(ui_text(capitalize(error)).size(13).color(t.danger_text))
                .style(theme::banner(scheme))
                .width(Length::Fill)
                .padding([8.0, 12.0])
        });
        let outside_banner = machine.outside_change().then(|| {
            container(
                row![
                    ui_text(
                        "The mouse was changed outside this window. Values marked \"Last set by gctl\" may be out of date."
                    )
                    .size(13)
                    .color(t.warning),
                    space().width(Length::Fill),
                    button(control_label("Dismiss"))
                        .style(theme::warning_button(scheme))
                        .height(CONTROL_HEIGHT)
                        .on_press(Message::Machine(Msg::DismissOutsideChange)),
                ]
                .spacing(12)
                .align_y(iced::Alignment::Center),
            )
            .style(theme::banner_warning(scheme))
            .width(Length::Fill)
            .padding([8.0, 12.0])
        });
        let content = self.content_pane(scheme);
        let mut page = column![];
        if let Some(banner) = error_banner {
            page = page.push(banner);
        }
        if let Some(banner) = outside_banner {
            page = page.push(banner);
        }
        page = page.push(row![self.sidebar(scheme), content].height(Length::Fill));
        page.into()
    }

    fn sidebar(&self, scheme: Scheme) -> Element<'_, Message> {
        let machine = &self.machine;
        let t = scheme.tokens();
        let Some(snap) = machine.snapshot() else {
            return container(space()).width(220).into();
        };

        // The badge wraps under the model name, so the longest model name never
        // clips it against the fixed sidebar width.
        let mut model_row =
            column![ui_text(snap.model.name).size(18)].align_x(iced::Alignment::Start);
        if !snap.model.tested {
            model_row = model_row.push(warning_tag(scheme, "Untested")).spacing(4);
        }

        let battery: Element<'_, Message> = match snap.battery.percent {
            Some(percent) => ui_text(format!("Battery {percent}%"))
                .size(13)
                .color(t.text_2)
                .into(),
            None => row![
                ui_text("Battery").size(13).color(t.text_2),
                tag(scheme, "Unknown"),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into(),
        };
        let link = match snap.model.link {
            gctl_proto::Link::Wired => "Wired",
            gctl_proto::Link::Wireless => "Wireless",
        };

        let edit_profile = machine.edit_profile();
        let pick_blocked = machine.pick_blocked();
        let profiles = row(Profile::all().into_iter().map(|profile| {
            let is_edited = edit_profile == Some(profile);
            let id = if is_edited {
                "edited-profile".to_owned()
            } else {
                format!("profile-pick-{}", profile.get())
            };
            let mut pick = button(ui_text(profile.get().to_string()).size(13))
                .style(theme::quiet_button(scheme))
                .width(30)
                .height(30);
            if is_edited {
                pick = pick.style(theme::pick_selected(scheme));
            }
            if pick_blocked.is_none() {
                pick = pick.on_press(Message::Machine(Msg::PickProfile(profile)));
            }
            container(pick).id(id).into()
        }))
        .spacing(4);
        let edit_row = row![ui_text("Edit").size(12).color(t.text_3), profiles]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        let show_make_active = edit_profile.is_some() && edit_profile != Some(snap.active);
        let mut active_area = column![
            ui_text(format!("Active profile: {}", snap.active.get()))
                .size(12)
                .color(t.text_2),
        ]
        .align_x(iced::Alignment::Start);
        if show_make_active {
            let blocked = machine.make_active_blocked();
            let mut b = button(control_label("Make active"))
                .style(theme::primary_button(scheme))
                .height(CONTROL_HEIGHT);
            if blocked.is_none() {
                b = b.on_press(Message::Machine(Msg::MakeActive));
            }
            active_area = active_area.push(b);
            if let Some(reason) = blocked {
                active_area =
                    active_area.push(ui_text(capitalize(&reason)).size(11).color(t.text_3));
            }
        }
        let active_area = active_area.spacing(4);

        let sections = column(
            [
                Section::Performance,
                Section::Lighting,
                Section::Buttons,
                Section::Device,
            ]
            .into_iter()
            .map(|section| self.section_row(scheme, section)),
        )
        .spacing(2);

        let sidebar = column![
            model_row,
            battery,
            ui_text(link).size(13).color(t.text_3),
            edit_row,
            active_area,
            rule::horizontal(1).style(theme::rule(scheme)),
            sections,
        ]
        .spacing(8)
        .padding(12);
        container(sidebar)
            .width(220)
            .style(theme::sidebar(scheme))
            .height(Length::Fill)
            .into()
    }

    fn section_row(&self, scheme: Scheme, section: Section) -> Element<'_, Message> {
        let machine = &self.machine;
        let t = scheme.tokens();
        let current = section == machine.section();
        let label = format!(
            "{}{}",
            section_label(section),
            if machine.pending(section) { " •" } else { "" }
        );
        let dot = container(space())
            .width(6)
            .height(6)
            .style(move |_theme| container::Style {
                background: Some(Background::Color(if current {
                    t.accent
                } else {
                    Color::TRANSPARENT
                })),
                border: Border {
                    radius: 999.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            });
        let row = row![
            dot,
            ui_text(label)
                .size(13)
                .color(if current { t.accent_text } else { t.text }),
        ]
        .padding([6, 10])
        .spacing(8)
        .align_y(iced::Alignment::Center);
        button(row)
            .style(theme::quiet_button(scheme))
            .on_press(Message::Machine(Msg::Select(section)))
            .width(Length::Fill)
            .into()
    }

    fn content_pane(&self, scheme: Scheme) -> Element<'_, Message> {
        // A pick empties the panes until a snapshot of the picked profile lands:
        // loading while its read runs, asleep when the read answered asleep.
        if self.machine.live_snapshot().is_none() {
            return match self.machine.connection() {
                Connection::Asleep => empty_state(
                    scheme,
                    "The mouse is asleep. Move it to wake it.",
                    String::new(),
                ),
                Connection::Loading
                | Connection::NoMouse
                | Connection::Ready
                | Connection::Failed => center(ui_text("Loading")).into(),
            };
        }
        let machine = &self.machine;
        let section = machine.section();
        let body = match section {
            Section::Performance => self.performance_pane(scheme),
            Section::Lighting => self.lighting_pane(scheme),
            Section::Buttons => self.buttons_pane(scheme),
            Section::Device => self.device_pane(scheme),
        };
        if section == Section::Device {
            return scrollable(container(body).padding([12, 16]))
                .style(theme::scrollable(scheme))
                .into();
        }
        let blocked = machine.apply_blocked();
        let nothing_pending = !machine.pending(section);
        let apply: Element<'_, Message> = {
            let mut b = button(control_label("Apply"))
                .style(theme::filled_button(scheme))
                .height(CONTROL_HEIGHT);
            if blocked.is_none() && !nothing_pending {
                b = b.on_press(Message::Machine(Msg::Apply));
            }
            b.into()
        };
        let revert: Element<'_, Message> = {
            let mut b = button(control_label("Revert"))
                .style(theme::quiet_button(scheme))
                .height(CONTROL_HEIGHT);
            if !nothing_pending {
                b = b.on_press(Message::Machine(Msg::Revert));
            }
            b.into()
        };
        let notice = machine.notice().map(|notice| match notice {
            Notice::Done(message) => ui_text(capitalize(message))
                .size(13)
                .color(scheme.tokens().success),
            Notice::Failed(message) => ui_text(capitalize(message))
                .size(13)
                .color(scheme.tokens().danger_text),
        });
        let changed = moved_lines(machine, scheme);
        let reason = blocked.map(|reason| {
            ui_text(capitalize(&reason))
                .size(13)
                .color(scheme.tokens().text_3)
        });
        let mut footer_row = row![revert, space().width(Length::Fill)]
            .spacing(12)
            .align_y(iced::Alignment::Center);
        if let Some(reason) = reason {
            footer_row = footer_row.push(reason);
        }
        footer_row = footer_row.push(apply);
        let mut footer =
            column![rule::horizontal(1).style(theme::rule(scheme)), footer_row].spacing(8);
        if let Some(notice) = notice {
            footer = footer.push(notice);
        }
        if let Some(changed) = changed {
            footer = footer.push(changed);
        }
        let footer = footer.padding([12, 16]);
        // The cards take the height the pinned footer leaves over; the footer
        // never scrolls out of the shipped window.
        column![
            scrollable(container(body).padding([12, 16]))
                .style(theme::scrollable(scheme))
                .height(Length::Fill),
            footer
        ]
        .into()
    }

    // --- Performance ----------------------------------------------------------

    fn performance_pane(&self, scheme: Scheme) -> Element<'_, Message> {
        let machine = &self.machine;
        let t = scheme.tokens();
        let stage_texts = machine.stage_texts().unwrap_or_default();
        let stage_colors = machine.stage_colors();

        let stage_rows: Vec<Element<'_, Message>> = stage_texts
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let remembered = stage_colors
                    .as_ref()
                    .and_then(|colors| colors.get(index))
                    .copied();
                let hex = machine.stage_color_hex(index);
                let hex_value = if hex.is_empty() {
                    remembered.map(parse::format_color).unwrap_or_default()
                } else {
                    hex
                };
                let mut control_children: Vec<Element<'_, Message>> = vec![
                    text_input("100-19000", value)
                        .style(theme::text_input(scheme))
                        .font(MONO_FONT)
                        .size(14)
                        .padding([9.0, 12.0])
                        .on_input(move |text| {
                            Message::Machine(Msg::Edit(Edit::DpiText(index, text)))
                        })
                        .width(110)
                        .into(),
                ];
                if let Some(color) = remembered {
                    control_children.push(color_swatch(scheme, color));
                }
                control_children.push(
                    text_input("hex", &hex_value)
                        .style(theme::text_input(scheme))
                        .font(if hex_value.is_empty() {
                            UI_FONT
                        } else {
                            MONO_FONT
                        })
                        .size(14)
                        .padding([9.0, 12.0])
                        .on_input(move |text| {
                            Message::Machine(Msg::Edit(Edit::StageColorHex(index, text)))
                        })
                        .width(80)
                        .into(),
                );
                if stage_texts.len() > 1 {
                    control_children.push(
                        button(control_label("Remove"))
                            .on_press(Message::Machine(Msg::Edit(Edit::RemoveStage(index))))
                            .style(theme::quiet_button(scheme))
                            .height(CONTROL_HEIGHT)
                            .into(),
                    );
                }
                let control = row(control_children)
                    .spacing(8)
                    .align_y(iced::Alignment::Center);
                let mut stage_tags = Vec::new();
                if remembered.is_none() {
                    stage_tags.push("Unknown");
                }
                setting_row(
                    scheme,
                    format!("Stage {}", index + 1),
                    control.into(),
                    Element::from(space()),
                    stage_tags,
                )
            })
            .collect();

        let add_stage = if stage_texts.len() < gctl_proto::DpiStages::MAX {
            Some(
                button(control_label("Add stage"))
                    .style(theme::primary_button(scheme))
                    .height(CONTROL_HEIGHT)
                    .on_press(Message::Machine(Msg::Edit(Edit::AddStage))),
            )
        } else {
            None
        };

        let active_stage: Element<'_, Message> = pick_list(
            (1..=stage_texts.len() as u8).collect::<Vec<_>>(),
            machine.active_stage(),
            |stage| Message::Machine(Msg::Edit(Edit::ActiveStage(stage))),
        )
        .placeholder("active stage")
        .style(theme::pick_list(scheme))
        .menu_style(theme::pick_menu(scheme))
        .text_size(14)
        .font(MONO_FONT)
        .padding([9.0, 12.0])
        .into();

        let polling: Element<'_, Message> = container(
            pick_list(
                POLLING_OPTIONS.as_slice(),
                machine.polling_rate().map(polling_label),
                |label| Message::Machine(Msg::Edit(polling_pick(&label))),
            )
            .style(theme::pick_list(scheme))
            .menu_style(theme::pick_menu(scheme))
            .text_size(14)
            .font(MONO_FONT)
            .padding([9.0, 12.0]),
        )
        .id("polling-picker")
        .into();

        let lift_off: Element<'_, Message> = container(
            pick_list(
                LIFT_OFF_OPTIONS.as_slice(),
                machine.lift_off().map(lift_off_label),
                |label| Message::Machine(Msg::Edit(lift_off_pick(&label))),
            )
            .style(theme::pick_list(scheme))
            .menu_style(theme::pick_menu(scheme))
            .text_size(14)
            .font(MONO_FONT)
            .padding([9.0, 12.0]),
        )
        .id("lift-off-picker")
        .into();

        let debounce = machine.debounce().unwrap_or(0);
        let debounce_control: Element<'_, Message> = slider(0..=16, debounce, |ms| {
            Message::Machine(Msg::Edit(Edit::Debounce(ms)))
        })
        .style(theme::slider(scheme))
        .width(Length::Fill)
        .height(CONTROL_HEIGHT)
        .into();
        let debounce_readout: Element<'_, Message> =
            ui_text(format!("{debounce} ms")).font(MONO_FONT).into();

        let never = machine.sleep_never().unwrap_or(false);
        let minutes = text_input("minutes", &machine.sleep_minutes().unwrap_or_default())
            .style(theme::text_input(scheme))
            .font(MONO_FONT)
            .size(14)
            .padding([9.0, 12.0])
            .width(60)
            .id("sleep-minutes");
        let minutes: Element<'_, Message> = if never {
            minutes.into()
        } else {
            minutes
                .on_input(|text| Message::Machine(Msg::Edit(Edit::SleepMinutes(text))))
                .into()
        };
        let seconds = text_input("seconds", &machine.sleep_seconds().unwrap_or_default())
            .style(theme::text_input(scheme))
            .font(MONO_FONT)
            .size(14)
            .padding([9.0, 12.0])
            .width(60)
            .id("sleep-seconds");
        let seconds: Element<'_, Message> = if never {
            seconds.into()
        } else {
            seconds
                .on_input(|text| Message::Machine(Msg::Edit(Edit::SleepSeconds(text))))
                .into()
        };
        let sleep_row: Element<'_, Message> = row![
            checkbox(never)
                .style(theme::checkbox(scheme))
                .on_toggle(|never| Message::Machine(Msg::Edit(Edit::SleepNever(never)))),
            ui_text("Never").size(14),
            minutes,
            ui_text("m").size(13).color(t.text_3),
            seconds,
            ui_text("s").size(13).color(t.text_3),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into();

        let remembered_dpi_colors = machine
            .snapshot()
            .and_then(|snap| snap.dpi_colors.as_ref())
            .is_some();
        let mut dpi_tags = vec!["per profile"];
        if remembered_dpi_colors {
            dpi_tags.push("last set by gctl");
        }

        let mut dpi_body = column![
            setting_row(
                scheme,
                "DPI stages",
                Element::from(space()),
                Element::from(space()),
                dpi_tags
            ),
            column(stage_rows).spacing(6),
        ]
        .spacing(10);
        if let Some(add) = add_stage {
            dpi_body = dpi_body.push(setting_row(
                scheme,
                "",
                add.into(),
                Element::from(space()),
                Vec::new(),
            ));
        }
        dpi_body = dpi_body.push(setting_row(
            scheme,
            "Active stage",
            active_stage,
            Element::from(space()),
            vec!["per profile"],
        ));
        let dpi_card = card(scheme, "DPI", dpi_body);

        let sensor_card = card(
            scheme,
            "Sensor",
            column![
                setting_row(
                    scheme,
                    "Polling rate",
                    polling,
                    Element::from(space()),
                    vec!["all profiles"]
                ),
                setting_row(
                    scheme,
                    "Lift-off",
                    lift_off,
                    Element::from(space()),
                    vec!["all profiles"]
                ),
                setting_row(
                    scheme,
                    "Debounce",
                    debounce_control,
                    debounce_readout,
                    vec!["per profile"]
                ),
                setting_row(
                    scheme,
                    "Sleep",
                    sleep_row,
                    Element::from(space()),
                    vec!["all profiles"]
                ),
            ]
            .spacing(10),
        );

        column![dpi_card, sensor_card].spacing(16).into()
    }

    // --- Lighting -------------------------------------------------------------

    fn lighting_pane(&self, scheme: Scheme) -> Element<'_, Message> {
        let machine = &self.machine;
        let kind = machine.effect_kind();

        let effect_picker = pick_list(EffectKind::ALL, kind, |kind| {
            Message::Machine(Msg::Edit(Edit::Effect(kind)))
        })
        .style(theme::pick_list(scheme))
        .menu_style(theme::pick_menu(scheme))
        .text_size(14)
        .padding([9.0, 12.0]);
        // An unknown effect reads `Unknown` as a tag in the tag column, so a
        // selector can count it, never as an uncountable picker placeholder.
        let effect_row: Element<'_, Message> = effect_picker.into();

        let takes_rate = matches!(
            kind,
            Some(
                EffectKind::Glorious
                    | EffectKind::Cycle
                    | EffectKind::Pulse
                    | EffectKind::PulseOne
                    | EffectKind::Tail
                    | EffectKind::Rave
                    | EffectKind::Wave
            )
        );
        let takes_colors = matches!(
            kind,
            Some(EffectKind::Pulse | EffectKind::Rave | EffectKind::Solid | EffectKind::PulseOne)
        );

        let rate_row: Option<(Element<'_, Message>, Element<'_, Message>)> = if takes_rate {
            let rate = machine.effect_rate().unwrap_or(0);
            Some((
                slider(0..=100, rate, |rate| {
                    Message::Machine(Msg::Edit(Edit::Rate(rate)))
                })
                .style(theme::slider(scheme))
                .width(Length::Fill)
                .height(CONTROL_HEIGHT)
                .into(),
                ui_text(format!("{rate}")).font(MONO_FONT).into(),
            ))
        } else {
            None
        };

        let color_rows: Vec<Element<'_, Message>> = if takes_colors {
            let colors = machine.effect_colors().unwrap_or_default();
            let max = match kind {
                Some(EffectKind::Pulse) => 6usize,
                Some(EffectKind::Rave) => 2usize,
                Some(
                    EffectKind::Off
                    | EffectKind::Glorious
                    | EffectKind::Cycle
                    | EffectKind::Solid
                    | EffectKind::PulseOne
                    | EffectKind::Tail
                    | EffectKind::Wave,
                )
                | None => 1usize,
            };
            let mut rows: Vec<Element<'_, Message>> = (0..colors.len())
                .map(|index| {
                    let remembered = colors[index];
                    let hex = machine.effect_color_hex(index);
                    let hex_value = if hex.is_empty() {
                        parse::format_color(remembered)
                    } else {
                        hex
                    };
                    let remove = if colors.len() > min_effect_colors(kind) {
                        Some(
                            button(control_label("Remove"))
                                .on_press(Message::Machine(Msg::Edit(Edit::RemoveEffectColor(
                                    index,
                                ))))
                                .style(theme::quiet_button(scheme))
                                .height(CONTROL_HEIGHT),
                        )
                    } else {
                        None
                    };
                    let mut control = row![
                        color_swatch(scheme, remembered),
                        text_input("hex", &hex_value)
                            .style(theme::text_input(scheme))
                            .font(if hex_value.is_empty() {
                                UI_FONT
                            } else {
                                MONO_FONT
                            })
                            .size(14)
                            .padding([9.0, 12.0])
                            .on_input(move |text| {
                                Message::Machine(Msg::Edit(Edit::EffectColorHex(index, text)))
                            })
                            .width(80),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center);
                    if let Some(remove) = remove {
                        control = control.push(remove);
                    }
                    control.into()
                })
                .collect();
            if colors.len() < max {
                rows.push(
                    button(control_label("Add colour"))
                        .style(theme::primary_button(scheme))
                        .height(CONTROL_HEIGHT)
                        .on_press(Message::Machine(Msg::Edit(Edit::AddEffectColor)))
                        .into(),
                );
            }
            rows
        } else {
            Vec::new()
        };

        let (wired_control, wired_readout, wired_unknown) = brightness_row(
            scheme,
            "wired-slider",
            machine.brightness_wired(),
            |value| Message::Machine(Msg::Edit(Edit::BrightnessWired(value))),
            Edit::BrightnessWiredRelease,
        );
        let (wireless_control, wireless_readout, wireless_unknown) = brightness_row(
            scheme,
            "wireless-slider",
            machine.brightness_wireless(),
            |value| Message::Machine(Msg::Edit(Edit::BrightnessWireless(value))),
            Edit::BrightnessWirelessRelease,
        );

        let remembered_effect = machine.snapshot().and_then(|snap| snap.effect.as_ref());
        let remembered_brightness = machine.snapshot().and_then(|snap| snap.brightness);

        let mut effect_tags = vec!["per profile"];
        if remembered_effect.is_some() {
            effect_tags.push("last set by gctl");
        }
        if kind.is_none() {
            effect_tags.push("Unknown");
        }
        let mut brightness_tags = vec!["all profiles"];
        if remembered_brightness.is_some() {
            brightness_tags.push("last set by gctl");
        }
        let mut wired_tags = brightness_tags.clone();
        if wired_unknown {
            wired_tags.push("Unknown");
        }
        let mut wireless_tags = brightness_tags;
        if wireless_unknown {
            wireless_tags.push("Unknown");
        }

        let mut effect_body = column![setting_row(
            scheme,
            "Effect",
            effect_row,
            Element::from(space()),
            effect_tags
        )]
        .spacing(10);
        if let Some((rate_control, rate_readout)) = rate_row {
            effect_body = effect_body.push(setting_row(
                scheme,
                "Speed",
                rate_control,
                rate_readout,
                Vec::new(),
            ));
        }
        if takes_colors {
            effect_body = effect_body.push(setting_row(
                scheme,
                "Colours",
                column(color_rows).spacing(6).into(),
                Element::from(space()),
                Vec::new(),
            ));
        }
        let effect_card = card(scheme, "Effect", effect_body);

        let brightness_card = card(
            scheme,
            "Brightness",
            column![
                setting_row(scheme, "Wired", wired_control, wired_readout, wired_tags),
                setting_row(
                    scheme,
                    "Wireless",
                    wireless_control,
                    wireless_readout,
                    wireless_tags
                ),
            ]
            .spacing(10),
        );

        column![effect_card, brightness_card].spacing(16).into()
    }

    // --- Buttons --------------------------------------------------------------

    fn buttons_pane(&self, scheme: Scheme) -> Element<'_, Message> {
        let machine = &self.machine;
        let rows: Vec<Element<'_, Message>> = crate::get::BUTTONS
            .iter()
            .map(|button| self.bind_row(scheme, *button))
            .collect();

        let scroll = machine.scroll();
        let remembered_scroll = machine.snapshot().and_then(|snap| snap.scroll);
        // The `last set by gctl` and `Unknown` marks sit in the tag column; the
        // dimming follows what is shown (the overlay), so a picked direction
        // drops the `Unknown` mark even before Apply, like the brightness rows.
        let mut scroll_tags = vec!["all profiles"];
        match remembered_scroll {
            Some(_) => scroll_tags.push("last set by gctl"),
            None if scroll.is_none() => scroll_tags.push("Unknown"),
            None => {}
        }
        let invert = scroll == Some(gctl_proto::ScrollDirection::Invert);
        let toggler = toggler(invert)
            .style(theme::toggler(scheme))
            .on_toggle(|invert| Message::Machine(Msg::Edit(Edit::Scroll(scroll_of(invert)))));
        // Unknown draws dimmed, like the unknown brightness sliders, so an OFF
        // toggler never reads as `default`.
        let toggler: Element<'_, Message> = if scroll.is_none() {
            toggler.style(theme::dim_toggler(scheme)).into()
        } else {
            toggler.into()
        };

        let scroll_row: Element<'_, Message> = container(
            row![toggler, ui_text("Invert scroll").size(14)]
                .spacing(8)
                .align_y(iced::Alignment::Center),
        )
        .center_y(CONTROL_HEIGHT)
        .into();

        let scroll_card = card(
            scheme,
            "Scroll",
            setting_row(
                scheme,
                "Scroll",
                scroll_row,
                Element::from(space()),
                scroll_tags,
            ),
        );
        let buttons_card = card(scheme, "Buttons", column(rows).spacing(8));

        column![scroll_card, buttons_card].spacing(16).into()
    }

    fn bind_row(&self, scheme: Scheme, button: Button) -> Element<'_, Message> {
        let machine = &self.machine;
        let t = scheme.tokens();
        let binding = machine.bind(button);
        let label = button_label(button);
        let remembered_bind = machine.snapshot().and_then(|snap| snap.bind(button));
        let category = binding_category(binding);

        let category_picker = pick_list(
            parse::BIND_FAMILY_TOKENS,
            category.map(parse::family_token),
            move |label| {
                let edit = match parse::bind_category(label) {
                    Some(category) => default_binding(button, category),
                    None => Edit::InvalidBind(button),
                };
                Message::Machine(Msg::Edit(edit))
            },
        )
        .style(theme::pick_list(scheme))
        .menu_style(theme::pick_menu(scheme))
        .text_size(14)
        .padding([9.0, 12.0]);
        // An unknown bind reads `Unknown` as a tag in the tag column, so a
        // selector can count it, never as an uncountable picker placeholder.
        let category_cell: Element<'_, Message> = category_picker.into();

        let control: Element<'_, Message> = match category {
            None | Some(parse::BindCategory::None) => row![category_cell]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .into(),
            Some(parse::BindCategory::Mouse) => row![
                category_cell,
                fn_picker(
                    scheme,
                    &parse::MOUSE_FN_NAMES,
                    parse::BindCategory::Mouse,
                    binding,
                    button,
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into(),
            Some(parse::BindCategory::Media) => row![
                category_cell,
                fn_picker(
                    scheme,
                    &parse::MEDIA_FN_NAMES,
                    parse::BindCategory::Media,
                    binding,
                    button,
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into(),
            Some(parse::BindCategory::Dpi) => row![
                category_cell,
                fn_picker(
                    scheme,
                    &parse::DPI_FN_NAMES,
                    parse::BindCategory::Dpi,
                    binding,
                    button,
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into(),
            Some(parse::BindCategory::Keyboard) => row![
                category_cell,
                fn_picker(
                    scheme,
                    &parse::KEYBOARD_FN_NAMES,
                    parse::BindCategory::Keyboard,
                    binding,
                    button,
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into(),
            Some(parse::BindCategory::Key) => {
                // The Key category is only shown for a key bind, so the binding
                // is always a key here; render the category cell alone if not.
                let Some(Binding::Key { key, modifiers }) = binding else {
                    return setting_row(
                        scheme,
                        label,
                        row![category_cell]
                            .spacing(8)
                            .align_y(iced::Alignment::Center)
                            .into(),
                        Element::from(space()),
                        Vec::new(),
                    );
                };
                let key_names = key_names();
                let current_key = Some(key.name());
                // The modifiers sit on a second line under the pickers, so the
                // widest (`meta-right`) never wraps and the key picker keeps its
                // full width.
                let toggles: Vec<Element<'static, Message>> = parse::MODIFIERS
                    .iter()
                    .copied()
                    .map(|(bit, name)| {
                        let on = modifiers.bits() & bit.bits() != 0;
                        row![
                            checkbox(on)
                                .style(theme::checkbox(scheme))
                                .on_toggle(move |on| {
                                    let mut rebuilt = gctl_proto::Modifiers::NONE;
                                    for (b, n) in parse::MODIFIERS {
                                        let is_set = if n == name {
                                            on
                                        } else {
                                            modifiers.bits() & b.bits() != 0
                                        };
                                        if is_set {
                                            rebuilt = rebuilt | b;
                                        }
                                    }
                                    Message::Machine(Msg::Edit(key_edit(button, key, rebuilt)))
                                }),
                            ui_text(name).size(11).color(t.text_4),
                        ]
                        .spacing(2)
                        .align_y(iced::Alignment::Center)
                        .into()
                    })
                    .collect();
                let key_picker = pick_list(key_names, current_key, move |name| {
                    let edit = match gctl_proto::Key::from_name(name) {
                        Ok(key) => key_edit(button, key, modifiers),
                        Err(_) => Edit::InvalidBind(button),
                    };
                    Message::Machine(Msg::Edit(edit))
                })
                .style(theme::pick_list(scheme))
                .menu_style(theme::pick_menu(scheme))
                .text_size(14)
                .padding([9.0, 12.0])
                .width(KEY_PICKER_WIDTH);
                column![
                    row![category_cell, key_picker]
                        .spacing(8)
                        .align_y(iced::Alignment::Center),
                    iced::widget::row(toggles).spacing(6),
                ]
                .spacing(6)
                .align_x(iced::Alignment::Start)
                .into()
            }
        };

        let mut tags = Vec::new();
        if remembered_bind.is_some() {
            tags.push("last set by gctl");
        }
        if category.is_none() {
            tags.push("Unknown");
        }
        setting_row(scheme, label, control, Element::from(space()), tags)
    }

    // --- Device ---------------------------------------------------------------

    fn device_pane(&self, scheme: Scheme) -> Element<'_, Message> {
        let machine = &self.machine;
        let t = scheme.tokens();
        let Some(snap) = machine.live_snapshot() else {
            return center(ui_text("Loading")).into();
        };
        let link = match snap.model.link {
            gctl_proto::Link::Wired => "Wired",
            gctl_proto::Link::Wireless => "Wireless",
        };
        let firmware = snap.firmware.to_string();
        let battery_control: Element<'_, Message> = match snap.battery.percent {
            Some(percent) => container(ui_text(format!("{percent}%")).size(14).font(MONO_FONT))
                .center_y(CONTROL_HEIGHT)
                .into(),
            None => Element::from(space()),
        };
        let battery_tags = if snap.battery.percent.is_none() {
            vec!["Unknown"]
        } else {
            Vec::new()
        };

        let profile = snap.profile.get();
        let blocked = machine.file_blocked();
        let export_button: Element<'_, Message> = {
            let mut b = button(file_button_label(format!("Export profile {profile}")))
                .style(theme::primary_button(scheme))
                .width(FILE_BUTTON_WIDTH)
                .height(CONTROL_HEIGHT);
            if blocked.is_none() {
                b = b.on_press(Message::ExportClicked);
            }
            b.into()
        };
        let import_button: Element<'_, Message> = {
            let mut b = button(file_button_label(format!("Import into profile {profile}")))
                .style(theme::primary_button(scheme))
                .width(FILE_BUTTON_WIDTH)
                .height(CONTROL_HEIGHT);
            if blocked.is_none() {
                b = b.on_press(Message::ImportClicked);
            }
            b.into()
        };
        let export_row: Element<'_, Message> = row![
            text_input("path.toml", &self.export_path)
                .style(theme::text_input(scheme))
                .size(14)
                .padding([9.0, 12.0])
                .on_input(Message::ExportPath)
                .width(Length::Fill),
            export_button,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into();
        let import_row: Element<'_, Message> = row![
            text_input("path.toml", &self.import_path)
                .style(theme::text_input(scheme))
                .size(14)
                .padding([9.0, 12.0])
                .on_input(Message::ImportPath)
                .width(Length::Fill),
            import_button,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into();

        let notice = machine.notice().map(|notice| match notice {
            Notice::Done(message) => ui_text(capitalize(message)).size(13).color(t.success),
            Notice::Failed(message) => ui_text(capitalize(message)).size(13).color(t.danger_text),
        });
        let changed = moved_lines(machine, scheme);
        let reason = blocked.map(|reason| ui_text(capitalize(&reason)).size(13).color(t.text_3));

        let mouse_card = card(
            scheme,
            "Mouse",
            column![
                setting_row(
                    scheme,
                    "Model",
                    container(ui_text(snap.model.name).size(14))
                        .center_y(CONTROL_HEIGHT)
                        .into(),
                    Element::from(space()),
                    Vec::new()
                ),
                setting_row(
                    scheme,
                    "Link",
                    container(ui_text(link).size(14))
                        .center_y(CONTROL_HEIGHT)
                        .into(),
                    Element::from(space()),
                    Vec::new()
                ),
                setting_row(
                    scheme,
                    "Firmware",
                    container(ui_text(firmware).size(14).font(MONO_FONT))
                        .center_y(CONTROL_HEIGHT)
                        .into(),
                    Element::from(space()),
                    Vec::new()
                ),
                setting_row(
                    scheme,
                    "Battery",
                    battery_control,
                    Element::from(space()),
                    battery_tags,
                ),
            ]
            .spacing(8),
        );

        let mut profile_body = column![
            setting_row(
                scheme,
                "Export profile",
                export_row,
                Element::from(space()),
                Vec::new()
            ),
            setting_row(
                scheme,
                "Import profile",
                import_row,
                Element::from(space()),
                Vec::new()
            ),
        ]
        .spacing(8);
        // An absent reason or notice takes no space; a present one is a real
        // line the legend flows under (main_view adds the banner the same way).
        if let Some(reason) = reason {
            profile_body = profile_body.push(reason);
        }
        if let Some(notice) = notice {
            profile_body = profile_body.push(notice);
        }
        if let Some(changed) = changed {
            profile_body = profile_body.push(changed);
        }
        let profile_card = card(
            scheme,
            "Profile file",
            profile_body.push(
                ui_text(
                    "Last set by gctl: the mouse cannot report this value, so gctl shows the last one it wrote. Another program or a reset may have changed it since."
                )
                .size(13)
                .color(t.text_4),
            ),
        );

        column![mouse_card, profile_card].spacing(16).into()
    }
}

// --- small view helpers --------------------------------------------------------

/// The one draw-site helper for state-produced copy: it capitalizes the first
/// letter of every line, so a banner or notice joining two failures never draws
/// a later line lowercase. A line that starts with a quote or a digit is left
/// as it is. The state keeps the CLI's lowercase text.
fn capitalize(text: &str) -> String {
    text.lines()
        .map(|line| {
            let Some(first) = line.chars().next() else {
                return line.to_owned();
            };
            if first.is_ascii_digit() || first == '"' {
                return line.to_owned();
            }
            let mut out = first.to_uppercase().collect::<String>();
            out.push_str(&line[first.len_utf8()..]);
            out
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The moved lines, one warning-tone widget in their own slot under the
/// notice: the footer's, and the Device pane's profile card. `None` when the
/// last read moved nothing under a live entry.
fn moved_lines(machine: &State, scheme: Scheme) -> Option<iced::widget::Text<'static>> {
    let lines = machine.changed();
    if lines.is_empty() {
        return None;
    }
    Some(
        ui_text(capitalize(&lines.join("\n")))
            .size(13)
            .color(scheme.tokens().warning),
    )
}

/// Body text in the UI font.
fn ui_text<'a>(content: impl iced::widget::text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(content).font(UI_FONT)
}

/// A control button's label, vertically centred in the shared control height.
/// Its width stays Shrink, so the button hugs its label.
fn control_label<'a>(content: impl iced::widget::text::IntoFragment<'a>) -> Element<'a, Message> {
    container(ui_text(content).size(14))
        .center_y(Length::Fill)
        .into()
}

/// A fixed-width file button's label, centred in both axes. Only safe inside a
/// button with an explicit width (Export / Import at `FILE_BUTTON_WIDTH`): a
/// `Fill`-width label inside a Shrink-width button would stretch it to fill
/// its parent, which is why this is separate from `control_label`.
fn file_button_label<'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
) -> Element<'a, Message> {
    container(ui_text(content).size(14))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn empty_state(scheme: Scheme, heading: &'static str, hint: String) -> Element<'static, Message> {
    let mut column = column![ui_text(heading).size(24)]
        .spacing(8)
        .align_x(iced::Alignment::Center);
    if !hint.is_empty() {
        column = column.push(ui_text(hint).size(14).color(scheme.tokens().text_3));
    }
    center(column)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// One setting card: a title over its rows, surface-1 with a 1px line.
fn card<'a>(
    scheme: Scheme,
    title: &'static str,
    body: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(column![ui_text(title).size(18), body.into()].spacing(12))
        .style(theme::card(scheme))
        .padding(16)
        .width(Length::Fill)
        .into()
}

/// The fixed widths of the four setting-grid columns: label, readout and tags
/// are fixed, the control fills the space between label and readout. The same
/// widths on every pane, so every label starts at one x, every slider ends at
/// one x and every tag row ends at the card's right edge (gui.md §Window).
const LABEL_COLUMN: f32 = 110.0;
const READOUT_COLUMN: f32 = 56.0;
const TAG_COLUMN: f32 = 180.0;

/// The bind pickers' shared fixed width, wide enough for the longest name any
/// of them offers (`profile-cycle-down`, wider than the longest key name
/// `NumpadSubtract`) in full, so a bind never crushes its picker.
const KEY_PICKER_WIDTH: f32 = 160.0;

/// The Export and Import buttons share this width, so both path inputs end at
/// one x.
const FILE_BUTTON_WIDTH: f32 = 170.0;

/// The one setting-row layout, a grid `label | control | readout | tags`: the
/// control column fills the space between the fixed label and readout columns,
/// and the tag column is pinned to the card's right edge. A control narrower
/// than its column sits at the column's left edge.
fn setting_row<'a>(
    scheme: Scheme,
    label: impl iced::widget::text::IntoFragment<'a>,
    control: Element<'a, Message>,
    readout: Element<'a, Message>,
    tags: Vec<&'static str>,
) -> Element<'a, Message> {
    let t = scheme.tokens();
    let label = container(ui_text(label).size(14).color(t.text_2).width(LABEL_COLUMN))
        .center_y(CONTROL_HEIGHT);
    let control = container(control).width(Length::Fill);
    let readout = container(readout)
        .width(READOUT_COLUMN)
        .center_y(CONTROL_HEIGHT);
    let tags = container(row(tags.into_iter().map(|mark| tag(scheme, mark))).spacing(4))
        .width(TAG_COLUMN)
        .align_x(iced::Alignment::End)
        .center_y(CONTROL_HEIGHT);
    row![label, control, readout, tags]
        .width(Length::Fill)
        .spacing(8)
        .align_y(iced::Alignment::Start)
        .into()
}

/// A neutral mark tag: text-3 on a quiet surface tint, pill radius.
fn tag(scheme: Scheme, mark: &'static str) -> Element<'static, Message> {
    container(ui_text(mark).size(11))
        .style(theme::tag(scheme))
        .padding([2.0, 8.0])
        .into()
}

/// A warning-tone tag, for the `Untested` badge.
fn warning_tag(scheme: Scheme, mark: &'static str) -> Element<'static, Message> {
    container(ui_text(mark).size(11).color(scheme.tokens().warning))
        .style(theme::tag_warning(scheme))
        .padding([2.0, 8.0])
        .into()
}

fn opaque_modal(scheme: Scheme, content: Element<'static, Message>) -> Element<'static, Message> {
    let card = container(content)
        .style(theme::modal(scheme))
        .padding(24)
        .max_width(480);
    let centered = center(card).width(Length::Fill).height(Length::Fill);
    let scrim = container(space())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::scrim(scheme));
    let overlay: Element<'static, Message> = stack![scrim, centered].into();
    iced::widget::opaque(overlay)
}

fn discard_prompt(scheme: Scheme, prompt: Prompt, current: Section) -> Element<'static, Message> {
    let message = match prompt {
        // Name the section being LEFT: it is the one whose edits are lost.
        Prompt::Discard(Leave::Section(_)) => {
            format!("Discard edits to {}?", section_label(current))
        }
        Prompt::Discard(Leave::Profile(profile)) => {
            format!("Discard edits and switch to profile {}?", profile.get())
        }
        Prompt::Import(_) => String::new(),
    };
    column![
        ui_text(message).size(20),
        row![
            button(control_label("Keep editing"))
                .on_press(Message::Machine(Msg::KeepEditing))
                .style(theme::secondary_button(scheme))
                .height(CONTROL_HEIGHT),
            button(control_label("Discard"))
                .on_press(Message::Machine(Msg::Discard))
                .style(theme::danger_button(scheme))
                .height(CONTROL_HEIGHT),
        ]
        .spacing(12),
    ]
    .spacing(12)
    .into()
}

fn import_prompt(scheme: Scheme, prompt: Prompt) -> Element<'static, Message> {
    let Prompt::Import(check) = prompt else {
        return space().into();
    };
    let t = scheme.tokens();
    let mut column = column![ui_text("Import profile?").size(20)].spacing(12);
    if let Some(note) = check.note {
        column = column.push(ui_text(capitalize(&note)).size(14).color(t.warning));
    }
    if check.device_wide {
        column = column.push(
            ui_text(
                "This overwrites device-wide settings (polling rate, lift-off, sleep, brightness, scroll).",
            )
            .size(14)
            .color(t.text_2),
        );
    }
    column = column.push(
        row![
            button(control_label("Cancel"))
                .on_press(Message::Machine(Msg::ImportCancel))
                .style(theme::secondary_button(scheme))
                .height(CONTROL_HEIGHT),
            button(control_label("Import"))
                .on_press(Message::Machine(Msg::ImportConfirm))
                .style(theme::primary_button(scheme))
                .height(CONTROL_HEIGHT),
        ]
        .spacing(12),
    );
    column.into()
}

fn section_label(section: Section) -> &'static str {
    match section {
        Section::Performance => "Performance",
        Section::Lighting => "Lighting",
        Section::Buttons => "Buttons",
        Section::Device => "Device",
    }
}

/// The polling rates the picker offers, low to high.
const POLLING_RATES: [gctl_proto::PollingRate; 4] = [
    gctl_proto::PollingRate::Hz125,
    gctl_proto::PollingRate::Hz250,
    gctl_proto::PollingRate::Hz500,
    gctl_proto::PollingRate::Hz1000,
];

/// The lift-off distances the picker offers.
const LIFT_OFFS: [gctl_proto::LiftOff; 2] = [gctl_proto::LiftOff::Mm1, gctl_proto::LiftOff::Mm2];

/// The polling-rate labels the picker offers, low to high, built through the
/// parse formatter so the picker cannot drift from the CLI's grammar.
static POLLING_OPTIONS: std::sync::LazyLock<Vec<String>> =
    std::sync::LazyLock::new(|| POLLING_RATES.into_iter().map(polling_label).collect());

/// The lift-off labels the picker offers, built through the parse formatter.
static LIFT_OFF_OPTIONS: std::sync::LazyLock<Vec<String>> =
    std::sync::LazyLock::new(|| LIFT_OFFS.into_iter().map(lift_off_label).collect());

fn polling_label(rate: gctl_proto::PollingRate) -> String {
    format!("{} Hz", parse::format_polling_rate(rate))
}

fn lift_off_label(lift_off: gctl_proto::LiftOff) -> String {
    format!("{} mm", parse::format_lift_off(lift_off))
}

/// The rate a picker label names, from the same list the picker offers.
fn polling_rate_of(label: &str) -> Option<gctl_proto::PollingRate> {
    POLLING_RATES
        .iter()
        .copied()
        .find(|rate| polling_label(*rate) == label)
}

/// The distance a picker label names, from the same list the picker offers.
fn lift_off_of(label: &str) -> Option<gctl_proto::LiftOff> {
    LIFT_OFFS
        .iter()
        .copied()
        .find(|lift_off| lift_off_label(*lift_off) == label)
}

/// The edit a polling-rate picker option produces: its rate, or an invalid
/// entry when the label does not round-trip.
fn polling_pick(label: &str) -> Edit {
    match polling_rate_of(label) {
        Some(rate) => Edit::PollingRate(rate),
        None => Edit::InvalidPollingRate,
    }
}

/// The edit a lift-off picker option produces: its distance, or an invalid
/// entry when the label does not round-trip.
fn lift_off_pick(label: &str) -> Edit {
    match lift_off_of(label) {
        Some(lift_off) => Edit::LiftOff(lift_off),
        None => Edit::InvalidLiftOff,
    }
}

fn scroll_of(invert: bool) -> gctl_proto::ScrollDirection {
    if invert {
        gctl_proto::ScrollDirection::Invert
    } else {
        gctl_proto::ScrollDirection::Default
    }
}

fn color_swatch(scheme: Scheme, color: gctl_proto::Rgb) -> Element<'static, Message> {
    let line = scheme.tokens().line;
    container(space())
        .width(18)
        .height(18)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(Color::from_rgb8(
                color.r, color.g, color.b,
            ))),
            border: Border {
                radius: 4.0.into(),
                width: 1.0,
                color: line,
            },
            ..container::Style::default()
        })
        .into()
}

fn min_effect_colors(kind: Option<EffectKind>) -> usize {
    match kind {
        Some(EffectKind::Pulse) => 2,
        Some(
            EffectKind::Off
            | EffectKind::Glorious
            | EffectKind::Cycle
            | EffectKind::Rave
            | EffectKind::Solid
            | EffectKind::PulseOne
            | EffectKind::Tail
            | EffectKind::Wave,
        )
        | None => 1,
    }
}

fn fn_picker(
    scheme: Scheme,
    names: &'static [&'static str],
    family: parse::BindCategory,
    binding: Option<Binding>,
    button: Button,
) -> Element<'static, Message> {
    let family_token = parse::family_token(family);
    let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    let current = binding.and_then(|binding| {
        let text = parse::format_binding(binding);
        let name = text.strip_prefix(family_token)?.strip_prefix(':')?;
        Some(name.to_owned())
    });
    pick_list(names, current, move |name| {
        let edit = bind_edit(button, &format!("{family_token}:{name}"));
        Message::Machine(Msg::Edit(edit))
    })
    .style(theme::pick_list(scheme))
    .menu_style(theme::pick_menu(scheme))
    .text_size(14)
    .padding([9.0, 12.0])
    .width(KEY_PICKER_WIDTH)
    .into()
}

/// The edit a picker composition produces: a valid bind, or `InvalidBind` when
/// the composed text fails to parse. The latter blocks Apply; it never panics.
fn bind_edit(button: Button, text: &str) -> Edit {
    match parse::binding(text) {
        Ok(binding) => Edit::Bind(button, binding),
        Err(_) => Edit::InvalidBind(button),
    }
}

/// A key bind composed through `parse::format_binding` and `parse::binding`,
/// so the key and modifier names have one source.
fn key_edit(button: Button, key: gctl_proto::Key, modifiers: gctl_proto::Modifiers) -> Edit {
    bind_edit(
        button,
        &parse::format_binding(Binding::Key { key, modifiers }),
    )
}

fn binding_category(binding: Option<Binding>) -> Option<parse::BindCategory> {
    match binding {
        None => None,
        Some(Binding::None) => Some(parse::BindCategory::None),
        Some(Binding::Mouse(_)) => Some(parse::BindCategory::Mouse),
        Some(Binding::Media(_)) => Some(parse::BindCategory::Media),
        Some(Binding::Dpi(_)) => Some(parse::BindCategory::Dpi),
        Some(Binding::Keyboard(_)) => Some(parse::BindCategory::Keyboard),
        Some(Binding::Key { .. }) => Some(parse::BindCategory::Key),
    }
}

/// The edit a category pick produces: the family's default binding, or an
/// invalid entry when the key bind's default text fails to parse.
fn default_binding(button: Button, category: parse::BindCategory) -> Edit {
    match category {
        parse::BindCategory::None => Edit::Bind(button, Binding::None),
        parse::BindCategory::Mouse => Edit::Bind(button, Binding::Mouse(gctl_proto::MouseFn::Left)),
        parse::BindCategory::Media => Edit::Bind(button, Binding::Media(gctl_proto::MediaFn::Mute)),
        parse::BindCategory::Dpi => Edit::Bind(button, Binding::Dpi(gctl_proto::DpiFn::StageUp)),
        parse::BindCategory::Keyboard => Edit::Bind(
            button,
            Binding::Keyboard(gctl_proto::KeyboardFn::ProfileCycleUp),
        ),
        parse::BindCategory::Key => bind_edit(button, "key:KeyA"),
    }
}

fn key_names() -> Vec<&'static str> {
    gctl_proto::Key::all().map(gctl_proto::Key::name).collect()
}

/// One brightness row's grid cells: a dimmed slider with its handle at 0 and
/// `Unknown` in the tag column until the link is known, then a live slider and
/// the number readout. The unknown slider sends `release` on release, so a
/// click at 0 can set the link to 0 while a click that picked a value keeps it.
/// The slider carries `id`, so a test can find its bounds by layout id rather
/// than by the label.
fn brightness_row(
    scheme: Scheme,
    id: &'static str,
    value: Option<u8>,
    on_change: impl Fn(u8) -> Message + 'static,
    release: Edit,
) -> (Element<'static, Message>, Element<'static, Message>, bool) {
    match value {
        Some(value) => {
            let slider = container(
                slider(0..=255, value, on_change)
                    .width(Length::Fill)
                    .height(CONTROL_HEIGHT)
                    .style(theme::slider(scheme)),
            )
            .width(Length::Fill)
            .id(id);
            (
                slider.into(),
                ui_text(format!("{value}")).font(MONO_FONT).into(),
                false,
            )
        }
        None => {
            let slider = container(
                slider(0..=255, 0, on_change)
                    .width(Length::Fill)
                    .height(CONTROL_HEIGHT)
                    .style(theme::dim_slider(scheme))
                    .on_release(Message::Machine(Msg::Edit(release))),
            )
            .width(Length::Fill)
            .id(id);
            (slider.into(), Element::from(space()), true)
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/gui_view.rs"]
mod tests;
