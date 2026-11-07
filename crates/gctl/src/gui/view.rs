//! The iced configurator window over [`state`] and [`io`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use iced::futures::StreamExt;
use iced::futures::channel::mpsc;
use iced::theme::Mode;
use iced::time::Instant;
use iced::widget::{
    Float, center, checkbox, column, container, row, rule, scrollable, slider, space, stack, text,
    text_input, toggler,
};
use iced::{Background, Border, Color, Element, Length, Padding, Size, Subscription, Task, Vector};

use gctl_proto::{Binding, Button, Device, Profile};

use super::control::Control;
use super::dropdown::{self, Dropdown};
use super::io;
use super::motion::{self, Curve, Ease, Key, Prefs};
use super::state::{
    Connection, Edit, EffectKind, ImportCheck, Leave, LoadError, Msg, Notice, Prompt, Request,
    Section, Snapshot, State, WriteFailure, button_label,
};
use super::theme::{self, CONTROL_HEIGHT, MONO_FONT, Scheme, Tokens, UI_FONT};
use crate::memory::Memory;
use crate::parse;
use crate::writes::WriteOp;

/// The window's own message type, wrapping the state machine's `Msg`.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    Machine(Msg),
    /// One redraw's instant: the shell's clock moves forward to here.
    Frame(Instant),
    /// The desktop's colour scheme mode answered the boot query or changed.
    Theme(Mode),
    /// The portal's (or GNOME's) reduced-motion flag.
    Motion(bool),
    /// kdeglobals' animation factor, read off the UI thread. `sequence` orders
    /// the reads: a slow older answer never overwrites a newer one.
    Kde {
        sequence: u64,
        factor: Option<f32>,
    },
    /// A window event; only a regained focus re-reads the desktop's motion
    /// preference.
    Window(iced::window::Event),
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
/// the export/import path text, the current colour scheme and the motion.
pub(crate) struct Shell {
    pub(crate) machine: State,
    device: Option<DeviceHandle>,
    state_dir: Option<PathBuf>,
    lock_path: Option<PathBuf>,
    export_path: String,
    import_path: String,
    pub(crate) scheme: Scheme,
    pub(crate) motion: Motion,
}

#[derive(Clone)]
struct Flags {
    state_dir: Option<PathBuf>,
    lock_path: Option<PathBuf>,
    /// kdeglobals, resolved by `main`; `None` when the desktop has no KDE
    /// configuration to read.
    kdeglobals: Option<PathBuf>,
}

/// The shell's clock and the values it moves. The clock only ever advances,
/// every animation starts at it, and the view reads it, never `Instant::now()`.
pub(crate) struct Motion {
    clock: Instant,
    pub(crate) prefs: Prefs,
    /// kdeglobals, resolved by `main`; its factor is read on the blocking pool.
    pub(crate) kdeglobals: Option<PathBuf>,
    /// The factor the last kdeglobals read answered, kept so a portal message
    /// can re-derive the preference without reading the file again.
    kde: Option<f32>,
    /// The sequence number of the next kdeglobals read, and of the newest one
    /// whose answer was applied: a slow older answer is dropped.
    kde_reads: u64,
    kde_applied: u64,
    section_alpha: Ease,
    section_rise: Ease,
    segment: Ease,
    dot: Ease,
    active_dot: Ease,
    /// The tint a control's state change eases: a section row's selection, a
    /// segment's edited mark. One ease per key, so a fill lands with the mark
    /// it belongs to instead of snapping ahead of it.
    selection: std::collections::HashMap<Key, Ease>,
    notice: Ghost,
    changed: Ghost,
    error: Ghost,
    outside: Ghost,
}

/// The state a message started from, for the values its change animates.
#[derive(Clone, Copy)]
struct Before {
    section: Section,
    profile: Option<Profile>,
    active: Option<Profile>,
}

impl Before {
    fn of(machine: &State) -> Before {
        Before {
            section: machine.section(),
            profile: machine.edit_profile(),
            active: machine.snapshot().map(|snapshot| snapshot.active),
        }
    }
}

/// A line that leaves the state keeps being drawn while it fades out: the
/// shell holds the last text and its fade; `gui::state` stays untouched.
struct Ghost {
    text: Option<String>,
    /// A notice's tone: true is the success colour.
    success: bool,
    fade: Ease,
}

impl Ghost {
    fn new() -> Ghost {
        Ghost {
            text: None,
            success: false,
            fade: Ease::settled(0.0),
        }
    }

    /// Follows what the state draws now: a new text fades in, a gone one fades
    /// out from the text it kept.
    fn observe(&mut self, current: Option<(&str, bool)>, now: Instant, prefs: Prefs) {
        match current {
            Some((text, success)) => {
                if self.text.as_deref() != Some(text)
                    || self.success != success
                    || self.fade.target() <= 0.0
                {
                    self.text = Some(text.to_owned());
                    self.success = success;
                    self.fade
                        .start_from(0.0, 1.0, now, prefs.fade(motion::NEAR), Curve::EaseOut);
                }
            }
            None => {
                if self.text.is_some() && self.fade.target() > 0.0 {
                    self.fade.start(
                        0.0,
                        now,
                        prefs.fade(motion::exit(motion::NEAR)),
                        Curve::Ease,
                    );
                }
            }
        }
    }

    fn visible(&self, now: Instant) -> Option<(&str, f32)> {
        let alpha = self.fade.at(now);
        let text = self.text.as_deref()?;
        (alpha > 0.0).then_some((text, alpha))
    }

    fn animating(&self, now: Instant) -> bool {
        self.fade.animating(now)
    }
}

/// What one line draws now: the state's text, or the ghost's while it fades
/// out. A text the shell never saw (a test mutating the machine directly)
/// draws at full strength.
fn ghost_line<'a>(
    ghost: &'a Ghost,
    current: Option<&'a str>,
    now: Instant,
) -> Option<(&'a str, f32)> {
    match (current, ghost.visible(now)) {
        (Some(text), Some((shown, alpha))) if shown == text => Some((text, alpha)),
        (Some(text), _) => Some((text, 1.0)),
        (None, Some((shown, alpha))) => Some((shown, alpha)),
        (None, None) => None,
    }
}

/// The notice's shape of `ghost_line`, which also carries its tone.
fn ghost_notice<'a>(
    ghost: &'a Ghost,
    current: Option<(&'a str, bool)>,
    now: Instant,
) -> Option<(&'a str, bool, f32)> {
    match (current, ghost.visible(now)) {
        (Some((text, success)), Some((shown, alpha))) if shown == text => {
            Some((text, success, alpha))
        }
        (Some((text, success)), _) => Some((text, success, 1.0)),
        (None, Some((shown, alpha))) => Some((shown, ghost.success, alpha)),
        (None, None) => None,
    }
}

impl Motion {
    fn new(machine: &State, kdeglobals: Option<PathBuf>) -> Motion {
        Motion {
            clock: Instant::now(),
            prefs: Prefs::default(),
            kdeglobals,
            kde: None,
            kde_reads: 0,
            kde_applied: 0,
            section_alpha: Ease::settled(1.0),
            section_rise: Ease::settled(0.0),
            segment: Ease::settled(segment_offset(profile_index(machine))),
            dot: Ease::settled(dot_offset(section_index(machine.section()))),
            active_dot: Ease::settled(active_dot_offset(active_index(machine))),
            selection: std::collections::HashMap::new(),
            notice: Ghost::new(),
            changed: Ghost::new(),
            error: Ghost::new(),
            outside: Ghost::new(),
        }
    }

    /// Moves the clock forward; it never winds back.
    fn advance(&mut self, at: Instant) {
        self.clock = self.clock.max(at);
    }

    pub(crate) fn now(&self) -> Instant {
        self.clock
    }

    /// A control's state tint at the clock, `current` when it never moved.
    pub(crate) fn selection_at(&self, key: Key, current: bool, now: Instant) -> f32 {
        self.selection
            .get(&key)
            .map(|ease| ease.at(now))
            .unwrap_or(if current { 1.0 } else { 0.0 })
    }

    /// Under an open prompt the sliding marks and the cards' rise go straight
    /// to their targets, so a mark that moved under the prompt is at its new
    /// place the moment it closes instead of jumping back into a slide.
    fn settle_marks(&mut self, machine: &State) {
        self.segment = Ease::settled(segment_offset(profile_index(machine)));
        self.dot = Ease::settled(dot_offset(section_index(machine.section())));
        self.active_dot = Ease::settled(active_dot_offset(active_index(machine)));
        self.section_rise = Ease::settled(0.0);
    }

    /// Takes the factor kdeglobals answered, keeping the portal's flag. An
    /// answer older than the newest one applied is dropped.
    fn set_kde(&mut self, sequence: u64, kde: Option<f32>) {
        if sequence <= self.kde_applied {
            return;
        }
        self.kde_applied = sequence;
        self.kde = kde;
        self.prefs = Prefs::from_desktop(self.prefs.reduce, kde);
    }

    fn set_reduce(&mut self, reduce: bool) {
        self.prefs = Prefs::from_desktop(reduce, self.kde);
    }

    /// Starts the motion a message's state change asks for.
    fn after(&mut self, machine: &State, before: Before) {
        let now = self.clock;
        if machine.section() != before.section {
            self.section_alpha.start_from(
                0.0,
                1.0,
                now,
                self.prefs.fade(motion::FAR),
                Curve::EaseOut,
            );
            if let Some(duration) = self.prefs.travel(motion::NEAR) {
                self.section_rise
                    .start_from(RISE, 0.0, now, duration, Curve::EaseOut);
            } else {
                self.section_rise = Ease::settled(0.0);
            }
            if let Some(duration) = self.prefs.travel(motion::SLIDE) {
                self.dot.start(
                    dot_offset(section_index(machine.section())),
                    now,
                    duration,
                    Curve::Slide,
                );
            } else {
                self.dot = Ease::settled(dot_offset(section_index(machine.section())));
            }
            // The row's fill and label land on the tint's ladder, with the
            // mark that moved to them.
            for index in 0..4 {
                self.selection
                    .entry(Key::SectionRow(index))
                    .or_insert_with(|| Ease::settled(0.0))
                    .start(
                        if index == section_index(machine.section()) {
                            1.0
                        } else {
                            0.0
                        },
                        now,
                        self.prefs.fade(motion::TINT),
                        Curve::Ease,
                    );
            }
        }
        // The first profile the mouse reports settles the marks where the
        // window draws them; only a change from one known profile to another
        // slides.
        match (before.profile, machine.edit_profile()) {
            (None, Some(_)) => {
                self.segment = Ease::settled(segment_offset(profile_index(machine)));
            }
            (Some(before), Some(after)) if before != after => {
                if let Some(duration) = self.prefs.travel(motion::SLIDE) {
                    self.segment.start(
                        segment_offset(profile_index(machine)),
                        now,
                        duration,
                        Curve::Slide,
                    );
                } else {
                    self.segment = Ease::settled(segment_offset(profile_index(machine)));
                }
                // The digit's colour eases onto the capsule that slides under
                // it, so the mark and its digit change together.
                for profile in Profile::all() {
                    self.selection
                        .entry(Key::Segment(profile.get()))
                        .or_insert_with(|| Ease::settled(0.0))
                        .start(
                            if machine.edit_profile() == Some(profile) {
                                1.0
                            } else {
                                0.0
                            },
                            now,
                            self.prefs.fade(motion::TINT),
                            Curve::Ease,
                        );
                }
            }
            _ => {}
        }
        let active = machine.snapshot().map(|snapshot| snapshot.active);
        if active != before.active {
            let offset = active_dot_offset(profile_slot(active));
            if before.active.is_none() {
                self.active_dot = Ease::settled(offset);
            } else if let Some(duration) = self.prefs.travel(motion::SLIDE) {
                self.active_dot.start(offset, now, duration, Curve::Slide);
            } else {
                self.active_dot = Ease::settled(offset);
            }
        }
    }

    pub(crate) fn animating(&self) -> bool {
        let now = self.clock;
        self.section_alpha.animating(now)
            || self.section_rise.animating(now)
            || self.segment.animating(now)
            || self.dot.animating(now)
            || self.active_dot.animating(now)
            || self.selection.values().any(|ease| ease.animating(now))
            || self.notice.animating(now)
            || self.changed.animating(now)
            || self.error.animating(now)
            || self.outside.animating(now)
    }
}

/// How far the cards start below their resting place when a section opens.
const RISE: f32 = 6.0;

fn section_index(section: Section) -> usize {
    match section {
        Section::Performance => 0,
        Section::Lighting => 1,
        Section::Buttons => 2,
        Section::Device => 3,
    }
}

fn profile_slot(profile: Option<Profile>) -> usize {
    profile
        .map(|profile| usize::from(profile.get().saturating_sub(1)))
        .unwrap_or(0)
}

fn profile_index(machine: &State) -> usize {
    profile_slot(machine.edit_profile())
}

fn active_index(machine: &State) -> usize {
    profile_slot(machine.snapshot().map(|snapshot| snapshot.active))
}

/// The left offset of the active-profile dot under segment `index`.
fn active_dot_offset(index: usize) -> f32 {
    TRACK_PADDING + index as f32 * (SEGMENT_WIDTH + SEGMENT_GAP) + (SEGMENT_WIDTH - DOT_SIZE) / 2.0
}

impl Shell {
    /// A shell over `machine` with no device, no paths, the dark scheme, no
    /// motion preference from the desktop and nothing animating.
    pub(crate) fn new(machine: State) -> Shell {
        Shell {
            motion: Motion::new(&machine, None),
            machine,
            device: None,
            state_dir: None,
            lock_path: None,
            export_path: String::new(),
            import_path: String::new(),
            scheme: Scheme::Dark,
        }
    }

    fn boot(flags: Flags) -> (Shell, Task<Message>) {
        let (machine, requests) = State::new();
        let mut shell = Shell::new(machine);
        shell.motion = Motion::new(&shell.machine, flags.kdeglobals);
        shell.state_dir = flags.state_dir;
        shell.lock_path = flags.lock_path;
        let mut task = shell.run_requests(requests);
        // The desktop's mode answers asynchronously; dark holds until it does.
        task = Task::batch([
            task,
            iced::system::theme().map(Message::Theme),
            shell.kde_task(),
        ]);
        (shell, task)
    }

    /// Reads kdeglobals' animation factor off the UI thread; `Task::none` when
    /// the desktop has no file to read. Each read carries the next sequence
    /// number, so an older answer that arrives late is dropped.
    fn kde_task(&mut self) -> Task<Message> {
        let Some(path) = self.motion.kdeglobals.clone() else {
            return Task::none();
        };
        self.motion.kde_reads += 1;
        let sequence = self.motion.kde_reads;
        Task::perform(
            async move {
                spawn(move || motion::kde_factor_file(&path))
                    .await
                    .ok()
                    .flatten()
            },
            move |factor| Message::Kde { sequence, factor },
        )
    }

    /// Advances the machine and starts the motion its change asks for. Every
    /// mutation goes through here, so no animation can miss its message.
    fn machine_update(&mut self, message: Msg) -> Vec<Request> {
        let before = Before::of(&self.machine);
        let requests = self.machine.update(message);
        self.motion.after(&self.machine, before);
        // The base layer draws at rest under a prompt, its marks included.
        if self.machine.prompt().is_some() {
            self.motion.settle_marks(&self.machine);
        }
        self.sync_ghosts();
        requests
    }

    /// Follows the lines the state draws now, so a line that leaves keeps its
    /// last text through its exit fade.
    fn sync_ghosts(&mut self) {
        let now = self.motion.now();
        let prefs = self.motion.prefs;
        let notice = self.machine.notice().map(|notice| match notice {
            Notice::Done(message) => (message.as_str(), true),
            Notice::Failed(message) => (message.as_str(), false),
        });
        let error = self.machine.error().map(|error| (error, false));
        let outside = self
            .machine
            .outside_change()
            .then_some((OUTSIDE_BANNER, false));
        let changed = {
            let lines = self.machine.changed();
            (!lines.is_empty()).then(|| (lines.join("\n"), true))
        };
        self.motion.notice.observe(notice, now, prefs);
        self.motion.error.observe(error, now, prefs);
        self.motion.outside.observe(outside, now, prefs);
        self.motion.changed.observe(
            changed.as_ref().map(|(text, ok)| (text.as_str(), *ok)),
            now,
            prefs,
        );
    }

    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        self.motion.advance(match &message {
            Message::Frame(at) => *at,
            _ => Instant::now(),
        });
        match message {
            Message::Frame(_) => Task::none(),
            Message::Motion(reduce) => {
                self.motion.set_reduce(reduce);
                Task::none()
            }
            Message::Kde { sequence, factor } => {
                self.motion.set_kde(sequence, factor);
                Task::none()
            }
            Message::Window(iced::window::Event::Focused) => self.kde_task(),
            Message::Window(_) => Task::none(),
            Message::Machine(message) => {
                let requests = self.machine_update(message);
                self.run_requests(requests)
            }
            Message::Theme(mode) => {
                self.scheme = Scheme::from_mode(mode);
                Task::none()
            }
            Message::Loaded { device, result } => {
                self.device = device;
                let requests = self.machine_update(Msg::Loaded(result));
                self.run_requests(requests)
            }
            Message::Applied {
                device,
                error,
                snapshot,
                remembered,
            } => {
                self.device = device;
                let requests = self.machine_update(Msg::Applied {
                    error,
                    snapshot,
                    remembered,
                });
                self.run_requests(requests)
            }
            Message::Exported { device, result } => {
                self.device = device;
                let requests = self.machine_update(Msg::Exported(result));
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
                let requests = self.machine_update(Msg::Imported {
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
        let frames = if self.motion.animating() {
            iced::window::frames().map(Message::Frame)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            iced::time::every(tick_interval(self.machine.connection()))
                .map(|_| Message::Machine(Msg::Tick)),
            iced::system::theme_changes().map(Message::Theme),
            iced::window::events().map(|(_id, event)| Message::Window(event)),
            frames,
            Subscription::run(|| {
                mundy::Preferences::stream(mundy::Interest::ReducedMotion).map(|prefs| {
                    Message::Motion(prefs.reduced_motion == mundy::ReducedMotion::Reduce)
                })
            }),
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

/// Starts the window. `state_dir`, `lock_path` and `kdeglobals` come from
/// `main`, which is the only reader of environment variables.
pub fn run(
    state_dir: Option<&Path>,
    lock_path: Option<PathBuf>,
    kdeglobals: Option<PathBuf>,
) -> iced::Result {
    let flags = Flags {
        state_dir: state_dir.map(Path::to_path_buf),
        lock_path,
        kdeglobals,
    };
    let boot = move || Shell::boot(flags.clone());
    iced::application(boot, Shell::update, Shell::view)
        .theme(|shell: &Shell| shell.theme())
        .subscription(Shell::subscription)
        .default_font(UI_FONT)
        .font(theme::FONTS[0])
        .font(theme::FONTS[1])
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

/// The sidebar's fixed width.
const SIDEBAR_WIDTH: f32 = 220.0;
/// The gap above each sidebar eyebrow group, from the rhythm scale's heading
/// step: an eyebrow opens a group, so it takes the 24 px above an h4. The 16 px
/// flow gap is already the row rhythm, and 32/40 px belong to a page column.
const SIDEBAR_GROUP_GAP: f32 = 24.0;
/// The outside-change banner's one text, shared by the live banner and the
/// ghost it leaves while it fades out.
const OUTSIDE_BANNER: &str = "The mouse was changed outside this window. Values marked \"Last set by gctl\" may be out of date.";
/// The sidebar's inner padding; the header, the eyebrows, the profile track and
/// the section dot all share the content edge it leaves.
const SIDEBAR_PADDING: f32 = 12.0;

/// One profile segment, and the gutter between two of them.
const SEGMENT_WIDTH: f32 = 52.0;
const SEGMENT_HEIGHT: f32 = 30.0;
const SEGMENT_GAP: f32 = 1.0;
/// The track's inner gutter, so the selected capsule never touches its frame.
const TRACK_PADDING: f32 = 4.0;
const TRACK_WIDTH: f32 = 3.0 * SEGMENT_WIDTH + 2.0 * SEGMENT_GAP + 2.0 * TRACK_PADDING;
const TRACK_HEIGHT: f32 = SEGMENT_HEIGHT + 2.0 * TRACK_PADDING;

/// The one dot that marks the open section, and the gutter it lives in.
const DOT_SIZE: f32 = 6.0;
const DOT_COLUMN: f32 = 18.0;
const SECTION_ROW_HEIGHT: f32 = 34.0;
const SECTION_ROW_GAP: f32 = 2.0;
const SECTION_ROW_PITCH: f32 = SECTION_ROW_HEIGHT + SECTION_ROW_GAP;

/// The link line's fixed height: the battery tag is taller than the meter, so
/// the line holds one height and the profile group below never shifts.
const LINK_LINE_HEIGHT: f32 = 20.0;

/// The shared width of the Performance pane's single-value controls.
const VALUE_CONTROL_WIDTH: f32 = 150.0;
/// A stage's colour field.
const HEX_WIDTH: f32 = 80.0;
const EYEBROW_SIZE: f32 = 11.0;

/// The left offset of the selected segment's capsule at `index`.
fn segment_offset(index: usize) -> f32 {
    TRACK_PADDING + index as f32 * (SEGMENT_WIDTH + SEGMENT_GAP)
}

/// The top offset of the section dot at `index`.
fn dot_offset(index: usize) -> f32 {
    index as f32 * SECTION_ROW_PITCH + (SECTION_ROW_HEIGHT - DOT_SIZE) / 2.0
}

/// One pane's paint: the scheme, the alpha its cards fade at, and how far they
/// have risen. iced has no subtree opacity, so every style and text colour the
/// pane paints carries this alpha.
#[derive(Clone, Copy)]
struct Look {
    scheme: Scheme,
    alpha: f32,
    rise: f32,
}

impl Look {
    fn tokens(self) -> Tokens {
        self.scheme.tokens().scale_alpha(self.alpha)
    }
}

impl Shell {
    /// The iced theme this shell's scheme carries; `run` hands it to iced.
    pub(crate) fn theme(&self) -> iced::Theme {
        self.scheme.theme()
    }

    pub(crate) fn view(&self) -> Element<'_, Message> {
        self.render(self.scheme)
    }

    /// The whole window under one scheme, shared by the live window and the
    /// two-theme render tests.
    pub(crate) fn render(&self, scheme: Scheme) -> Element<'_, Message> {
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
            Prompt::Discard(_) => discard_prompt(scheme, self, prompt, self.machine.section()),
            Prompt::Import(_) => import_prompt(scheme, self, prompt),
        };
        Some(opaque_modal(scheme, content))
    }

    fn main_view(&self, scheme: Scheme) -> Element<'_, Message> {
        let t = scheme.tokens();
        let now = self.motion.now();
        let error_banner =
            ghost_line(&self.motion.error, self.machine.error(), now).map(|(text, alpha)| {
                let fading = t.scale_alpha(alpha);
                container(ui_text(capitalize(text)).size(13).color(fading.danger_text))
                    .style(theme::banner_at(fading))
                    .width(Length::Fill)
                    .padding([8.0, 12.0])
            });
        let outside = self.machine.outside_change().then_some(OUTSIDE_BANNER);
        let outside_banner = ghost_line(&self.motion.outside, outside, now).map(|(text, alpha)| {
            let fading = t.scale_alpha(alpha);
            let dismiss = self
                .control(control_label("Dismiss"), theme::warning_surface(fading))
                .height(CONTROL_HEIGHT)
                .on_press(Message::Machine(Msg::DismissOutsideChange));
            container(
                row![
                    ui_text(text).size(13).color(fading.warning),
                    space().width(Length::Fill),
                    dismiss,
                ]
                .spacing(12)
                .align_y(iced::Alignment::Center),
            )
            .style(theme::banner_warning_at(fading))
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
            return container(space()).width(SIDEBAR_WIDTH).into();
        };

        // The badge wraps under the model name, so the longest model name never
        // clips it against the fixed sidebar width.
        let mut model_row = column![ui_text(snap.model.name).size(18).color(t.text)]
            .align_x(iced::Alignment::Start);
        if !snap.model.tested {
            model_row = model_row.push(warning_tag(scheme, "Untested")).spacing(4);
        }

        let link = match snap.model.link {
            gctl_proto::Link::Wired => "Wired",
            gctl_proto::Link::Wireless => "Wireless",
        };
        let battery: Element<'_, Message> = match snap.battery.percent {
            Some(percent) => row![
                ui_text(format!("{percent}%"))
                    .size(12)
                    .font(MONO_FONT)
                    .color(t.text_2),
                battery_meter(scheme, percent),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into(),
            None => tag(
                Look {
                    scheme,
                    alpha: 1.0,
                    rise: 0.0,
                },
                "Unknown",
            ),
        };
        // One fixed height whether the line shows the meter or the `Unknown`
        // tag: the taller tag would otherwise push the profile group down.
        let battery_line = container(
            row![
                ui_text(eyebrow(link)).size(EYEBROW_SIZE).color(t.text_4),
                battery,
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .height(LINK_LINE_HEIGHT)
        .align_y(iced::Alignment::Center);

        let edited = machine.edit_profile();
        let segments = self.profile_segments(scheme);

        let show_make_active = edited.is_some() && edited != Some(snap.active);
        let mut active_area = column![].spacing(4);
        if show_make_active {
            let name = edited.unwrap_or(snap.active).get();
            let blocked = machine.make_active_blocked();
            let mut control = self
                .control(
                    control_label(format!("Make {name} active")),
                    theme::primary_surface(*t),
                )
                .height(CONTROL_HEIGHT)
                .disabled(blocked.is_some());
            if blocked.is_none() {
                control = control.on_press(Message::Machine(Msg::MakeActive));
            }
            active_area = active_area.push(control);
            if let Some(reason) = blocked {
                active_area =
                    active_area.push(ui_text(capitalize(&reason)).size(11).color(t.text_3));
            }
        }

        // The header, the profile group and the settings group read as three
        // groups: each eyebrow opens its group with the rhythm's heading gap.
        let header = column![model_row, battery_line].spacing(8);
        let mut profile_group = column![
            ui_text(eyebrow("Profile"))
                .size(EYEBROW_SIZE)
                .color(t.text_4),
            segments,
            self.active_profile_dot(scheme, snap.active),
        ]
        .spacing(8);
        if show_make_active {
            profile_group = profile_group.push(active_area);
        }
        let settings_group = column![
            ui_text(eyebrow("Settings"))
                .size(EYEBROW_SIZE)
                .color(t.text_4),
            self.section_rows(scheme),
        ]
        .spacing(8);
        let body = column![header, profile_group, settings_group].spacing(SIDEBAR_GROUP_GAP);

        container(body.padding(SIDEBAR_PADDING))
            .width(SIDEBAR_WIDTH)
            .style(theme::sidebar(scheme))
            .height(Length::Fill)
            .into()
    }

    /// The dot under the active profile's segment, sliding when the active
    /// profile moves.
    fn active_profile_dot(&self, scheme: Scheme, active: Profile) -> Element<'_, Message> {
        let t = scheme.tokens();
        let now = self.motion.now();
        let base = active_dot_offset(profile_slot(Some(active)));
        let offset = if self.base_moves() && self.motion.active_dot.animating(now) {
            self.motion.active_dot.at(now)
        } else {
            base
        };
        let dot = container(space())
            .width(DOT_SIZE)
            .height(DOT_SIZE)
            .id("active-profile-dot")
            .style(move |_theme| container::Style {
                background: Some(Background::Color(t.accent)),
                border: Border {
                    radius: 999.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            });
        let dot = container(dot)
            .width(TRACK_WIDTH)
            .height(DOT_SIZE)
            .padding(Padding {
                top: 0.0,
                left: base,
                right: 0.0,
                bottom: 0.0,
            });
        let shift = offset - base;
        Float::new(dot)
            .translate(move |_, _| Vector::new(shift, 0.0))
            .into()
    }

    /// The one segmented profile control: a framed track, three equal segments,
    /// and one capsule sliding under the selected digit.
    fn profile_segments(&self, scheme: Scheme) -> Element<'_, Message> {
        let machine = &self.machine;
        let t = scheme.tokens();
        let edited = machine.edit_profile();
        let blocked = machine.pick_blocked();
        let now = self.motion.now();
        // The capsule's slide is a transform, never layout: its resting place
        // is the drawn segment's, and it only leaves it while it moves.
        let base = segment_offset(profile_index(machine));
        let offset = if self.base_moves() && self.motion.segment.animating(now) {
            self.motion.segment.at(now)
        } else {
            base
        };

        let track = container(space())
            .width(TRACK_WIDTH)
            .height(TRACK_HEIGHT)
            .style(theme::segment_track(scheme));
        let capsule = container(
            container(space())
                .width(SEGMENT_WIDTH)
                .height(SEGMENT_HEIGHT)
                .style(theme::segment_selected(scheme)),
        )
        .width(TRACK_WIDTH)
        .height(TRACK_HEIGHT)
        .padding(Padding {
            top: TRACK_PADDING,
            left: base,
            right: 0.0,
            bottom: 0.0,
        });
        let shift = offset - base;
        let capsule: Element<'_, Message> = Float::new(capsule)
            .translate(move |_, _| Vector::new(shift, 0.0))
            .into();
        let cells = row(Profile::all().into_iter().map(|profile| {
            let is_edited = edited == Some(profile);
            let id = if is_edited {
                "edited-profile".to_owned()
            } else {
                format!("profile-pick-{}", profile.get())
            };
            let key = Key::Segment(profile.get());
            // The digit's tone eases with the capsule that slides under it, so
            // the mark and its digit change together.
            let tint = self.motion.selection_at(key, is_edited, now);
            let colour = theme::mix(t.text_2, t.accent_text, tint);
            let face = container(ui_text(profile.get().to_string()).size(13).color(colour))
                .width(SEGMENT_WIDTH)
                .height(SEGMENT_HEIGHT)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .id(id);
            // The selected cell keeps its capsule; only the other two fade a
            // hover surface over the track.
            let mut cell = self
                .control(face, theme::segment_surface(*t, !is_edited))
                .padding(0)
                .width(SEGMENT_WIDTH)
                .height(SEGMENT_HEIGHT)
                .disabled(blocked.is_some());
            if blocked.is_none() {
                cell = cell.on_press(Message::Machine(Msg::PickProfile(profile)));
            }
            Element::from(cell)
        }))
        .spacing(SEGMENT_GAP);
        let cells = container(cells)
            .width(TRACK_WIDTH)
            .height(TRACK_HEIGHT)
            .padding(Padding {
                top: TRACK_PADDING,
                left: TRACK_PADDING,
                right: 0.0,
                bottom: 0.0,
            });

        stack![track, capsule, cells].into()
    }

    /// The settings rows under one dot that slides to the open section.
    fn section_rows(&self, scheme: Scheme) -> Element<'_, Message> {
        let machine = &self.machine;
        let t = scheme.tokens();
        let now = self.motion.now();
        let sections = [
            Section::Performance,
            Section::Lighting,
            Section::Buttons,
            Section::Device,
        ];
        let rows: Vec<Element<'_, Message>> = sections
            .into_iter()
            .enumerate()
            .map(|(index, section)| {
                let current = section == machine.section();
                let label = format!(
                    "{}{}",
                    section_label(section),
                    if machine.pending(section) { " •" } else { "" }
                );
                let key = Key::SectionRow(index);
                // The open row's fill and label ease onto the new section, so
                // they land with the dot that slides to them.
                let selected = self.motion.selection_at(key, current, now);
                let content = container(
                    row![
                        space().width(DOT_COLUMN),
                        ui_text(label)
                            .size(13)
                            .color(theme::mix(t.text, t.accent_text, selected)),
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .height(SECTION_ROW_HEIGHT)
                .center_y(Length::Fill)
                .align_x(iced::Alignment::Start);
                let row = self
                    .control(content, theme::nav_surface(*t, selected))
                    .padding(0)
                    .width(Length::Fill)
                    .height(SECTION_ROW_HEIGHT)
                    .on_press(Message::Machine(Msg::Select(section)));
                row.into()
            })
            .collect();
        let rows = column(rows).spacing(SECTION_ROW_GAP).width(Length::Fill);

        let base = dot_offset(section_index(machine.section()));
        let dot_at = if self.base_moves() && self.motion.dot.animating(now) {
            self.motion.dot.at(now)
        } else {
            base
        };
        let dot = container(space())
            .width(DOT_SIZE)
            .height(DOT_SIZE)
            .id("section-dot")
            .style(move |_theme| container::Style {
                background: Some(Background::Color(t.accent)),
                border: Border {
                    radius: 999.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            });
        let dot = column![space().height(base), dot];
        let shift = dot_at - base;
        let dot: Element<'_, Message> = Float::new(dot)
            .translate(move |_, _| Vector::new(0.0, shift))
            .into();

        stack![rows, dot].into()
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
        let look = Look {
            scheme,
            alpha: self.motion.section_alpha.at(self.motion.now()),
            rise: self.motion.section_rise.at(self.motion.now()),
        };
        let body = match section {
            Section::Performance => self.performance_pane(look),
            Section::Lighting => self.lighting_pane(look),
            Section::Buttons => self.buttons_pane(look),
            Section::Device => self.device_pane(look),
        };
        let cards = scrollable(container(body).padding([12.0, 16.0]))
            .style(theme::scrollable_at(look.tokens()))
            .height(Length::Fill);
        // The rise is a transform, never layout: the cards' hit boxes ride with
        // them, and at rest the float draws inline. Under a prompt it settles
        // to its place, so it never paints above the scrim.
        let rise = if self.base_moves() { look.rise } else { 0.0 };
        let cards: Element<'_, Message> = Float::new(cards)
            .translate(move |_, _| Vector::new(0.0, rise))
            .into();
        if section == Section::Device {
            return cards;
        }
        let blocked = machine.apply_blocked();
        let nothing_pending = !machine.pending(section);
        let apply: Control<'_, Message> = {
            let mut control = self
                .control(
                    control_label("Apply"),
                    theme::filled_surface(*scheme.tokens()),
                )
                .height(CONTROL_HEIGHT)
                .disabled(blocked.is_some() || nothing_pending);
            if blocked.is_none() && !nothing_pending {
                control = control.on_press(Message::Machine(Msg::Apply));
            }
            control
        };
        let revert: Control<'_, Message> = {
            let mut control = self
                .control(
                    control_label("Revert"),
                    theme::quiet_surface(*scheme.tokens()),
                )
                .height(CONTROL_HEIGHT)
                .disabled(nothing_pending);
            if !nothing_pending {
                control = control.on_press(Message::Machine(Msg::Revert));
            }
            control
        };
        let notice = self.notice_line(scheme, 1.0);
        let changed = self.changed_line(scheme, 1.0);
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
        column![cards, footer].into()
    }

    /// The footer's (or the Device card's) notice: the state's, or the ghost of
    /// the one that just left, at `alpha` of the surface it sits on.
    fn notice_line(&self, scheme: Scheme, alpha: f32) -> Option<Element<'_, Message>> {
        let current = self.machine.notice().map(|notice| match notice {
            Notice::Done(message) => (message.as_str(), true),
            Notice::Failed(message) => (message.as_str(), false),
        });
        let (text, success, fade) = ghost_notice(&self.motion.notice, current, self.motion.now())?;
        let colour = if success {
            scheme.tokens().success
        } else {
            scheme.tokens().danger_text
        };
        Some(
            ui_text(capitalize(text))
                .size(13)
                .color(colour.scale_alpha(fade * alpha))
                .into(),
        )
    }

    /// The moved lines, or the ghost of the ones that just left.
    fn changed_line(&self, scheme: Scheme, alpha: f32) -> Option<Element<'_, Message>> {
        let current = {
            let lines = self.machine.changed();
            (!lines.is_empty()).then(|| lines.join("\n"))
        };
        let (text, fade) = ghost_line(&self.motion.changed, current.as_deref(), self.motion.now())?;
        Some(
            ui_text(capitalize(text))
                .size(13)
                .color(scheme.tokens().warning.scale_alpha(fade * alpha))
                .into(),
        )
    }

    /// One control in the base layer. Its hover fade and press sink live in the
    /// control widget's own state, read from the cursor on every event, so a
    /// control that vanishes, turns disabled or sits under the prompt's scrim
    /// keeps no stale surface. While a prompt is open the base draws at rest.
    fn control<'a>(
        &self,
        content: impl Into<Element<'a, Message>>,
        surface: theme::Surface<'a>,
    ) -> Control<'a, Message> {
        Control::new(content, surface, self.motion.prefs).frozen(self.machine.prompt().is_some())
    }

    /// A control inside the prompt itself: it draws above the scrim and keeps
    /// its pointer state.
    fn modal_control<'a>(
        &self,
        content: impl Into<Element<'a, Message>>,
        surface: theme::Surface<'a>,
    ) -> Control<'a, Message> {
        Control::new(content, surface, self.motion.prefs)
    }

    /// Whether the base layer may translate at all: an open prompt's scrim
    /// covers it, and a translated `Float` would paint above that scrim.
    fn base_moves(&self) -> bool {
        self.machine.prompt().is_none()
    }

    // --- Performance ----------------------------------------------------------

    fn performance_pane(&self, look: Look) -> Element<'_, Message> {
        let machine = &self.machine;
        let t = look.tokens();
        let stage_texts = machine.stage_texts().unwrap_or_default();
        let stage_colors = machine.stage_colors();
        // An unreadable memory means the colour is unknown outright; a readable
        // memory that never remembered one says which value is missing.
        let memory_broken = machine
            .snapshot()
            .and_then(|snap| snap.memory_error.as_ref())
            .is_some();

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
                        .style(theme::text_input_at(t))
                        .font(MONO_FONT)
                        .size(14)
                        .padding([9.0, 12.0])
                        .on_input(move |text| {
                            Message::Machine(Msg::Edit(Edit::DpiText(index, text)))
                        })
                        .width(VALUE_CONTROL_WIDTH)
                        .into(),
                ];
                if let Some(color) = remembered {
                    control_children.push(color_swatch(look, color));
                }
                control_children.push(
                    text_input("hex", &hex_value)
                        .style(theme::text_input_at(t))
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
                        .width(HEX_WIDTH)
                        .into(),
                );
                if stage_texts.len() > 1 {
                    let remove = self
                        .control(control_label("Remove"), theme::quiet_surface(t))
                        .height(CONTROL_HEIGHT)
                        .on_press(Message::Machine(Msg::Edit(Edit::RemoveStage(index))));
                    control_children.push(remove.into());
                }
                let control = row(control_children)
                    .spacing(8)
                    .align_y(iced::Alignment::Center);
                let tags = match (remembered.is_none(), memory_broken) {
                    (true, true) => vec!["Unknown"],
                    (true, false) => vec!["Colour unknown"],
                    (false, _) => Vec::new(),
                };
                setting_row(
                    look,
                    format!("Stage {}", index + 1),
                    control.into(),
                    Element::from(space()),
                    tags,
                )
            })
            .collect();

        let add_stage: Option<Element<'_, Message>> =
            if stage_texts.len() < gctl_proto::DpiStages::MAX {
                let add = self
                    .control(control_label("Add stage"), theme::secondary_surface(t))
                    .height(CONTROL_HEIGHT)
                    .on_press(Message::Machine(Msg::Edit(Edit::AddStage)));
                Some(add.into())
            } else {
                None
            };

        let stage_count = stage_texts.len();
        let active_options: Vec<String> = (1..=stage_count)
            .filter_map(|stage| u8::try_from(stage).ok())
            .filter_map(|stage| parse::dpi_stage_number(stage).ok())
            .map(parse::format_dpi_stage)
            .collect();
        let active_selected = machine
            .active_stage()
            .map(|stage| usize::from(stage).saturating_sub(1));
        let active_labels = active_options.clone();
        let active_stage = dropdown(
            look,
            self.motion.prefs,
            self.machine.prompt().is_some(),
            Field {
                id: "active-stage-picker".to_owned(),
                width: VALUE_CONTROL_WIDTH,
                placeholder: "Select",
                font: MONO_FONT,
            },
            active_options,
            active_selected,
            move |index| {
                active_labels
                    .get(index)
                    .and_then(|label| parse::dpi_stage(label).ok())
                    .map(|stage| {
                        Message::Machine(Msg::Edit(Edit::ActiveStage(parse::dpi_stage_value(
                            stage,
                        ))))
                    })
            },
        );

        let polling_options = POLLING_OPTIONS.clone();
        let polling_selected = machine.polling_rate().and_then(|rate| {
            let label = polling_label(rate);
            polling_options.iter().position(|option| *option == label)
        });
        let polling_labels = polling_options.clone();
        let polling = dropdown(
            look,
            self.motion.prefs,
            self.machine.prompt().is_some(),
            Field {
                id: "polling-picker".to_owned(),
                width: VALUE_CONTROL_WIDTH,
                placeholder: "Select",
                font: MONO_FONT,
            },
            polling_options,
            polling_selected,
            move |index| {
                polling_labels
                    .get(index)
                    .map(|label| Message::Machine(Msg::Edit(polling_pick(label))))
            },
        );

        let lift_off_options = LIFT_OFF_OPTIONS.clone();
        let lift_off_selected = machine.lift_off().and_then(|lift_off| {
            let label = lift_off_label(lift_off);
            lift_off_options.iter().position(|option| *option == label)
        });
        let lift_off_labels = lift_off_options.clone();
        let lift_off = dropdown(
            look,
            self.motion.prefs,
            self.machine.prompt().is_some(),
            Field {
                id: "lift-off-picker".to_owned(),
                width: VALUE_CONTROL_WIDTH,
                placeholder: "Select",
                font: MONO_FONT,
            },
            lift_off_options,
            lift_off_selected,
            move |index| {
                lift_off_labels
                    .get(index)
                    .map(|label| Message::Machine(Msg::Edit(lift_off_pick(label))))
            },
        );

        let debounce = machine.debounce().unwrap_or(0);
        let debounce_control: Element<'_, Message> = slider(0..=16, debounce, |ms| {
            Message::Machine(Msg::Edit(Edit::Debounce(ms)))
        })
        .style(theme::slider_at(t))
        .width(Length::Fill)
        .height(CONTROL_HEIGHT)
        .into();
        let debounce_readout: Element<'_, Message> =
            ui_text(format!("{debounce} ms")).font(MONO_FONT).into();

        let never = machine.sleep_never().unwrap_or(false);
        let minutes = text_input("minutes", &machine.sleep_minutes().unwrap_or_default())
            .style(theme::text_input_at(t))
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
            .style(theme::text_input_at(t))
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
                .style(theme::checkbox_at(t))
                .on_toggle(|never| Message::Machine(Msg::Edit(Edit::SleepNever(never)))),
            ui_text("Never").size(14).color(t.text),
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

        let mut dpi_body = column![column(stage_rows).spacing(6)].spacing(10);
        if let Some(add) = add_stage {
            dpi_body = dpi_body.push(setting_row(
                look,
                "",
                add,
                Element::from(space()),
                Vec::new(),
            ));
        }
        dpi_body = dpi_body.push(setting_row(
            look,
            "Active stage",
            active_stage,
            Element::from(space()),
            vec!["per profile"],
        ));
        let dpi_card = card(look, "DPI", dpi_tags, dpi_body);

        let sensor_card = card(
            look,
            "Sensor",
            Vec::new(),
            column![
                setting_row(
                    look,
                    "Polling rate",
                    polling,
                    Element::from(space()),
                    vec!["all profiles"]
                ),
                setting_row(
                    look,
                    "Lift-off",
                    lift_off,
                    Element::from(space()),
                    vec!["all profiles"]
                ),
                setting_row(
                    look,
                    "Debounce",
                    debounce_control,
                    debounce_readout,
                    vec!["per profile"]
                ),
                setting_row(
                    look,
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

    fn lighting_pane(&self, look: Look) -> Element<'_, Message> {
        let machine = &self.machine;
        let t = look.tokens();
        let kind = machine.effect_kind();

        let effect_options: Vec<String> = parse::EFFECT_NAMES
            .iter()
            .map(|token| parse::effect_label(token))
            .collect();
        let effect_selected = kind.and_then(|kind| {
            parse::EFFECT_NAMES
                .iter()
                .position(|token| *token == kind.token())
        });
        let effect_picker = dropdown(
            look,
            self.motion.prefs,
            self.machine.prompt().is_some(),
            Field {
                id: "effect-picker".to_owned(),
                width: VALUE_CONTROL_WIDTH,
                placeholder: "Select",
                font: UI_FONT,
            },
            effect_options,
            effect_selected,
            |index| {
                parse::EFFECT_NAMES
                    .get(index)
                    .copied()
                    .and_then(EffectKind::from_token)
                    .map(|kind| Message::Machine(Msg::Edit(Edit::Effect(kind))))
            },
        );
        let effect_row: Element<'_, Message> = effect_picker;

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
        let takes_colors = takes_colours(kind);

        let rate_row: Option<(Element<'_, Message>, Element<'_, Message>)> = if takes_rate {
            let rate = machine.effect_rate().unwrap_or(0);
            Some((
                slider(0..=100, rate, |rate| {
                    Message::Machine(Msg::Edit(Edit::Rate(rate)))
                })
                .style(theme::slider_at(t))
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
            let max = max_effect_colors(kind);
            let mut rows: Vec<Element<'_, Message>> = (0..colors.len())
                .map(|index| {
                    let remembered = colors[index];
                    let hex = machine.effect_color_hex(index);
                    let hex_value = if hex.is_empty() {
                        parse::format_color(remembered)
                    } else {
                        hex
                    };
                    let remove: Option<Element<'_, Message>> = if colors.len()
                        > min_effect_colors(kind)
                    {
                        let remove = self
                            .control(control_label("Remove"), theme::quiet_surface(t))
                            .height(CONTROL_HEIGHT)
                            .on_press(Message::Machine(Msg::Edit(Edit::RemoveEffectColor(index))));
                        Some(remove.into())
                    } else {
                        None
                    };
                    let mut control = row![
                        color_swatch(look, remembered),
                        text_input("hex", &hex_value)
                            .style(theme::text_input_at(t))
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
                            .width(HEX_WIDTH),
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
                let add = self
                    .control(control_label("Add colour"), theme::secondary_surface(t))
                    .height(CONTROL_HEIGHT)
                    .on_press(Message::Machine(Msg::Edit(Edit::AddEffectColor)));
                rows.push(add.into());
            }
            rows
        } else {
            Vec::new()
        };

        let (wired_control, wired_readout, wired_unknown) = brightness_row(
            look,
            "wired-slider",
            machine.brightness_wired(),
            |value| Message::Machine(Msg::Edit(Edit::BrightnessWired(value))),
            Edit::BrightnessWiredRelease,
        );
        let (wireless_control, wireless_readout, wireless_unknown) = brightness_row(
            look,
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
            look,
            "Effect",
            effect_row,
            Element::from(space()),
            effect_tags
        )]
        .spacing(10);
        if let Some((rate_control, rate_readout)) = rate_row {
            effect_body = effect_body.push(setting_row(
                look,
                "Speed",
                rate_control,
                rate_readout,
                Vec::new(),
            ));
        }
        if takes_colors {
            effect_body = effect_body.push(setting_row(
                look,
                "Colours",
                column(color_rows).spacing(6).into(),
                Element::from(space()),
                Vec::new(),
            ));
        }
        let effect_card = card(look, "Effect", Vec::new(), effect_body);

        let brightness_card = card(
            look,
            "Brightness",
            Vec::new(),
            column![
                setting_row(look, "Wired", wired_control, wired_readout, wired_tags),
                setting_row(
                    look,
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

    fn buttons_pane(&self, look: Look) -> Element<'_, Message> {
        let machine = &self.machine;
        let t = look.tokens();
        let rows: Vec<Element<'_, Message>> = crate::get::BUTTONS
            .iter()
            .map(|button| self.bind_row(look, *button))
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
            .style(theme::toggler_at(t))
            .on_toggle(|invert| Message::Machine(Msg::Edit(Edit::Scroll(scroll_of(invert)))));
        // Unknown draws dimmed, like the unknown brightness sliders, so an OFF
        // toggler never reads as `default`.
        let toggler: Element<'_, Message> = if scroll.is_none() {
            toggler.style(theme::dim_toggler_at(t)).into()
        } else {
            toggler.into()
        };

        let scroll_row: Element<'_, Message> = container(
            row![toggler, ui_text("Invert scroll").size(14).color(t.text)]
                .spacing(8)
                .align_y(iced::Alignment::Center),
        )
        .center_y(CONTROL_HEIGHT)
        .into();

        let scroll_card = card(
            look,
            "Scroll",
            Vec::new(),
            setting_row(
                look,
                "Scroll",
                scroll_row,
                Element::from(space()),
                scroll_tags,
            ),
        );
        let buttons_card = card(look, "Buttons", Vec::new(), column(rows).spacing(8));

        column![scroll_card, buttons_card].spacing(16).into()
    }

    fn bind_row(&self, look: Look, button: Button) -> Element<'_, Message> {
        let machine = &self.machine;
        let t = look.tokens();
        let binding = machine.bind(button);
        let label = button_label(button);
        let remembered_bind = machine.snapshot().and_then(|snap| snap.bind(button));
        let category = binding_category(binding);

        let category_options: Vec<String> = parse::BIND_FAMILY_TOKENS
            .iter()
            .map(|token| (*token).to_owned())
            .collect();
        let category_selected = category.and_then(|category| {
            let token = parse::family_token(category);
            category_options.iter().position(|option| option == token)
        });
        let category_picker = dropdown(
            look,
            self.motion.prefs,
            self.machine.prompt().is_some(),
            Field {
                id: format!("bind-category-picker-{}", parse::format_button(button)),
                width: KEY_PICKER_WIDTH,
                placeholder: "Select",
                font: UI_FONT,
            },
            category_options,
            category_selected,
            move |index| {
                parse::BIND_FAMILY_TOKENS
                    .get(index)
                    .copied()
                    .and_then(parse::bind_category)
                    .map(|category| Message::Machine(Msg::Edit(default_binding(button, category))))
            },
        );
        // An unknown bind reads `Unknown` as a tag in the tag column, so a
        // selector can count it, never as an uncountable picker placeholder.
        let category_cell: Element<'_, Message> = category_picker;

        let control: Element<'_, Message> = match category {
            None | Some(parse::BindCategory::None) => row![category_cell]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .into(),
            Some(parse::BindCategory::Mouse) => row![
                category_cell,
                self.fn_picker(
                    look,
                    &parse::MOUSE_FN_NAMES,
                    parse::BindCategory::Mouse,
                    binding,
                    button
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into(),
            Some(parse::BindCategory::Media) => row![
                category_cell,
                self.fn_picker(
                    look,
                    &parse::MEDIA_FN_NAMES,
                    parse::BindCategory::Media,
                    binding,
                    button
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into(),
            Some(parse::BindCategory::Dpi) => row![
                category_cell,
                self.fn_picker(
                    look,
                    &parse::DPI_FN_NAMES,
                    parse::BindCategory::Dpi,
                    binding,
                    button
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into(),
            Some(parse::BindCategory::Keyboard) => row![
                category_cell,
                self.fn_picker(
                    look,
                    &parse::KEYBOARD_FN_NAMES,
                    parse::BindCategory::Keyboard,
                    binding,
                    button
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
                        look,
                        label,
                        row![category_cell]
                            .spacing(8)
                            .align_y(iced::Alignment::Center)
                            .into(),
                        Element::from(space()),
                        Vec::new(),
                    );
                };
                let key_options = key_names();
                let current_key = key.name();
                let key_selected = key_options.iter().position(|name| *name == current_key);
                let key_picker = dropdown(
                    look,
                    self.motion.prefs,
                    self.machine.prompt().is_some(),
                    Field {
                        id: format!("key-picker-{}", parse::format_button(button)),
                        width: KEY_PICKER_WIDTH,
                        placeholder: "Select",
                        font: UI_FONT,
                    },
                    key_options,
                    key_selected,
                    move |index| {
                        gctl_proto::Key::all().nth(index).map(|key| {
                            Message::Machine(Msg::Edit(key_edit(button, key, modifiers)))
                        })
                    },
                );
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
                                .style(theme::checkbox_at(t))
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
        setting_row(look, label, control, Element::from(space()), tags)
    }

    /// A bind's function picker, built through the parse formatter so the
    /// picker cannot drift from the CLI's grammar.
    fn fn_picker(
        &self,
        look: Look,
        names: &'static [&'static str],
        family: parse::BindCategory,
        binding: Option<Binding>,
        button: Button,
    ) -> Element<'_, Message> {
        let family_token = parse::family_token(family);
        let options: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
        let current = binding.and_then(|binding| {
            let text = parse::format_binding(binding);
            let name = text.strip_prefix(family_token)?.strip_prefix(':')?;
            Some(name.to_owned())
        });
        let selected = current.and_then(|name| options.iter().position(|option| *option == name));
        dropdown(
            look,
            self.motion.prefs,
            self.machine.prompt().is_some(),
            Field {
                id: format!("bind-fn-picker-{}", parse::format_button(button)),
                width: KEY_PICKER_WIDTH,
                placeholder: "Select",
                font: UI_FONT,
            },
            options,
            selected,
            move |index| {
                let name = names.get(index)?;
                let edit = bind_edit(button, &format!("{family_token}:{name}"));
                Some(Message::Machine(Msg::Edit(edit)))
            },
        )
    }

    // --- Device ---------------------------------------------------------------

    fn device_pane(&self, look: Look) -> Element<'_, Message> {
        let machine = &self.machine;
        let t = look.tokens();
        let Some(snap) = machine.live_snapshot() else {
            return center(ui_text("Loading")).into();
        };
        let link = match snap.model.link {
            gctl_proto::Link::Wired => "Wired",
            gctl_proto::Link::Wireless => "Wireless",
        };
        let firmware = snap.firmware.to_string();
        let battery_control: Element<'_, Message> = match snap.battery.percent {
            Some(percent) => container(
                ui_text(format!("{percent}%"))
                    .size(14)
                    .font(MONO_FONT)
                    .color(t.text),
            )
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
            let mut control = self
                .control(
                    file_button_label(format!("Export profile {profile}")),
                    theme::secondary_surface(t),
                )
                .width(FILE_BUTTON_WIDTH)
                .height(CONTROL_HEIGHT)
                .disabled(blocked.is_some());
            if blocked.is_none() {
                control = control.on_press(Message::ExportClicked);
            }
            control.into()
        };
        let import_button: Element<'_, Message> = {
            let mut control = self
                .control(
                    file_button_label(format!("Import into profile {profile}")),
                    theme::secondary_surface(t),
                )
                .width(FILE_BUTTON_WIDTH)
                .height(CONTROL_HEIGHT)
                .disabled(blocked.is_some());
            if blocked.is_none() {
                control = control.on_press(Message::ImportClicked);
            }
            control.into()
        };
        let export_row: Element<'_, Message> = row![
            text_input("path.toml", &self.export_path)
                .style(theme::text_input_at(t))
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
                .style(theme::text_input_at(t))
                .size(14)
                .padding([9.0, 12.0])
                .on_input(Message::ImportPath)
                .width(Length::Fill),
            import_button,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into();

        let notice = self.notice_line(look.scheme, look.alpha);
        let changed = self.changed_line(look.scheme, look.alpha);
        let reason = blocked.map(|reason| ui_text(capitalize(&reason)).size(13).color(t.text_3));

        let mouse_card = card(
            look,
            "Mouse",
            Vec::new(),
            column![
                setting_row(
                    look,
                    "Model",
                    container(ui_text(snap.model.name).size(14).color(t.text))
                        .center_y(CONTROL_HEIGHT)
                        .into(),
                    Element::from(space()),
                    Vec::new()
                ),
                setting_row(
                    look,
                    "Link",
                    container(ui_text(link).size(14).color(t.text))
                        .center_y(CONTROL_HEIGHT)
                        .into(),
                    Element::from(space()),
                    Vec::new()
                ),
                setting_row(
                    look,
                    "Firmware",
                    container(ui_text(firmware).size(14).font(MONO_FONT).color(t.text))
                        .center_y(CONTROL_HEIGHT)
                        .into(),
                    Element::from(space()),
                    Vec::new()
                ),
                setting_row(
                    look,
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
                look,
                "Export profile",
                export_row,
                Element::from(space()),
                Vec::new()
            ),
            setting_row(
                look,
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
            look,
            "Profile file",
            Vec::new(),
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

/// A caps eyebrow as the view draws it: every character upper-cased with one
/// hair space between them, since iced has no letter-spacing.
fn eyebrow(word: &str) -> String {
    word.chars()
        .flat_map(char::to_uppercase)
        .map(String::from)
        .collect::<Vec<_>>()
        .join("\u{200A}")
}

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

/// One setting card: an eyebrow title over its rows, surface-1 with a 1px line.
fn card<'a>(
    look: Look,
    title: &'static str,
    tags: Vec<&'static str>,
    body: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(column![title_row(look, title, tags), body.into()].spacing(12))
        .style(theme::card_at(look.tokens()))
        .padding(16)
        .width(Length::Fill)
        .into()
}

/// The fixed widths of the setting grid's columns: label, readout and tags are
/// fixed, the control fills the space between label and readout. The same
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

/// A card's title row: the caps eyebrow in the label column, its marks pinned
/// to the card's right edge on the same row.
fn title_row<'a>(look: Look, title: &'static str, tags: Vec<&'static str>) -> Element<'a, Message> {
    let t = look.tokens();
    let label = container(
        ui_text(eyebrow(title))
            .size(EYEBROW_SIZE)
            .color(t.text_3)
            .width(LABEL_COLUMN),
    )
    .center_y(CONTROL_HEIGHT);
    let control = container(space()).width(Length::Fill);
    let readout = container(space()).width(READOUT_COLUMN);
    let tags = container(row(tags.into_iter().map(|mark| tag(look, mark))).spacing(4))
        .width(TAG_COLUMN)
        .align_x(iced::Alignment::End)
        .center_y(CONTROL_HEIGHT);
    row![label, control, readout, tags]
        .width(Length::Fill)
        .spacing(8)
        .align_y(iced::Alignment::Start)
        .into()
}

/// The one setting-row layout, a grid `label | control | readout | tags`: the
/// control column fills the space between the fixed label and readout columns,
/// and the tag column is pinned to the card's right edge. A control narrower
/// than its column sits at the column's left edge.
fn setting_row<'a>(
    look: Look,
    label: impl iced::widget::text::IntoFragment<'a>,
    control: Element<'a, Message>,
    readout: Element<'a, Message>,
    tags: Vec<&'static str>,
) -> Element<'a, Message> {
    let t = look.tokens();
    let label = container(ui_text(label).size(14).color(t.text_2).width(LABEL_COLUMN))
        .center_y(CONTROL_HEIGHT);
    let control = container(control).width(Length::Fill);
    let readout = container(readout)
        .width(READOUT_COLUMN)
        .center_y(CONTROL_HEIGHT);
    let tags = container(row(tags.into_iter().map(|mark| tag(look, mark))).spacing(4))
        .width(TAG_COLUMN)
        .align_x(iced::Alignment::End)
        .center_y(CONTROL_HEIGHT);
    row![label, control, readout, tags]
        .width(Length::Fill)
        .spacing(8)
        .align_y(iced::Alignment::Start)
        .into()
}

/// One dropdown field's own shape: the id a test finds it by, the grid width it
/// shares, and the face its labels use.
struct Field {
    id: String,
    width: f32,
    placeholder: &'static str,
    font: iced::Font,
}

/// gctl's own dropdown, wrapped in an id-bearing container of the width the
/// Performance grid shares. The trigger shows the selected label or a
/// placeholder, its hover and press living in the control that paints the
/// field; the panel is the widget's own overlay. An option is picked by its
/// index into `options`, so no caller compares a label back to a value.
fn dropdown<'a>(
    look: Look,
    prefs: Prefs,
    frozen: bool,
    field: Field,
    options: Vec<String>,
    selected: Option<usize>,
    on_select: impl FnMut(usize) -> Option<Message> + 'a,
) -> Element<'a, Message> {
    let t = look.tokens();
    let label = selected
        .and_then(|index| options.get(index))
        .cloned()
        .unwrap_or_else(|| field.placeholder.to_owned());
    let colour = if selected.is_some() { t.text } else { t.text_4 };
    let face = container(ui_text(label).size(14).font(field.font).color(colour))
        .padding([0.0, 12.0])
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);
    let trigger = Control::new(face, theme::field_surface(t), prefs)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(0)
        .frozen(frozen);
    container(Dropdown::new(
        dropdown::Paint {
            scheme: look.scheme,
            alpha: look.alpha,
            frozen,
        },
        prefs,
        trigger,
        options,
        selected,
        field.font,
        on_select,
    ))
    .id(field.id)
    .width(field.width)
    .height(CONTROL_HEIGHT)
    .into()
}

/// A neutral mark tag: text-3 on a quiet surface tint, pill radius.
fn tag(look: Look, mark: &'static str) -> Element<'static, Message> {
    let t = look.tokens();
    container(ui_text(mark).size(11).color(t.text_3))
        .style(theme::tag_at(t))
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

/// The battery's meter: a well track with an accent fill at the reading.
fn battery_meter(scheme: Scheme, percent: u8) -> Element<'static, Message> {
    let t = scheme.tokens();
    let width = 40.0;
    let fill = width * f32::from(percent.min(100)) / 100.0;
    let track = container(space())
        .width(width)
        .height(6)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.well)),
            border: Border {
                radius: 999.0.into(),
                width: 1.0,
                color: t.line,
            },
            ..container::Style::default()
        });
    let fill = container(space())
        .width(fill)
        .height(6)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.accent)),
            border: Border {
                radius: 999.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    stack![track, fill].into()
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

fn discard_prompt(
    scheme: Scheme,
    shell: &Shell,
    prompt: Prompt,
    current: Section,
) -> Element<'static, Message> {
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
    let keep = shell
        .modal_control(
            control_label("Keep editing"),
            theme::secondary_surface(*scheme.tokens()),
        )
        .height(CONTROL_HEIGHT)
        .on_press(Message::Machine(Msg::KeepEditing));
    let discard = shell
        .modal_control(
            control_label("Discard"),
            theme::danger_surface(*scheme.tokens()),
        )
        .height(CONTROL_HEIGHT)
        .on_press(Message::Machine(Msg::Discard));
    column![ui_text(message).size(20), row![keep, discard].spacing(12),]
        .spacing(12)
        .into()
}

fn import_prompt(scheme: Scheme, shell: &Shell, prompt: Prompt) -> Element<'static, Message> {
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
    let cancel = shell
        .modal_control(control_label("Cancel"), theme::secondary_surface(*t))
        .height(CONTROL_HEIGHT)
        .on_press(Message::Machine(Msg::ImportCancel));
    let confirm = shell
        .modal_control(control_label("Import"), theme::primary_surface(*t))
        .height(CONTROL_HEIGHT)
        .on_press(Message::Machine(Msg::ImportConfirm));
    column = column.push(row![cancel, confirm].spacing(12));
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

/// The rate a picker label names, parsed off the label's number through the
/// CLI grammar rather than compared against a formatted label.
fn polling_rate_of(label: &str) -> Option<gctl_proto::PollingRate> {
    parse::polling_rate(label.strip_suffix(" Hz")?).ok()
}

/// The distance a picker label names, parsed off the label's number through
/// the CLI grammar.
fn lift_off_of(label: &str) -> Option<gctl_proto::LiftOff> {
    parse::lift_off(label.strip_suffix(" mm")?).ok()
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

fn color_swatch(look: Look, color: gctl_proto::Rgb) -> Element<'static, Message> {
    let line = look.tokens().line;
    let alpha = look.alpha;
    container(space())
        .width(18)
        .height(18)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(
                Color::from_rgb8(color.r, color.g, color.b).scale_alpha(alpha),
            )),
            border: Border {
                radius: 4.0.into(),
                width: 1.0,
                color: line,
            },
            ..container::Style::default()
        })
        .into()
}

/// Whether the chosen effect draws colour rows at all.
fn takes_colours(kind: Option<EffectKind>) -> bool {
    matches!(
        kind,
        Some(EffectKind::Pulse | EffectKind::Rave | EffectKind::Solid | EffectKind::PulseOne)
    )
}

/// The most colours the chosen effect takes.
fn max_effect_colors(kind: Option<EffectKind>) -> usize {
    match kind {
        Some(EffectKind::Pulse) => 6,
        Some(EffectKind::Rave) => 2,
        Some(
            EffectKind::Off
            | EffectKind::Glorious
            | EffectKind::Cycle
            | EffectKind::Solid
            | EffectKind::PulseOne
            | EffectKind::Tail
            | EffectKind::Wave,
        )
        | None => 1,
    }
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

fn key_names() -> Vec<String> {
    gctl_proto::Key::all()
        .map(gctl_proto::Key::name)
        .map(str::to_owned)
        .collect()
}

/// One brightness row's grid cells: a dimmed slider with its handle at 0 and
/// `Unknown` in the tag column until the link is known, then a live slider and
/// the number readout. The unknown slider sends `release` on release, so a
/// click at 0 can set the link to 0 while a click that picked a value keeps it.
/// The slider carries `id`, so a test can find its bounds by layout id rather
/// than by the label.
fn brightness_row(
    look: Look,
    id: &'static str,
    value: Option<u8>,
    on_change: impl Fn(u8) -> Message + 'static,
    release: Edit,
) -> (Element<'static, Message>, Element<'static, Message>, bool) {
    let t = look.tokens();
    match value {
        Some(value) => {
            let slider = container(
                slider(0..=255, value, on_change)
                    .width(Length::Fill)
                    .height(CONTROL_HEIGHT)
                    .style(theme::slider_at(t)),
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
                    .style(theme::dim_slider_at(t))
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

#[cfg(test)]
#[path = "../../tests/unit/gui_motion_marks.rs"]
mod motion_marks;

#[cfg(test)]
#[path = "../../tests/unit/gui_motion_ghosts.rs"]
mod motion_ghosts;

#[cfg(test)]
#[path = "../../tests/unit/gui_motion_desktop.rs"]
mod motion_desktop;

#[cfg(test)]
#[path = "../../tests/unit/gui_motion_tints.rs"]
mod motion_tints;

#[cfg(test)]
#[path = "../../tests/unit/gui_layout_link_line.rs"]
mod layout_link_line;
