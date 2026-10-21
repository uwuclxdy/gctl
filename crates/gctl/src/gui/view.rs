//! The iced configurator window over [`state`] and [`io`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use iced::futures::StreamExt;
use iced::futures::channel::mpsc;
use iced::theme::palette::Palette;
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
use crate::memory::Memory;
use crate::parse;
use crate::writes::WriteOp;

// --- palette (dark block, converted from oklch by computation) -----------------

// --bg: oklch(0.255 0.024 280)
const BG: Color = Color::from_rgb8(0x20, 0x22, 0x2E);
// --well: oklch(0.215 0.020 280)
const WELL: Color = Color::from_rgb8(0x17, 0x19, 0x23);
// --surface-1: oklch(0.300 0.024 280)
const SURFACE_1: Color = Color::from_rgb8(0x2B, 0x2D, 0x3A);
// --surface-2: oklch(0.345 0.026 280)
const SURFACE_2: Color = Color::from_rgb8(0x36, 0x38, 0x47);
// --surface-3: oklch(0.395 0.028 280)
const SURFACE_3: Color = Color::from_rgb8(0x43, 0x45, 0x55);
// --text: oklch(0.900 0.030 280)
const TEXT: Color = Color::from_rgb8(0xDA, 0xDC, 0xF2);
// --text-2: oklch(0.800 0.030 280)
const TEXT_2: Color = Color::from_rgb8(0xB9, 0xBC, 0xD1);
// --text-3: oklch(0.720 0.030 280)
const TEXT_3: Color = Color::from_rgb8(0xA0, 0xA3, 0xB8);
// --text-4: oklch(0.680 0.030 280)
const TEXT_4: Color = Color::from_rgb8(0x94, 0x97, 0xAB);
// --accent: oklch(0.76 0.110 238)
const ACCENT: Color = Color::from_rgb8(0x68, 0xBB, 0xEF);
// --accent-hover: oklch(0.82 0.115 238)
const ACCENT_HOVER: Color = Color::from_rgb8(0x77, 0xCF, 0xFF);
// --accent-press: oklch(0.70 0.105 238)
const ACCENT_PRESS: Color = Color::from_rgb8(0x59, 0xA8, 0xD8);
// --accent-text: oklch(0.82 0.115 238)
const ACCENT_TEXT: Color = Color::from_rgb8(0x77, 0xCF, 0xFF);
// --accent-quiet: oklch(0.76 0.110 238 / 0.16)
const ACCENT_QUIET: Color = Color::from_rgba8(0x68, 0xBB, 0xEF, 0.16);
// --accent-line: oklch(0.76 0.110 238 / 0.38)
const ACCENT_LINE: Color = Color::from_rgba8(0x68, 0xBB, 0xEF, 0.38);
// --on-accent: oklch(0.22 0.020 280)
const ON_ACCENT: Color = Color::from_rgb8(0x19, 0x1A, 0x24);
// --success: oklch(0.80 0.130 150)
const SUCCESS: Color = Color::from_rgb8(0x7C, 0xD5, 0x91);
// --warning: oklch(0.84 0.110 90)
const WARNING: Color = Color::from_rgb8(0xE6, 0xC8, 0x73);
// --danger: oklch(0.72 0.140 15)
const DANGER: Color = Color::from_rgb8(0xEF, 0x7D, 0x88);
// --danger-text: oklch(0.81 0.140 15)
const DANGER_TEXT: Color = Color::from_rgb8(0xFF, 0x99, 0xA4);
// --on-danger: oklch(0.22 0.02 15)
const ON_DANGER: Color = Color::from_rgb8(0x23, 0x17, 0x18);
// --line: oklch(0.345 0.026 280)
const LINE: Color = Color::from_rgb8(0x36, 0x38, 0x47);

fn palette() -> Palette {
    Palette {
        background: BG,
        text: TEXT,
        primary: ACCENT,
        success: SUCCESS,
        warning: WARNING,
        danger: DANGER,
    }
}

fn theme() -> iced::Theme {
    iced::Theme::custom("gctl", palette())
}

/// The tick interval for the current connection.
fn tick_interval(connection: Connection) -> Duration {
    match connection {
        Connection::Ready => Duration::from_secs(60),
        Connection::Asleep => Duration::from_secs(5),
        Connection::Loading | Connection::NoMouse | Connection::Failed => Duration::from_secs(3),
    }
}

/// The window's own message type, wrapping the state machine's `Msg`.
#[derive(Debug, Clone)]
enum Message {
    Machine(Msg),
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
        /// `None` when the import never reached the mouse.
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

/// Everything the window carries besides the state machine: the opened device
/// and the export/import path text.
struct Shell {
    machine: State,
    device: Option<DeviceHandle>,
    state_dir: Option<PathBuf>,
    lock_path: Option<PathBuf>,
    export_path: String,
    import_path: String,
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
        };
        let task = shell.run_requests(requests);
        (shell, task)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Machine(message) => {
                let requests = self.machine.update(message);
                self.run_requests(requests)
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
                snapshot,
                remembered,
            } => {
                self.device = device;
                let requests = self.machine.update(Msg::Imported {
                    error,
                    written,
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
        iced::time::every(tick_interval(self.machine.connection()))
            .map(|_| Message::Machine(Msg::Tick))
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
    /// Every transaction shares this one helper, so the re-open policy and the
    /// wait bridge cannot drift between them.
    fn device_task<T>(
        &mut self,
        no_mouse: impl Fn() -> T + Send + 'static,
        failed: impl Fn(String) -> T + Send + 'static,
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
                Ok((device, Err(message))) => map(device, failed(message)),
                Err(error) => map(None, failed(error)),
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
                    }),
                    Err(LoadError::NoMouse),
                    None,
                )
            },
            |message| {
                (
                    Some(WriteFailure {
                        message: message.clone(),
                        landed: Vec::new(),
                    }),
                    Err(LoadError::Failed(message)),
                    None,
                )
            },
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
                    }),
                    Err(LoadError::NoMouse),
                    None,
                )
            },
            |message| {
                (
                    Some(WriteFailure {
                        message: message.clone(),
                        landed: Vec::new(),
                    }),
                    Err(LoadError::Failed(message)),
                    None,
                )
            },
            move |device, sender| {
                let error = {
                    let guard = lock_device(device);
                    match io::with_lock(lock_path.as_deref(), wait(sender), || {
                        io::make_active(&guard, profile)
                    }) {
                        Ok(Ok(())) => None,
                        Ok(Err(message)) | Err(message) => Some(WriteFailure {
                            message,
                            landed: Vec::new(),
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
            || (Some("no mouse found".to_owned()), Vec::new(), None, None),
            |message| (Some(message.clone()), Vec::new(), None, None),
            move |device, sender| {
                let (error, written) = {
                    let guard = lock_device(device);
                    match io::with_lock(lock_path.as_deref(), wait(sender), || {
                        io::run_import(&guard, state_dir.as_deref(), profile, &path, &check)
                    }) {
                        Ok(Ok(written)) => (None, written),
                        Ok(Err(failure)) => (Some(failure.message), failure.landed),
                        Err(message) => (Some(message), Vec::new()),
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
                (error, written, Some(snapshot), remembered)
            },
            |device, (error, written, snapshot, remembered)| Message::Imported {
                device,
                error,
                written,
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

/// Starts the window. `state_dir` and `lock_path` come from `main`.
pub fn run(state_dir: Option<&Path>, lock_path: Option<PathBuf>) -> iced::Result {
    let flags = Flags {
        state_dir: state_dir.map(Path::to_path_buf),
        lock_path,
    };
    let boot = move || Shell::boot(flags.clone());
    iced::application(boot, Shell::update, Shell::view)
        .theme(|_shell: &Shell| theme())
        .subscription(Shell::subscription)
        .title("gctl")
        .window_size(Size::new(1024.0, 680.0))
        .run()
}

// --- the view -----------------------------------------------------------------

impl Shell {
    fn view(&self) -> Element<'_, Message> {
        let base: Element<'_, Message> = match self.machine.connection() {
            Connection::Loading => center(text("Loading")).into(),
            Connection::NoMouse => empty_state(
                "No mouse found",
                "Plug in a Glorious mouse, or install gctl's udev rule (70-gctl.rules) so your user can open its hidraw node."
                    .to_owned(),
            ),
            Connection::Asleep | Connection::Ready | Connection::Failed
                if self.machine.snapshot().is_none() =>
            {
                match self.machine.connection() {
                    Connection::Asleep => empty_state(
                        "The mouse is asleep. Move it to wake it.",
                        String::new(),
                    ),
                    Connection::Failed => empty_state(
                        "Could not read the mouse",
                        self.machine.error().unwrap_or_default().to_owned(),
                    ),
                    Connection::Loading | Connection::NoMouse | Connection::Ready => {
                        center(text("Loading")).into()
                    }
                }
            }
            Connection::Asleep | Connection::Ready | Connection::Failed => self.main_view(),
        };
        let overlay = self.modal();
        match overlay {
            Some(modal) => stack![base, modal].into(),
            None => base,
        }
    }

    fn modal(&self) -> Option<Element<'_, Message>> {
        let prompt = self.machine.prompt()?;
        let content = match &prompt {
            Prompt::Discard(_) => discard_prompt(prompt, self.machine.section()),
            Prompt::Import(_) => import_prompt(prompt),
        };
        Some(opaque_modal(content))
    }

    fn main_view(&self) -> Element<'_, Message> {
        let machine = &self.machine;
        let error_banner = machine.error().map(|error| {
            container(text(error).color(DANGER_TEXT))
                .style(surface(SURFACE_1))
                .width(Length::Fill)
                .padding(8)
        });
        let content = self.content_pane();
        let mut page = column![];
        if let Some(banner) = error_banner {
            page = page.push(banner);
        }
        page = page.push(row![self.sidebar(), content].height(Length::Fill));
        page.into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let machine = &self.machine;
        let Some(snap) = machine.snapshot() else {
            return container(space()).width(220).into();
        };

        let badge = if snap.model.tested {
            Element::from(space())
        } else {
            container(text("Untested").size(11).color(ON_ACCENT))
                .style(accent_fill())
                .padding([2, 6])
                .into()
        };
        // The badge wraps under the model name, so the longest model name never
        // clips it against the fixed sidebar width.
        let model_row = column![text(snap.model.name).size(18), badge]
            .spacing(4)
            .align_x(iced::Alignment::Start);

        let battery = match snap.battery.percent {
            Some(percent) => format!("Battery {percent}%"),
            None => "Battery unknown".to_owned(),
        };
        let link = match snap.model.link {
            gctl_proto::Link::Wired => "Wired",
            gctl_proto::Link::Wireless => "Wireless",
        };

        let edit_profile = machine.edit_profile();
        let pick_blocked = machine.pick_blocked();
        let profiles = row(Profile::all().into_iter().map(|profile| {
            let is_edited = edit_profile == Some(profile);
            let style = if is_edited {
                accent_quiet_button()
            } else {
                transparent_button()
            };
            let id = if is_edited {
                "edited-profile".to_owned()
            } else {
                format!("profile-pick-{}", profile.get())
            };
            let mut pick = button(text(profile.get().to_string())).style(style);
            if pick_blocked.is_none() {
                pick = pick.on_press(Message::Machine(Msg::PickProfile(profile)));
            }
            container(pick).id(id).into()
        }))
        .spacing(4);
        let edit_row = row![text("Edit").color(TEXT_3), profiles]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        let make_active: Element<'_, Message> =
            if edit_profile.is_some() && edit_profile != Some(snap.active) {
                match machine.make_active_blocked() {
                    None => button("Make active")
                        .on_press(Message::Machine(Msg::MakeActive))
                        .style(accent_button())
                        .into(),
                    Some(reason) => column![
                        button("Make active").style(disabled_button()),
                        text(reason).color(DANGER_TEXT).size(12),
                    ]
                    .spacing(4)
                    .into(),
                }
            } else {
                Element::from(space())
            };
        let active_row = row![
            text(format!("Active profile: {}", snap.active.get())).color(TEXT_2),
            make_active,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        let sections = column(
            [
                Section::Performance,
                Section::Lighting,
                Section::Buttons,
                Section::Device,
            ]
            .into_iter()
            .map(|section| self.section_row(section)),
        )
        .spacing(2);

        let sidebar = column![
            model_row,
            text(battery).color(TEXT_2),
            text(link).color(TEXT_3),
            edit_row,
            active_row,
            rule::horizontal(1),
            sections,
        ]
        .spacing(8)
        .padding(12);
        container(sidebar)
            .width(220)
            .style(surface(SURFACE_1))
            .height(Length::Fill)
            .into()
    }

    fn section_row(&self, section: Section) -> Element<'_, Message> {
        let machine = &self.machine;
        let current = section == machine.section();
        let label = format!(
            "{}{}",
            section_label(section),
            if machine.pending(section) { " •" } else { "" }
        );
        let row = row![
            text(label).color(if current { ACCENT_TEXT } else { TEXT }),
            space().width(Length::Fill),
        ]
        .padding([6, 10]);
        let style = if current {
            accent_quiet_button()
        } else {
            transparent_button()
        };
        button(row)
            .style(style)
            .on_press(Message::Machine(Msg::Select(section)))
            .width(Length::Fill)
            .into()
    }

    fn content_pane(&self) -> Element<'_, Message> {
        // A pick empties the panes until a snapshot of the picked profile lands:
        // loading while its read runs, asleep when the read answered asleep.
        if self.machine.live_snapshot().is_none() {
            return match self.machine.connection() {
                Connection::Asleep => {
                    empty_state("The mouse is asleep. Move it to wake it.", String::new())
                }
                Connection::Loading
                | Connection::NoMouse
                | Connection::Ready
                | Connection::Failed => center(text("Loading")).into(),
            };
        }
        let machine = &self.machine;
        let section = machine.section();
        let body = match section {
            Section::Performance => self.performance_pane(),
            Section::Lighting => self.lighting_pane(),
            Section::Buttons => self.buttons_pane(),
            Section::Device => self.device_pane(),
        };
        if section == Section::Device {
            return column![scrollable(container(body).padding([12, 16]))].into();
        }
        let blocked = machine.apply_blocked();
        let nothing_pending = !machine.pending(section);
        let apply: Element<'_, Message> = match (&blocked, nothing_pending) {
            (None, false) => button("Apply")
                .style(accent_button())
                .on_press(Message::Machine(Msg::Apply))
                .into(),
            _ => button("Apply").style(disabled_button()).into(),
        };
        let revert: Element<'_, Message> = if nothing_pending {
            button("Revert").style(disabled_button()).into()
        } else {
            button("Revert")
                .style(secondary_button())
                .on_press(Message::Machine(Msg::Revert))
                .into()
        };
        let notice = machine.notice().map(|notice| match notice {
            Notice::Done(message) => text(message).color(SUCCESS),
            Notice::Failed(message) => text(message).color(DANGER_TEXT),
        });
        let reason = blocked.map(|reason| text(reason).color(DANGER_TEXT).size(13));
        let footer = column![
            rule::horizontal(1),
            row![
                revert,
                space().width(Length::Fill),
                match reason {
                    Some(reason) => reason.into(),
                    None => Element::from(space()),
                },
                apply,
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center),
            match notice {
                Some(notice) => notice.into(),
                None => Element::from(space()),
            },
        ]
        .spacing(8)
        .padding([12, 16]);
        column![scrollable(container(body).padding([12, 16])), footer].into()
    }

    // --- Performance ----------------------------------------------------------

    fn performance_pane(&self) -> Element<'_, Message> {
        let machine = &self.machine;
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
                let remove: Element<'_, Message> = if stage_texts.len() > 1 {
                    button("Remove")
                        .on_press(Message::Machine(Msg::Edit(Edit::RemoveStage(index))))
                        .style(secondary_button())
                        .into()
                } else {
                    Element::from(space())
                };
                row![
                    text(format!("Stage {}", index + 1)).width(64),
                    text_input("100-19000", value)
                        .style(input_style())
                        .on_input(move |text| Message::Machine(Msg::Edit(Edit::DpiText(
                            index, text,
                        ))))
                        .width(110),
                    remembered.map(color_swatch).unwrap_or_else(|| {
                        container(text("Unknown").color(TEXT_4).size(11))
                            .padding(4)
                            .into()
                    }),
                    text_input("hex", &hex_value)
                        .style(input_style())
                        .on_input(move |text| {
                            Message::Machine(Msg::Edit(Edit::StageColorHex(index, text)))
                        })
                        .width(80),
                    remove,
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .into()
            })
            .collect();

        let add_stage = if stage_texts.len() < gctl_proto::DpiStages::MAX {
            Some(button("Add stage").on_press(Message::Machine(Msg::Edit(Edit::AddStage))))
        } else {
            None
        };

        let active_stage: Element<'_, Message> = pick_list(
            (1..=stage_texts.len() as u8).collect::<Vec<_>>(),
            machine.active_stage(),
            |stage| Message::Machine(Msg::Edit(Edit::ActiveStage(stage))),
        )
        .placeholder("active stage")
        .into();

        let polling: Element<'_, Message> = container(pick_list(
            POLLING_OPTIONS.as_slice(),
            machine.polling_rate().map(polling_label),
            |label| Message::Machine(Msg::Edit(polling_pick(&label))),
        ))
        .id("polling-picker")
        .into();

        let lift_off: Element<'_, Message> = container(pick_list(
            LIFT_OFF_OPTIONS.as_slice(),
            machine.lift_off().map(lift_off_label),
            |label| Message::Machine(Msg::Edit(lift_off_pick(&label))),
        ))
        .id("lift-off-picker")
        .into();

        let debounce_row: Element<'_, Message> = row![
            slider(0..=16, machine.debounce().unwrap_or(0), |ms| {
                Message::Machine(Msg::Edit(Edit::Debounce(ms)))
            }),
            text(format!("{} ms", machine.debounce().unwrap_or(0))).width(48),
        ]
        .spacing(8)
        .into();

        let never = machine.sleep_never().unwrap_or(false);
        let minutes = text_input("minutes", &machine.sleep_minutes().unwrap_or_default())
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
            checkbox(never).on_toggle(|never| Message::Machine(Msg::Edit(Edit::SleepNever(never)))),
            text("Never"),
            minutes,
            text("m"),
            seconds,
            text("s"),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into();

        let dpi_mark: Element<'_, Message> =
            match machine.snapshot().and_then(|snap| snap.dpi_colors.as_ref()) {
                Some(_) => mark_text("set by gctl"),
                None => Element::from(space()),
            };
        let dpi_label_row: Element<'_, Message> = row![
            text("DPI stages").color(TEXT_2),
            text("per profile").color(TEXT_4).size(12),
            dpi_mark,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into();

        column![
            dpi_label_row,
            column(stage_rows).spacing(6),
            match add_stage {
                Some(add) => add.into(),
                None => Element::from(space()),
            },
            labeled_row("Active stage", "per profile"),
            active_stage,
            labeled_row("Polling rate", "all profiles"),
            polling,
            labeled_row("Lift-off", "all profiles"),
            lift_off,
            labeled_row("Debounce", "per profile"),
            debounce_row,
            labeled_row("Sleep", "all profiles"),
            sleep_row,
        ]
        .spacing(10)
        .into()
    }

    // --- Lighting -------------------------------------------------------------

    fn lighting_pane(&self) -> Element<'_, Message> {
        let machine = &self.machine;
        let kind = machine.effect_kind();

        let effect_picker = pick_list(EffectKind::ALL, kind, |kind| {
            Message::Machine(Msg::Edit(Edit::Effect(kind)))
        });
        // An unknown effect reads `Unknown` as a text widget, so a selector can
        // count it, never as an uncountable picker placeholder.
        let effect_row: Element<'_, Message> = if kind.is_none() {
            row![effect_picker, mark_text("Unknown")]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .into()
        } else {
            effect_picker.into()
        };

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

        let rate_row: Element<'_, Message> = if takes_rate {
            let rate = machine.effect_rate().unwrap_or(0);
            row![
                text("Speed").width(56),
                slider(0..=100, rate, |rate| {
                    Message::Machine(Msg::Edit(Edit::Rate(rate)))
                }),
                text(format!("{rate}")).width(40),
            ]
            .spacing(8)
            .into()
        } else {
            Element::from(space())
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
                    let remembered = colors.get(index).copied();
                    let hex = machine.effect_color_hex(index);
                    let hex_value = if hex.is_empty() {
                        remembered.map(parse::format_color).unwrap_or_default()
                    } else {
                        hex
                    };
                    let remove = if colors.len() > min_effect_colors(kind) {
                        Some(
                            button("Remove")
                                .on_press(Message::Machine(Msg::Edit(Edit::RemoveEffectColor(
                                    index,
                                ))))
                                .style(secondary_button()),
                        )
                    } else {
                        None
                    };
                    row![
                        remembered.map(color_swatch).unwrap_or_else(|| {
                            container(text("Unknown").color(TEXT_4).size(11))
                                .padding(4)
                                .into()
                        }),
                        text_input("hex", &hex_value)
                            .style(input_style())
                            .on_input(move |text| {
                                Message::Machine(Msg::Edit(Edit::EffectColorHex(index, text)))
                            })
                            .width(80),
                        match remove {
                            Some(remove) => remove.into(),
                            None => Element::from(space()),
                        },
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center)
                    .into()
                })
                .collect();
            if colors.len() < max {
                rows.push(
                    button("Add colour")
                        .on_press(Message::Machine(Msg::Edit(Edit::AddEffectColor)))
                        .into(),
                );
            }
            rows
        } else {
            Vec::new()
        };

        let wired_row = brightness_row(
            "wired-slider",
            "Wired",
            machine.brightness_wired(),
            |value| Message::Machine(Msg::Edit(Edit::BrightnessWired(value))),
            Edit::BrightnessWiredRelease,
        );
        let wireless_row = brightness_row(
            "wireless-slider",
            "Wireless",
            machine.brightness_wireless(),
            |value| Message::Machine(Msg::Edit(Edit::BrightnessWireless(value))),
            Edit::BrightnessWirelessRelease,
        );

        let remembered_effect = machine.snapshot().and_then(|snap| snap.effect.as_ref());
        let effect_mark: Element<'_, Message> = match remembered_effect {
            Some(_) => mark_text("set by gctl"),
            None => Element::from(space()),
        };
        let remembered_brightness = machine.snapshot().and_then(|snap| snap.brightness);
        let brightness_mark: Element<'_, Message> = match remembered_brightness {
            Some(_) => mark_text("set by gctl"),
            None => Element::from(space()),
        };

        column![
            labeled_row("Effect", "per profile"),
            effect_row,
            rate_row,
            column(color_rows).spacing(6),
            effect_mark,
            labeled_row("Brightness", "all profiles"),
            wired_row,
            wireless_row,
            brightness_mark,
        ]
        .spacing(10)
        .into()
    }

    // --- Buttons --------------------------------------------------------------

    fn buttons_pane(&self) -> Element<'_, Message> {
        let machine = &self.machine;
        let rows: Vec<Element<'_, Message>> = crate::get::BUTTONS
            .iter()
            .map(|button| self.bind_row(*button))
            .collect();

        let scroll = machine.scroll();
        let remembered_scroll = machine.snapshot().and_then(|snap| snap.scroll);
        // The `set by gctl` mark comes from the snapshot; the `Unknown` text and
        // the dimming follow what is shown (the overlay), so a picked direction
        // drops them even before Apply, like the brightness rows.
        let scroll_mark: Element<'_, Message> = match remembered_scroll {
            Some(_) => mark_text("set by gctl"),
            None if scroll.is_none() => mark_text("Unknown"),
            None => Element::from(space()),
        };
        let invert = scroll == Some(gctl_proto::ScrollDirection::Invert);
        let toggler = toggler(invert)
            .on_toggle(|invert| Message::Machine(Msg::Edit(Edit::Scroll(scroll_of(invert)))));
        // Unknown draws dimmed, like the unknown brightness sliders, so an OFF
        // toggler never reads as `default`.
        let toggler: Element<'_, Message> = if scroll.is_none() {
            toggler.style(dim_toggler()).into()
        } else {
            toggler.into()
        };

        let scroll_row: Element<'_, Message> = row![toggler, text("Invert scroll"), scroll_mark]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into();

        column![
            labeled_row("Scroll", "all profiles"),
            scroll_row,
            labeled_row("Buttons", "per profile"),
            column(rows).spacing(8),
        ]
        .spacing(10)
        .into()
    }

    fn bind_row(&self, button: Button) -> Element<'_, Message> {
        let machine = &self.machine;
        let binding = machine.bind(button);
        let label = button_label(button);
        let remembered_bind = machine
            .snapshot()
            .and_then(|snap| snap.binds.iter().find(|(b, _)| *b == button))
            .and_then(|(_, binding)| *binding);
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
        );
        // An unknown bind reads `Unknown` as a text widget, so a selector can
        // count it, never as an uncountable picker placeholder.
        let category_cell: Element<'_, Message> = if category.is_none() {
            row![category_picker, mark_text("Unknown")]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .into()
        } else {
            category_picker.into()
        };

        let mut row = row![text(label).width(88), category_cell]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        match category {
            None | Some(parse::BindCategory::None) => {}
            Some(parse::BindCategory::Mouse) => {
                row = row.push(fn_picker(
                    &parse::MOUSE_FN_NAMES,
                    parse::BindCategory::Mouse,
                    binding,
                    button,
                ));
            }
            Some(parse::BindCategory::Media) => {
                row = row.push(fn_picker(
                    &parse::MEDIA_FN_NAMES,
                    parse::BindCategory::Media,
                    binding,
                    button,
                ));
            }
            Some(parse::BindCategory::Dpi) => {
                row = row.push(fn_picker(
                    &parse::DPI_FN_NAMES,
                    parse::BindCategory::Dpi,
                    binding,
                    button,
                ));
            }
            Some(parse::BindCategory::Keyboard) => {
                row = row.push(fn_picker(
                    &parse::KEYBOARD_FN_NAMES,
                    parse::BindCategory::Keyboard,
                    binding,
                    button,
                ));
            }
            Some(parse::BindCategory::Key) => {
                // The Key category is only shown for a key bind, so the binding
                // is always a key here; render the category cell alone if not.
                let Some(Binding::Key { key, modifiers }) = binding else {
                    return row.into();
                };
                let key_names = key_names();
                let current_key = Some(key.name());
                let toggles: Vec<Element<'static, Message>> = parse::MODIFIERS
                    .iter()
                    .copied()
                    .map(|(bit, name)| {
                        let on = modifiers.bits() & bit.bits() != 0;
                        row![
                            checkbox(on).on_toggle(move |on| {
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
                            text(name).color(TEXT_4).size(11),
                        ]
                        .spacing(2)
                        .align_y(iced::Alignment::Center)
                        .into()
                    })
                    .collect();
                row = row
                    .push(iced::widget::row(toggles).spacing(6))
                    .push(pick_list(key_names, current_key, move |name| {
                        let edit = match gctl_proto::Key::from_name(name) {
                            Ok(key) => key_edit(button, key, modifiers),
                            Err(_) => Edit::InvalidBind(button),
                        };
                        Message::Machine(Msg::Edit(edit))
                    }));
            }
        }

        if remembered_bind.is_some() {
            row = row.push(mark_text("set by gctl"));
        }
        row.into()
    }

    // --- Device ---------------------------------------------------------------

    fn device_pane(&self) -> Element<'_, Message> {
        let machine = &self.machine;
        let Some(snap) = machine.snapshot() else {
            return text("no device").into();
        };
        let link = match snap.model.link {
            gctl_proto::Link::Wired => "Wired",
            gctl_proto::Link::Wireless => "Wireless",
        };
        let firmware = snap.firmware.to_string();
        let battery = snap
            .battery
            .percent
            .map(|percent| format!("{percent}%"))
            .unwrap_or_else(|| "unknown".to_owned());

        let profile = machine.edit_profile().map(Profile::get).unwrap_or(0);
        let blocked = machine.apply_blocked();
        let export_button: Element<'_, Message> = match &blocked {
            None => button(text(format!("Export profile {profile}")))
                .on_press(Message::ExportClicked)
                .into(),
            Some(_) => button(text(format!("Export profile {profile}")))
                .style(disabled_button())
                .into(),
        };
        let import_button: Element<'_, Message> = match &blocked {
            None => button(text(format!("Import into profile {profile}")))
                .on_press(Message::ImportClicked)
                .into(),
            Some(_) => button(text(format!("Import into profile {profile}")))
                .style(disabled_button())
                .into(),
        };
        let export_row: Element<'_, Message> = row![
            text_input("path.toml", &self.export_path)
                .style(input_style())
                .on_input(Message::ExportPath)
                .width(320),
            export_button,
        ]
        .spacing(8)
        .into();
        let import_row: Element<'_, Message> = row![
            text_input("path.toml", &self.import_path)
                .style(input_style())
                .on_input(Message::ImportPath)
                .width(320),
            import_button,
        ]
        .spacing(8)
        .into();

        let notice = machine.notice().map(|notice| match notice {
            Notice::Done(message) => text(message).color(SUCCESS),
            Notice::Failed(message) => text(message).color(DANGER_TEXT),
        });
        let reason = blocked.map(|reason| text(reason).color(DANGER_TEXT).size(13));

        column![
            field("Model", snap.model.name.to_owned()),
            field("Link", link.to_owned()),
            field("Firmware", firmware),
            field("Battery", battery),
            rule::horizontal(1),
            text("Export profile").color(TEXT_2),
            export_row,
            text("Import profile").color(TEXT_2),
            import_row,
            match reason {
                Some(reason) => reason.into(),
                None => Element::from(space()),
            },
            match notice {
                Some(notice) => notice.into(),
                None => Element::from(space()),
            },
            text("Set by gctl: gctl remembers this value because the mouse cannot report it.")
                .color(TEXT_4)
                .size(13),
        ]
        .spacing(8)
        .into()
    }
}

// --- small view helpers --------------------------------------------------------

fn surface(color: Color) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(color)),
        ..container::Style::default()
    }
}

fn accent_fill() -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(ACCENT)),
        text_color: Some(ON_ACCENT),
        ..container::Style::default()
    }
}

fn empty_state(heading: &'static str, hint: String) -> Element<'static, Message> {
    let mut column = column![text(heading).size(24)]
        .spacing(8)
        .align_x(iced::Alignment::Center);
    if !hint.is_empty() {
        column = column.push(text(hint).color(TEXT_3));
    }
    center(column)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn card_style() -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(SURFACE_1)),
        border: Border {
            radius: 14.0.into(),
            width: 1.0,
            color: LINE,
        },
        ..container::Style::default()
    }
}

fn scrim_style() -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(Color::from_rgba8(0x10, 0x11, 0x17, 0.55))),
        ..container::Style::default()
    }
}

fn opaque_modal(content: Element<'static, Message>) -> Element<'static, Message> {
    let card = container(content)
        .style(card_style())
        .padding(24)
        .max_width(480);
    let centered = center(card).width(Length::Fill).height(Length::Fill);
    let scrim = container(space())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(scrim_style());
    let overlay: Element<'static, Message> = stack![scrim, centered].into();
    iced::widget::opaque(overlay)
}

fn discard_prompt(prompt: Prompt, current: Section) -> Element<'static, Message> {
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
        text(message).size(20),
        row![
            button("Keep editing")
                .on_press(Message::Machine(Msg::KeepEditing))
                .style(secondary_button()),
            button("Discard")
                .on_press(Message::Machine(Msg::Discard))
                .style(danger_button()),
        ]
        .spacing(12),
    ]
    .spacing(12)
    .into()
}

fn import_prompt(prompt: Prompt) -> Element<'static, Message> {
    let Prompt::Import(check) = prompt else {
        return space().into();
    };
    let mut column = column![text("Import profile?").size(20)].spacing(12);
    if let Some(note) = check.note {
        column = column.push(text(note).color(WARNING));
    }
    if check.device_wide {
        column = column.push(
            text("This overwrites device-wide settings (polling rate, lift-off, sleep, brightness, scroll).")
                .color(TEXT_2),
        );
    }
    column = column.push(
        row![
            button("Cancel")
                .on_press(Message::Machine(Msg::ImportCancel))
                .style(secondary_button()),
            button("Import")
                .on_press(Message::Machine(Msg::ImportConfirm))
                .style(accent_button()),
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

fn field(label: &'static str, value: String) -> iced::widget::Row<'static, Message> {
    row![text(label).color(TEXT_3).width(100), text(value),].spacing(8)
}

fn labeled_row(label: &'static str, mark: &'static str) -> iced::widget::Row<'static, Message> {
    row![text(label).color(TEXT_2), text(mark).color(TEXT_4).size(12),]
        .spacing(8)
        .align_y(iced::Alignment::Center)
}

fn color_swatch(color: gctl_proto::Rgb) -> Element<'static, Message> {
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
                color: LINE,
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

/// The write-only mark: `unknown` or `set by gctl`, as a standalone label.
fn mark_text(mark: &'static str) -> Element<'static, Message> {
    text(mark).color(TEXT_4).size(12).into()
}

/// The `--well` background for input fields.
fn input_style() -> iced::widget::text_input::StyleFn<'static, iced::Theme> {
    Box::new(|theme, status| iced::widget::text_input::Style {
        background: Background::Color(WELL),
        ..iced::widget::text_input::default(theme, status)
    })
}

fn fn_picker(
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

fn accent_quiet_button() -> iced::widget::button::StyleFn<'static, iced::Theme> {
    Box::new(|_theme, status| iced::widget::button::Style {
        background: Some(Background::Color(match status {
            iced::widget::button::Status::Pressed => SURFACE_3,
            iced::widget::button::Status::Hovered => SURFACE_2,
            _ => ACCENT_QUIET,
        })),
        text_color: ACCENT_TEXT,
        border: Border {
            radius: 6.0.into(),
            color: ACCENT_LINE,
            width: 1.0,
        },
        ..iced::widget::button::Style::default()
    })
}

fn accent_button() -> iced::widget::button::StyleFn<'static, iced::Theme> {
    Box::new(|_theme, status| iced::widget::button::Style {
        background: Some(Background::Color(match status {
            iced::widget::button::Status::Hovered => ACCENT_HOVER,
            iced::widget::button::Status::Pressed => ACCENT_PRESS,
            _ => ACCENT,
        })),
        text_color: ON_ACCENT,
        border: Border {
            radius: 6.0.into(),
            ..Border::default()
        },
        ..iced::widget::button::Style::default()
    })
}

fn danger_button() -> iced::widget::button::StyleFn<'static, iced::Theme> {
    Box::new(|_theme, _status| iced::widget::button::Style {
        background: Some(Background::Color(DANGER)),
        text_color: ON_DANGER,
        border: Border {
            radius: 6.0.into(),
            ..Border::default()
        },
        ..iced::widget::button::Style::default()
    })
}

/// A quiet secondary button: a surface fill with a hairline, never the primary
/// accent and never a danger fill.
fn secondary_button() -> iced::widget::button::StyleFn<'static, iced::Theme> {
    Box::new(|_theme, status| iced::widget::button::Style {
        background: Some(Background::Color(match status {
            iced::widget::button::Status::Pressed => SURFACE_3,
            iced::widget::button::Status::Hovered => SURFACE_2,
            _ => SURFACE_1,
        })),
        text_color: TEXT,
        border: Border {
            radius: 6.0.into(),
            width: 1.0,
            color: LINE,
        },
        ..iced::widget::button::Style::default()
    })
}

/// A dimmed slider, for a write-only value with nothing remembered yet.
fn dim_slider() -> iced::widget::slider::StyleFn<'static, iced::Theme> {
    Box::new(|theme, status| {
        let mut style = iced::widget::slider::default(theme, status);
        style.handle.background = Background::Color(SURFACE_2);
        style.rail.backgrounds = (Background::Color(SURFACE_2), Background::Color(SURFACE_2));
        style
    })
}

/// A dimmed toggler, for the scroll direction while it is unknown.
fn dim_toggler() -> iced::widget::toggler::StyleFn<'static, iced::Theme> {
    Box::new(|theme, status| {
        let mut style = iced::widget::toggler::default(theme, status);
        style.background = Background::Color(SURFACE_2);
        style.foreground = Background::Color(SURFACE_3);
        style
    })
}

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

/// One brightness row: a dimmed slider with its handle at 0 and `Unknown` until
/// the link is known, then a live slider and the number. The unknown slider
/// sends `release` on release, so a click at 0 can set the link to 0 while a
/// click that picked a value keeps it. The slider carries `id`, so a test can
/// find its bounds by layout id rather than by the label.
fn brightness_row(
    id: &'static str,
    label: &'static str,
    value: Option<u8>,
    on_change: impl Fn(u8) -> Message + 'static,
    release: Edit,
) -> Element<'static, Message> {
    match value {
        Some(value) => {
            let slider = container(slider(0..=255, value, on_change).width(Length::Fill))
                .width(Length::Fill)
                .id(id);
            row![
                text(label).width(56),
                slider,
                text(format!("{value}")).width(64)
            ]
            .spacing(8)
            .into()
        }
        None => {
            let slider = container(
                slider(0..=255, 0, on_change)
                    .width(Length::Fill)
                    .style(dim_slider())
                    .on_release(Message::Machine(Msg::Edit(release))),
            )
            .width(Length::Fill)
            .id(id);
            row![
                text(label).width(56),
                slider,
                text("Unknown").width(Length::Shrink)
            ]
            .spacing(8)
            .into()
        }
    }
}

fn disabled_button() -> iced::widget::button::StyleFn<'static, iced::Theme> {
    Box::new(|_theme, _status| iced::widget::button::Style {
        background: Some(Background::Color(SURFACE_2)),
        text_color: TEXT_4,
        border: Border {
            radius: 6.0.into(),
            ..Border::default()
        },
        ..iced::widget::button::Style::default()
    })
}

fn transparent_button() -> iced::widget::button::StyleFn<'static, iced::Theme> {
    Box::new(|_theme, _status| iced::widget::button::Style {
        background: None,
        text_color: TEXT,
        ..iced::widget::button::Style::default()
    })
}

#[cfg(test)]
#[path = "../../tests/unit/gui_view.rs"]
mod tests;
