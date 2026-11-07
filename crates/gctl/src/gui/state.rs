//! The GUI's pure section state machine: an overlay of the user's edits over
//! the last snapshot, the write plan each Apply sends, the discard and import
//! prompts, and the one owner of "a transaction is running". No I/O and no
//! iced types.
//!
//! An edit is one overlay entry keyed by field. The window shows the entry's
//! value or else the snapshot's; only fields the user changed are in the
//! overlay. A reload replaces the snapshot and nothing else, so every unedited
//! field follows the mouse by construction. A write's answer patches the
//! snapshot with the ops that landed and takes the write-only half from the
//! memory file the write saved, then ends each entry its plan carried that now
//! equals the snapshot.

use std::path::PathBuf;

use gctl_proto::{
    Battery, Binding, Brightness, Button, Debounce, DpiStage, DpiStages, Effect, Firmware, LiftOff,
    Model, PollingRate, Profile, Rate, Rgb, ScrollDirection, SleepDelay,
};

use crate::get::BUTTONS;
use crate::memory::Memory;
use crate::parse;
use crate::writes::WriteOp;

/// The white a stage list uses to fill in unknown colours.
const WHITE: Rgb = Rgb {
    r: 255,
    g: 255,
    b: 255,
};

/// Everything one profile's window shows, read in one transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) model: &'static Model,
    pub(crate) battery: Battery,
    pub(crate) firmware: Firmware,
    pub(crate) active: Profile,
    pub(crate) profile: Profile,
    pub(crate) polling_rate: PollingRate,
    pub(crate) lift_off: LiftOff,
    pub(crate) sleep: SleepDelay,
    pub(crate) brightness: Option<Brightness>,
    pub(crate) scroll: Option<ScrollDirection>,
    pub(crate) dpi: DpiStages,
    pub(crate) dpi_stage: DpiStage,
    pub(crate) dpi_colors: Option<Vec<Rgb>>,
    pub(crate) debounce: Debounce,
    pub(crate) effect: Option<Effect>,
    /// The eight buttons in the order the CLI lists them; `None` = not remembered.
    pub(crate) binds: Vec<(Button, Option<Binding>)>,
    /// The write-only memory refused to load; the device half is still here.
    pub(crate) memory_error: Option<String>,
}

impl Snapshot {
    /// The remembered binding for `button`, or `None` when nobody remembers
    /// one. Look-up is by button, never by a position the `Vec` length does
    /// not enforce.
    pub(crate) fn bind(&self, button: Button) -> Option<Binding> {
        self.binds
            .iter()
            .find(|(candidate, _)| *candidate == button)
            .and_then(|(_, binding)| *binding)
    }
}

/// Why a snapshot read failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoadError {
    NoMouse,
    Asleep,
    Failed(String),
    Transport(String),
}

/// What the window currently shows for the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Connection {
    Loading,
    NoMouse,
    Asleep,
    Ready,
    Failed,
}

/// One content pane of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Section {
    Performance,
    Lighting,
    Buttons,
    Device,
}

/// What the import file's check found, for the confirmation prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImportCheck {
    pub(crate) note: Option<String>,
    pub(crate) device_wide: bool,
    /// The ops the file will write, in import order, so a running import knows
    /// which fields it carries (their entries compare at the answer) and a
    /// value change refuses the run.
    pub(crate) writes: Vec<WriteOp>,
}

/// A modal question blocking an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Prompt {
    Discard(Leave),
    Import(ImportCheck),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Leave {
    Section(Section),
    Profile(Profile),
}

/// The lighting effects, labelled with the CLI token title-cased.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EffectKind {
    Off,
    Glorious,
    Cycle,
    Pulse,
    Solid,
    PulseOne,
    Tail,
    Rave,
    Wave,
}

impl std::fmt::Display for EffectKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

impl EffectKind {
    pub(crate) const ALL: [EffectKind; 9] = [
        EffectKind::Off,
        EffectKind::Glorious,
        EffectKind::Cycle,
        EffectKind::Pulse,
        EffectKind::Solid,
        EffectKind::PulseOne,
        EffectKind::Tail,
        EffectKind::Rave,
        EffectKind::Wave,
    ];

    /// The CLI token title-cased: `pulse-one` -> `Pulse one`.
    pub(crate) fn label(self) -> String {
        parse::effect_label(self.token())
    }

    pub(crate) fn token(self) -> &'static str {
        match self {
            EffectKind::Off => "off",
            EffectKind::Glorious => "glorious",
            EffectKind::Cycle => "cycle",
            EffectKind::Pulse => "pulse",
            EffectKind::Solid => "solid",
            EffectKind::PulseOne => "pulse-one",
            EffectKind::Tail => "tail",
            EffectKind::Rave => "rave",
            EffectKind::Wave => "wave",
        }
    }

    /// The effect a CLI name names, or `None` for a name the CLI rejects.
    pub(crate) fn from_token(token: &str) -> Option<EffectKind> {
        Self::ALL.into_iter().find(|kind| kind.token() == token)
    }
}

/// One edit the user made, always a concrete typed value or raw field text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Edit {
    PollingRate(PollingRate),
    /// A picker offered a polling-rate label it could not map back; blocks Apply.
    InvalidPollingRate,
    LiftOff(LiftOff),
    /// A picker offered a lift-off label it could not map back; blocks Apply.
    InvalidLiftOff,
    Debounce(u8),
    SleepMinutes(String),
    SleepSeconds(String),
    SleepNever(bool),
    AddStage,
    RemoveStage(usize),
    DpiText(usize, String),
    ActiveStage(u8),
    // Constructed only by the state-machine unit tests; the window uses the hex forms.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "constructed only by the state tests")
    )]
    StageColor(usize, Rgb),
    StageColorHex(usize, String),
    Effect(EffectKind),
    Rate(u8),
    // Constructed only by the state-machine unit tests; the window uses the hex forms.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "constructed only by the state tests")
    )]
    EffectColor(usize, Rgb),
    EffectColorHex(usize, String),
    AddEffectColor,
    RemoveEffectColor(usize),
    BrightnessWired(u8),
    BrightnessWireless(u8),
    /// The brightness slider was released while its link had no value yet: set
    /// that link to 0. The unknown slider sends this instead of a value.
    BrightnessWiredRelease,
    BrightnessWirelessRelease,
    Bind(Button, Binding),
    /// A picker composed a bind text that failed to parse; blocks Apply.
    InvalidBind(Button),
    Scroll(ScrollDirection),
}

/// A result notice shown in the footer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Notice {
    Done(String),
    Failed(String),
}

/// A write's failure with the ops that landed before it. A fail-fast Apply or
/// import reports its landed prefix; a successful write's answer carries its
/// whole plan instead of a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WriteFailure {
    pub(crate) message: String,
    pub(crate) landed: Vec<WriteOp>,
    /// Whether the failing op handed at least one write report to the transport
    /// (a report that failed counts: it may have landed); the ops that landed
    /// before it are in `landed`. Only then do the plan's fields count as this
    /// window's own; a failing op that sent nothing owns nothing, even after
    /// earlier ops landed.
    pub(crate) reached: bool,
}

/// A message into the state machine, from the window or from a device task.
#[derive(Debug, Clone)]
pub(crate) enum Msg {
    Edit(Edit),
    Loaded(Result<Snapshot, LoadError>),
    Tick,
    Select(Section),
    PickProfile(Profile),
    MakeActive,
    Apply,
    Revert,
    KeepEditing,
    Discard,
    Applied {
        error: Option<WriteFailure>,
        snapshot: Result<Snapshot, LoadError>,
        /// The memory re-loaded after the write, when the device re-read failed.
        remembered: Option<Result<Memory, String>>,
    },
    Export(PathBuf),
    Exported(Result<PathBuf, String>),
    Import(PathBuf),
    ImportChecked(Result<ImportCheck, String>),
    ImportConfirm,
    ImportCancel,
    Imported {
        error: Option<String>,
        /// The writes that landed, on failure too (the ops before the failure).
        written: Vec<WriteOp>,
        /// The import's failing op handed at least one write report to the
        /// transport (the ops that landed before it are in `written`); `false`
        /// on success and on every failure before any report. Its check's
        /// fields count as this window's own only when this is `true` and the
        /// answer failed.
        reached: bool,
        /// `None` when `io::run_import` never ran under the lock (no mouse, a
        /// failed open, a lock failure), each with an empty `written`: an
        /// import that ran always re-reads. A task that died after it may have
        /// reached the mouse answers `Some(Err(..))`.
        snapshot: Option<Result<Snapshot, LoadError>>,
        /// The memory re-loaded after the write, when the device re-read failed.
        remembered: Option<Result<Memory, String>>,
    },
    LockWait,
    /// Closes the outside-change banner; clears the flag and starts nothing.
    DismissOutsideChange,
}

/// A device transaction the window must run off the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Request {
    Load(Option<Profile>),
    Apply {
        profile: Profile,
        ops: Vec<WriteOp>,
    },
    MakeActive(Profile),
    Export {
        profile: Profile,
        path: PathBuf,
    },
    PrepareImport {
        path: PathBuf,
    },
    Import {
        profile: Profile,
        path: PathBuf,
        check: ImportCheck,
    },
}

/// Which device transaction is running; the state starts nothing while one is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InFlight {
    Load,
    Write,
    Export,
    /// The import file check is running.
    ImportCheck,
    /// The import question is open, waiting for the user's answer.
    ImportQuestion,
    /// A confirmed import is writing, into this profile.
    Import(Profile),
}

/// What a user-started message is about to begin, for the one "may this start
/// now?" gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartKind {
    /// A fresh device transaction: Apply, make-active, export, import.
    Transaction,
    /// A profile pick: not a transaction, but refused while a write, import or
    /// export runs.
    Pick,
    /// The import question's confirm.
    ImportConfirm,
    /// A discard's follow-up: the profile switch's Load, or the queued
    /// import's run.
    FollowUp,
}

/// The overlay of edits over the snapshot, keyed by field.
#[derive(Debug, Default)]
struct Overlay {
    performance: PerformanceOverlay,
    lighting: LightingOverlay,
    buttons: ButtonsOverlay,
}

#[derive(Debug, Default)]
struct PerformanceOverlay {
    polling_rate: Option<PollingRate>,
    /// The polling-rate picker lost a value; blocks Apply until one is picked.
    invalid_polling: bool,
    lift_off: Option<LiftOff>,
    /// The lift-off picker lost a value; blocks Apply until one is picked.
    invalid_lift_off: bool,
    debounce: Option<u8>,
    sleep: Option<SleepOverlay>,
    /// The stage texts, colour list and per-slot hex texts are ONE unit: the
    /// first DPI edit captures them from the snapshot. The active stage is held
    /// only once the user picks one.
    dpi: Option<DpiOverlay>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SleepOverlay {
    never: bool,
    minutes: String,
    seconds: String,
}

impl SleepOverlay {
    fn from_snapshot(snap: &Snapshot) -> Self {
        let (never, minutes, seconds) = sleep_parts(snap.sleep);
        Self {
            never,
            minutes,
            seconds,
        }
    }

    /// The entry's value equals the snapshot's. An emptied minutes or seconds
    /// text never equals (it is an edit, not `0`); otherwise the parsed delay
    /// compares.
    fn matches(&self, snap: &Snapshot) -> bool {
        if self.never {
            return snap.sleep == SleepDelay::Never;
        }
        if self.minutes.is_empty() || self.seconds.is_empty() {
            return false;
        }
        sleep_delay(self).ok() == Some(snap.sleep)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DpiOverlay {
    /// One text value per stage; unparsed so an invalid entry stays visible.
    texts: Vec<String>,
    /// The user's pick, held only once they make one; until then Apply writes
    /// the latest read's stage, clamped to this entry's list.
    active_stage: Option<u8>,
    colors: Option<Vec<Rgb>>,
    /// The typed hex per stage colour: `None` = not edited (shows the colour),
    /// `Some("")` = emptied, `Some(text)` = typed. A typed text that does not
    /// parse (or is empty) is an edit that blocks Apply.
    hex: Vec<Option<String>>,
}

impl DpiOverlay {
    fn from_snapshot(snap: &Snapshot) -> Self {
        Self {
            texts: stage_texts(&snap.dpi),
            active_stage: None,
            colors: snap.dpi_colors.clone(),
            hex: vec![None; snap.dpi.as_slice().len()],
        }
    }

    /// The stage Apply writes: the user's pick, else the latest read's stage,
    /// clamped to the entry's stage count.
    fn effective_stage(&self, snap: &Snapshot) -> u8 {
        let stage = self.active_stage.unwrap_or(snap.dpi_stage.get());
        stage.min(self.texts.len() as u8)
    }

    /// The typed hex for a slot, or `None` when the slot was never edited.
    fn hex_slot(&self, index: usize) -> Option<&str> {
        match self.hex.get(index) {
            Some(Some(text)) => Some(text.as_str()),
            _ => None,
        }
    }

    fn hex_mut(&mut self) -> &mut Vec<Option<String>> {
        if self.hex.len() != self.texts.len() {
            self.hex.resize(self.texts.len(), None);
        }
        &mut self.hex
    }

    /// Whether the unit's value equals the snapshot's: parsed stage values,
    /// the active stage, and the resolved colour list. A typed hex that is
    /// emptied or does not parse never equals.
    fn matches(&self, snap: &Snapshot) -> bool {
        let mut parsed = Vec::with_capacity(self.texts.len());
        for text in &self.texts {
            match parse::dpi(text) {
                Ok(stage) => parsed.push(stage),
                Err(_) => return false,
            }
        }
        if parsed.as_slice() != snap.dpi.as_slice() {
            return false;
        }
        if self.effective_stage(snap) != snap.dpi_stage.get() {
            return false;
        }
        if self.colors != snap.dpi_colors {
            return false;
        }
        for hex in self.hex.iter().flatten() {
            if hex.is_empty() || parse::color(hex).is_err() {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Default)]
struct LightingOverlay {
    /// The effect kind, rate, colours and per-slot hex texts are ONE unit.
    effect: Option<EffectOverlay>,
    /// One entry with two `Option<u8>` links; a link stays `None` until touched.
    brightness: Option<BrightnessOverlay>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EffectOverlay {
    effect: Option<Effect>,
    /// The typed hex per effect colour: `None` = not edited, `Some("")` =
    /// emptied, `Some(text)` = typed.
    hex: Vec<Option<String>>,
}

impl EffectOverlay {
    fn from_snapshot(snap: &Snapshot) -> Self {
        Self {
            effect: snap.effect.clone(),
            hex: vec![None; effect_color_count(snap.effect.as_ref())],
        }
    }

    fn hex_slot(&self, index: usize) -> Option<&str> {
        match self.hex.get(index) {
            Some(Some(text)) => Some(text.as_str()),
            _ => None,
        }
    }

    fn hex_mut(&mut self, count: usize) -> &mut Vec<Option<String>> {
        if self.hex.len() != count {
            self.hex.resize(count, None);
        }
        &mut self.hex
    }

    /// The entry's value equals the snapshot's; a typed hex that is emptied or
    /// does not parse never equals (the resolved effect value already folds a
    /// valid hex's colour in).
    fn matches(&self, snap: &Snapshot) -> bool {
        if self.effect != snap.effect {
            return false;
        }
        for hex in self.hex.iter().flatten() {
            if hex.is_empty() || parse::color(hex).is_err() {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BrightnessOverlay {
    wired: Option<u8>,
    wireless: Option<u8>,
}

impl BrightnessOverlay {
    fn from_snapshot(snap: &Snapshot) -> Self {
        Self {
            wired: snap.brightness.map(|brightness| brightness.wired),
            wireless: snap.brightness.map(|brightness| brightness.wireless),
        }
    }

    fn matches(&self, snap: &Snapshot) -> bool {
        self.wired == snap.brightness.map(|brightness| brightness.wired)
            && self.wireless == snap.brightness.map(|brightness| brightness.wireless)
    }
}

#[derive(Debug)]
struct ButtonsOverlay {
    /// One entry per button, in the CLI's button order.
    binds: [Option<BindOverlay>; BUTTONS.len()],
    scroll: Option<ScrollDirection>,
}

impl Default for ButtonsOverlay {
    fn default() -> Self {
        Self {
            binds: std::array::from_fn(|_| None),
            scroll: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BindOverlay {
    /// The composed binding; kept on an invalid composition so the row still
    /// shows what the user had.
    binding: Option<Binding>,
    /// The picker composed a text that failed to parse; blocks Apply.
    invalid: bool,
}

impl BindOverlay {
    fn matches(&self, snap_binding: Option<Binding>) -> bool {
        !self.invalid && self.binding == snap_binding
    }
}

/// A field a write carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Carried {
    PollingRate,
    LiftOff,
    Sleep,
    Dpi,
    Debounce,
    Effect,
    Brightness,
    Bind(Button),
    Scroll,
}

/// What one good snapshot moved from the baseline: the previous good snapshot
/// with this window's landed ops applied. B draws `changed`, D reads `outside`.
#[derive(Debug, Default)]
struct Moved {
    /// One line per readable field under a pending entry that moved, stored
    /// with the entry it names so the line dies with that entry.
    changed: Vec<(Carried, String)>,
    /// A watched value moved that no landed op of this window explains.
    outside: bool,
}

/// The section state machine.
pub(crate) struct State {
    connection: Connection,
    snapshot: Option<Snapshot>,
    section: Section,
    edit_profile: Option<Profile>,
    overlay: Overlay,
    /// The in-flight Apply's plan, consumed by its answer to patch the snapshot
    /// with the ops that landed.
    carried_ops: Option<Vec<WriteOp>>,
    /// The fields a running import carries (its check's writes), for the
    /// edit-back exemption while it runs.
    import_carried: Option<Vec<Carried>>,
    /// The profile a running make-active targets; its answer sets `active`.
    make_active_target: Option<Profile>,
    in_flight: Option<InFlight>,
    prompt: Option<Prompt>,
    error: Option<String>,
    notice: Option<Notice>,
    import_path: Option<PathBuf>,
    /// An import check that answered while another prompt was open; it opens
    /// once that prompt closes.
    pending_import: Option<ImportCheck>,
    /// The lines of the last read that moved a field under a pending entry, in
    /// their own slot under the footer's notice, each stored with the entry it
    /// names.
    changed: Vec<(Carried, String)>,
    /// The fields a failed write's plan carried: its report may have reached
    /// the mouse, so the next good read explains them instead of reporting them.
    unsure_fields: Vec<Carried>,
    /// A watched value moved that no write of this window explains; only
    /// `DismissOutsideChange` clears it.
    outside: bool,
    reopen: bool,
    waiting: bool,
}

impl State {
    /// A fresh state that asks to load the active profile.
    pub(crate) fn new() -> (State, Vec<Request>) {
        (
            State {
                connection: Connection::Loading,
                snapshot: None,
                section: Section::Performance,
                edit_profile: None,
                overlay: Overlay::default(),
                carried_ops: None,
                import_carried: None,
                make_active_target: None,
                in_flight: Some(InFlight::Load),
                prompt: None,
                error: None,
                notice: None,
                import_path: None,
                pending_import: None,
                changed: Vec::new(),
                unsure_fields: Vec::new(),
                outside: false,
                reopen: true,
                waiting: false,
            },
            vec![Request::Load(None)],
        )
    }

    pub(crate) fn connection(&self) -> Connection {
        self.connection
    }

    pub(crate) fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    /// The snapshot the panes show: only one of the edited profile. A pick
    /// switches `edit_profile` at once, so the old profile's snapshot stays
    /// hidden while the picked profile's read is in flight.
    pub(crate) fn live_snapshot(&self) -> Option<&Snapshot> {
        let snap = self.snapshot.as_ref()?;
        match self.edit_profile {
            Some(editing) if snap.profile != editing => None,
            _ => Some(snap),
        }
    }

    pub(crate) fn section(&self) -> Section {
        self.section
    }

    pub(crate) fn edit_profile(&self) -> Option<Profile> {
        self.edit_profile
    }

    pub(crate) fn prompt(&self) -> Option<Prompt> {
        self.prompt.clone()
    }

    /// The error banner: a later failure first, then the memory refusal when it
    /// still stands, joined onto one banner.
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref().or_else(|| {
            self.snapshot
                .as_ref()
                .and_then(|snap| snap.memory_error.as_deref())
        })
    }

    /// Records a failure for the banner, joining the memory refusal below it
    /// when the snapshot still cannot read the write-only memory.
    fn set_banner_error(&mut self, error: Option<String>) {
        let memory = self
            .snapshot
            .as_ref()
            .and_then(|snap| snap.memory_error.as_deref());
        self.error = match (error, memory) {
            (Some(error), Some(memory)) => Some(format!("{error}\n{memory}")),
            (Some(error), None) => Some(error),
            (None, _) => None,
        };
    }

    /// Whether the next transaction should re-open the device handle.
    pub(crate) fn reopen(&self) -> bool {
        self.reopen
    }

    pub(crate) fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    /// The lines of the last read that moved a field under a pending entry, in
    /// row order; they draw in their own slot under the footer's notice. A line
    /// shows only while the entry it names still lives.
    pub(crate) fn changed(&self) -> Vec<String> {
        self.changed
            .iter()
            .filter(|(field, _)| self.entry_lives(*field))
            .map(|(_, line)| line.clone())
            .collect()
    }

    /// Whether the entry a stored moved line names is still in the overlay.
    fn entry_lives(&self, field: Carried) -> bool {
        let overlay = &self.overlay;
        match field {
            Carried::PollingRate => overlay.performance.polling_rate.is_some(),
            Carried::LiftOff => overlay.performance.lift_off.is_some(),
            Carried::Sleep => overlay.performance.sleep.is_some(),
            Carried::Dpi => overlay.performance.dpi.is_some(),
            Carried::Debounce => overlay.performance.debounce.is_some(),
            Carried::Effect => overlay.lighting.effect.is_some(),
            Carried::Brightness => overlay.lighting.brightness.is_some(),
            Carried::Bind(button) => BUTTONS
                .iter()
                .position(|b| *b == button)
                .is_some_and(|index| overlay.buttons.binds[index].is_some()),
            Carried::Scroll => overlay.buttons.scroll.is_some(),
        }
    }

    /// Clears the footer's slots for a user action: the notice and the moved
    /// lines both start fresh.
    fn clear_notice(&mut self) {
        self.notice = None;
        self.changed.clear();
    }

    /// Whether a readable value moved that no write of this window explains.
    /// Only `DismissOutsideChange` clears it.
    pub(crate) fn outside_change(&self) -> bool {
        self.outside
    }

    /// Whether the section has pending edits: its plan is non-empty or invalid.
    pub(crate) fn pending(&self, section: Section) -> bool {
        match self.plan(section) {
            Ok(ops) => !ops.is_empty(),
            Err(_) => true,
        }
    }

    /// The reason the in-flight transaction blocks a new one, or `None`.
    fn in_flight_reason(&self) -> Option<String> {
        if self.waiting && self.in_flight.is_some() {
            return Some("waiting for another gctl to finish".to_owned());
        }
        match self.in_flight {
            Some(InFlight::Load) => Some("reading the mouse".to_owned()),
            Some(InFlight::Write) | Some(InFlight::Import(_)) => {
                Some("writing to the mouse".to_owned())
            }
            Some(InFlight::Export) => Some("exporting".to_owned()),
            Some(InFlight::ImportCheck) => Some("checking the import file".to_owned()),
            Some(InFlight::ImportQuestion) => Some("waiting for the import answer".to_owned()),
            None => None,
        }
    }

    /// The reason an open question blocks a new user-started action, named by
    /// the question itself, or `None`.
    fn question_reason(&self) -> Option<String> {
        match &self.prompt {
            Some(Prompt::Import(_)) => Some("waiting for the import answer".to_owned()),
            Some(Prompt::Discard(_)) => Some("answering a question".to_owned()),
            None => None,
        }
    }

    /// The reason a user-started action is refused, or `None` when it may
    /// proceed. Every user-started message asks this first, and the view's
    /// disabled states read the same function. The background tick is not
    /// user-started and never calls it.
    fn start_refused(&self, kind: StartKind) -> Option<String> {
        match kind {
            StartKind::Pick => {
                // A pick during a read is not a transaction (it switches at
                // once); during a write, import or export it is refused before
                // the pending check.
                if matches!(self.in_flight, Some(InFlight::Load) | None) {
                    self.question_reason()
                } else {
                    self.in_flight_reason().or_else(|| self.question_reason())
                }
            }
            StartKind::Transaction => self.in_flight_reason().or_else(|| self.question_reason()),
            StartKind::ImportConfirm => {
                if matches!(self.prompt, Some(Prompt::Import(_)))
                    && matches!(self.in_flight, Some(InFlight::ImportQuestion))
                {
                    None
                } else {
                    self.in_flight_reason().or_else(|| self.question_reason())
                }
            }
            StartKind::FollowUp => {
                // A discard follow-up runs only with the question closed and
                // the line held by the discard's own state: nothing, a read
                // (whose answer is dropped and re-issued), or the queued
                // check's `ImportQuestion` marker. A write, import or export
                // can never be here.
                if self.prompt.is_some() {
                    self.question_reason()
                } else if matches!(self.in_flight, Some(InFlight::ImportQuestion)) {
                    None
                } else {
                    self.in_flight_reason()
                }
            }
        }
    }

    /// The reason Apply is blocked, or `None` when it can run.
    pub(crate) fn apply_blocked(&self) -> Option<String> {
        if let Some(reason) = self.start_refused(StartKind::Transaction) {
            return Some(reason);
        }
        if self.connection == Connection::Asleep {
            return Some("the mouse is asleep; wake it to apply".to_owned());
        }
        // The write-only memory refused to load: Apply of a remembered setting
        // cannot record its result, so it is refused with the CLI's own text.
        if let Some(memory) = self
            .snapshot
            .as_ref()
            .and_then(|snap| snap.memory_error.as_deref())
            && let Ok(ops) = self.plan(self.section)
            && ops.iter().any(WriteOp::needs_memory)
        {
            return Some(memory.to_owned());
        }
        self.plan(self.section).err()
    }

    /// The reason a profile pick is refused, or `None` when it may proceed. The
    /// sidebar's picker reads this so its buttons take no press while a write,
    /// import or export runs — the same gate the state's pick message asks.
    pub(crate) fn pick_blocked(&self) -> Option<String> {
        self.start_refused(StartKind::Pick)
    }

    /// The reason `make active` is blocked, or `None` when it can run. It needs
    /// an idle, awake mouse and a snapshot of the edited profile.
    pub(crate) fn make_active_blocked(&self) -> Option<String> {
        if let Some(reason) = self.start_refused(StartKind::Transaction) {
            return Some(reason);
        }
        if self.connection == Connection::Asleep {
            return Some("the mouse is asleep; wake it to change the active profile".to_owned());
        }
        if self.live_snapshot().is_none() {
            return Some("reading the mouse".to_owned());
        }
        None
    }

    /// The reason export or import is refused, or `None` when either may run.
    /// The Device pane's buttons and both start functions read this one gate.
    pub(crate) fn file_blocked(&self) -> Option<String> {
        if let Some(reason) = self.start_refused(StartKind::Transaction) {
            return Some(reason);
        }
        if self.connection == Connection::Asleep {
            return Some("the mouse is asleep; wake it to export or import".to_owned());
        }
        None
    }

    /// The writes this section's overlay asks for, in the CLI's write order.
    pub(crate) fn plan(&self, section: Section) -> Result<Vec<WriteOp>, String> {
        let Some(snap) = self.live_snapshot() else {
            return Ok(Vec::new());
        };
        match section {
            Section::Performance => plan_performance(&self.overlay.performance, snap),
            Section::Lighting => plan_lighting(&self.overlay.lighting, snap),
            Section::Buttons => plan_buttons(&self.overlay.buttons, snap),
            Section::Device => Ok(Vec::new()),
        }
    }

    /// Advances the state one message, returning the device requests it makes.
    pub(crate) fn update(&mut self, message: Msg) -> Vec<Request> {
        match message {
            Msg::Edit(edit) => {
                self.clear_notice();
                self.apply_edit(edit)
            }
            Msg::Loaded(result) => self.loaded(result),
            Msg::Tick => self.tick(),
            Msg::Select(section) => {
                self.clear_notice();
                self.select(section)
            }
            Msg::PickProfile(profile) => {
                self.clear_notice();
                self.pick_profile(profile)
            }
            Msg::MakeActive => {
                self.clear_notice();
                self.make_active()
            }
            Msg::Apply => {
                self.clear_notice();
                self.apply()
            }
            Msg::Revert => {
                self.clear_notice();
                self.revert();
                Vec::new()
            }
            Msg::KeepEditing => {
                if matches!(self.prompt, Some(Prompt::Discard(_))) {
                    self.prompt = None;
                    self.open_queued_import()
                } else {
                    Vec::new()
                }
            }
            Msg::Discard => self.discard(),
            Msg::Applied {
                error,
                snapshot,
                remembered,
            } => {
                self.applied(error, snapshot, remembered);
                Vec::new()
            }
            Msg::Export(path) => {
                self.clear_notice();
                self.export(path)
            }
            Msg::Exported(result) => {
                self.in_flight = None;
                self.waiting = false;
                self.notice = Some(match result {
                    Ok(path) => Notice::Done(format!("exported to {path:?}")),
                    Err(message) => Notice::Failed(message),
                });
                Vec::new()
            }
            Msg::Import(path) => {
                self.clear_notice();
                self.import(path)
            }
            Msg::ImportChecked(result) => self.import_checked(result),
            Msg::ImportConfirm => self.import_confirm(),
            Msg::ImportCancel => self.import_cancel(),
            Msg::Imported {
                error,
                written,
                reached,
                snapshot,
                remembered,
            } => {
                self.imported(error, written, reached, snapshot, remembered);
                Vec::new()
            }
            Msg::LockWait => {
                if self.in_flight.is_some() {
                    self.waiting = true;
                }
                Vec::new()
            }
            Msg::DismissOutsideChange => {
                self.outside = false;
                Vec::new()
            }
        }
    }

    // --- display values: the overlay when a field is edited, else the snapshot ---

    pub(crate) fn stage_texts(&self) -> Option<Vec<String>> {
        if let Some(dpi) = &self.overlay.performance.dpi {
            return Some(dpi.texts.clone());
        }
        self.live_snapshot().map(|snap| stage_texts(&snap.dpi))
    }

    pub(crate) fn active_stage(&self) -> Option<u8> {
        if let Some(dpi) = &self.overlay.performance.dpi {
            return self.live_snapshot().map(|snap| dpi.effective_stage(snap));
        }
        self.live_snapshot().map(|snap| snap.dpi_stage.get())
    }

    pub(crate) fn stage_colors(&self) -> Option<Vec<Rgb>> {
        if let Some(dpi) = &self.overlay.performance.dpi {
            return dpi.colors.clone();
        }
        self.live_snapshot()
            .and_then(|snap| snap.dpi_colors.clone())
    }

    /// The text one stage colour input shows: the typed hex (`Some`, emptied
    /// included), else the resolved colour, else empty.
    pub(crate) fn stage_color_hex(&self, index: usize) -> String {
        if let Some(dpi) = &self.overlay.performance.dpi {
            if let Some(text) = dpi.hex_slot(index) {
                return text.to_owned();
            }
            return dpi
                .colors
                .as_ref()
                .and_then(|colors| colors.get(index))
                .map(|color| parse::format_color(*color))
                .unwrap_or_default();
        }
        self.live_snapshot()
            .and_then(|snap| snap.dpi_colors.as_ref())
            .and_then(|colors| colors.get(index))
            .map(|color| parse::format_color(*color))
            .unwrap_or_default()
    }

    pub(crate) fn effect_color_hex(&self, index: usize) -> String {
        if let Some(entry) = &self.overlay.lighting.effect {
            if let Some(text) = entry.hex_slot(index) {
                return text.to_owned();
            }
            return effect_colors_of(entry.effect.as_ref())
                .get(index)
                .map(|color| parse::format_color(*color))
                .unwrap_or_default();
        }
        self.effect_colors()
            .and_then(|colors| colors.get(index).copied())
            .map(parse::format_color)
            .unwrap_or_default()
    }

    pub(crate) fn polling_rate(&self) -> Option<PollingRate> {
        if let Some(rate) = self.overlay.performance.polling_rate {
            return Some(rate);
        }
        self.live_snapshot().map(|snap| snap.polling_rate)
    }

    pub(crate) fn lift_off(&self) -> Option<LiftOff> {
        if let Some(lift_off) = self.overlay.performance.lift_off {
            return Some(lift_off);
        }
        self.live_snapshot().map(|snap| snap.lift_off)
    }

    pub(crate) fn debounce(&self) -> Option<u8> {
        if let Some(debounce) = self.overlay.performance.debounce {
            return Some(debounce);
        }
        self.live_snapshot().map(|snap| snap.debounce.get())
    }

    pub(crate) fn sleep_never(&self) -> Option<bool> {
        if let Some(sleep) = &self.overlay.performance.sleep {
            return Some(sleep.never);
        }
        self.live_snapshot()
            .map(|snap| snap.sleep == SleepDelay::Never)
    }

    pub(crate) fn sleep_minutes(&self) -> Option<String> {
        if let Some(sleep) = &self.overlay.performance.sleep {
            return Some(sleep.minutes.clone());
        }
        self.live_snapshot().map(|snap| match snap.sleep {
            SleepDelay::Never => String::new(),
            SleepDelay::Seconds(total) => (total / 60).to_string(),
        })
    }

    pub(crate) fn sleep_seconds(&self) -> Option<String> {
        if let Some(sleep) = &self.overlay.performance.sleep {
            return Some(sleep.seconds.clone());
        }
        self.live_snapshot().map(|snap| match snap.sleep {
            SleepDelay::Never => String::new(),
            SleepDelay::Seconds(total) => (total % 60).to_string(),
        })
    }

    fn effect(&self) -> Option<&Effect> {
        if let Some(entry) = &self.overlay.lighting.effect {
            return entry.effect.as_ref();
        }
        self.live_snapshot().and_then(|snap| snap.effect.as_ref())
    }

    pub(crate) fn effect_kind(&self) -> Option<EffectKind> {
        self.effect().map(kind_of)
    }

    pub(crate) fn effect_rate(&self) -> Option<u8> {
        self.effect()
            .map(|effect| effect_parts(Some(effect)).0.get())
    }

    pub(crate) fn effect_colors(&self) -> Option<Vec<Rgb>> {
        self.effect().map(|effect| effect_parts(Some(effect)).1)
    }

    pub(crate) fn brightness_wired(&self) -> Option<u8> {
        if let Some(brightness) = &self.overlay.lighting.brightness {
            return brightness.wired;
        }
        self.live_snapshot()
            .and_then(|snap| snap.brightness.map(|brightness| brightness.wired))
    }

    pub(crate) fn brightness_wireless(&self) -> Option<u8> {
        if let Some(brightness) = &self.overlay.lighting.brightness {
            return brightness.wireless;
        }
        self.live_snapshot()
            .and_then(|snap| snap.brightness.map(|brightness| brightness.wireless))
    }

    pub(crate) fn bind(&self, button: Button) -> Option<Binding> {
        let index = BUTTONS.iter().position(|b| *b == button)?;
        if let Some(bind) = &self.overlay.buttons.binds[index] {
            return bind.binding;
        }
        self.live_snapshot()?.bind(button)
    }

    pub(crate) fn scroll(&self) -> Option<ScrollDirection> {
        if let Some(direction) = self.overlay.buttons.scroll {
            return Some(direction);
        }
        self.live_snapshot().and_then(|snap| snap.scroll)
    }

    // --- message handlers -----------------------------------------------------

    fn loaded(&mut self, result: Result<Snapshot, LoadError>) -> Vec<Request> {
        self.in_flight = None;
        self.waiting = false;
        match &result {
            Ok(snap) => {
                // A read answering for a profile nobody is editing anymore is
                // dropped: the picked profile is loaded instead. The first
                // read has no `edit_profile`, so it is taken.
                if self
                    .edit_profile
                    .is_some_and(|editing| snap.profile != editing)
                {
                    self.in_flight = Some(InFlight::Load);
                    return vec![Request::Load(self.edit_profile)];
                }
                self.take_good(snap, &[]);
                self.reopen = false;
            }
            Err(LoadError::NoMouse) => {
                self.connection = Connection::NoMouse;
                self.error = None;
                self.reopen = true;
            }
            Err(LoadError::Asleep) => {
                self.connection = Connection::Asleep;
                self.error = None;
            }
            Err(LoadError::Failed(message)) => {
                self.connection = Connection::Failed;
                self.set_banner_error(Some(message.clone()));
            }
            Err(LoadError::Transport(message)) => {
                self.connection = Connection::Failed;
                self.set_banner_error(Some(message.clone()));
                self.reopen = true;
            }
        }
        Vec::new()
    }

    fn tick(&mut self) -> Vec<Request> {
        if self.in_flight.is_some() {
            return Vec::new();
        }
        self.in_flight = Some(InFlight::Load);
        // Load the edited profile, even while no mouse is open, so a reconnect
        // never switches the window to the active profile.
        vec![Request::Load(self.edit_profile)]
    }

    fn select(&mut self, section: Section) -> Vec<Request> {
        // Nothing replaces an open prompt; the discard and import questions are
        // each closed only by their own answers.
        if self.prompt.is_some() {
            return Vec::new();
        }
        if section == self.section {
            return Vec::new();
        }
        if self.pending(self.section) {
            self.prompt = Some(Prompt::Discard(Leave::Section(section)));
            Vec::new()
        } else {
            self.section = section;
            Vec::new()
        }
    }

    fn pick_profile(&mut self, profile: Profile) -> Vec<Request> {
        if Some(profile) == self.edit_profile || self.start_refused(StartKind::Pick).is_some() {
            return Vec::new();
        }
        if self.any_pending() {
            self.prompt = Some(Prompt::Discard(Leave::Profile(profile)));
            return Vec::new();
        }
        self.edit_profile = Some(profile);
        self.overlay = Overlay::default();
        if self.in_flight.is_none() {
            self.in_flight = Some(InFlight::Load);
            vec![Request::Load(Some(profile))]
        } else {
            // A pick during a read switches at once; the read's answer (for the
            // old profile) is dropped.
            Vec::new()
        }
    }

    fn any_pending(&self) -> bool {
        [
            Section::Performance,
            Section::Lighting,
            Section::Buttons,
            Section::Device,
        ]
        .iter()
        .any(|section| self.pending(*section))
    }

    fn make_active(&mut self) -> Vec<Request> {
        if self.make_active_blocked().is_some() {
            return Vec::new();
        }
        let Some(profile) = self.edit_profile else {
            return Vec::new();
        };
        let Some(snap) = self.live_snapshot() else {
            return Vec::new();
        };
        if profile == snap.active {
            return Vec::new();
        }
        self.make_active_target = Some(profile);
        self.in_flight = Some(InFlight::Write);
        vec![Request::MakeActive(profile)]
    }

    fn apply(&mut self) -> Vec<Request> {
        if self.apply_blocked().is_some() {
            return Vec::new();
        }
        let Ok(ops) = self.plan(self.section) else {
            return Vec::new();
        };
        if ops.is_empty() {
            return Vec::new();
        }
        let Some(profile) = self.edit_profile else {
            return Vec::new();
        };
        self.carried_ops = Some(ops.clone());
        self.in_flight = Some(InFlight::Write);
        vec![Request::Apply { profile, ops }]
    }

    fn export(&mut self, path: PathBuf) -> Vec<Request> {
        if self.file_blocked().is_some() {
            return Vec::new();
        }
        if path.as_os_str().is_empty() {
            self.notice = Some(Notice::Failed("Enter a file path.".to_owned()));
            return Vec::new();
        }
        let Some(profile) = self.edit_profile else {
            return Vec::new();
        };
        self.in_flight = Some(InFlight::Export);
        vec![Request::Export { profile, path }]
    }

    fn import(&mut self, path: PathBuf) -> Vec<Request> {
        if self.file_blocked().is_some() {
            return Vec::new();
        }
        if path.as_os_str().is_empty() {
            self.notice = Some(Notice::Failed("Enter a file path.".to_owned()));
            return Vec::new();
        }
        if self.edit_profile.is_none() {
            return Vec::new();
        }
        self.import_path = Some(path.clone());
        self.in_flight = Some(InFlight::ImportCheck);
        vec![Request::PrepareImport { path }]
    }

    fn import_checked(&mut self, result: Result<ImportCheck, String>) -> Vec<Request> {
        match result {
            Err(message) => {
                self.in_flight = None;
                self.waiting = false;
                self.import_path = None;
                self.import_carried = None;
                self.notice = Some(Notice::Failed(message));
                Vec::new()
            }
            Ok(check) => {
                if self.prompt.is_some() {
                    // Another question is open (a discard prompt): queue the
                    // check until that prompt closes. The line stays held, so
                    // nothing can start under the discard question.
                    self.pending_import = Some(check);
                    self.in_flight = Some(InFlight::ImportQuestion);
                    Vec::new()
                } else {
                    self.run_checked_import(check)
                }
            }
        }
    }

    /// Starts a checked import: a plain check runs at once, a device-wide or
    /// other-model check asks. The one branch, shared by the direct check and
    /// the queued check.
    fn run_checked_import(&mut self, check: ImportCheck) -> Vec<Request> {
        let Some(profile) = self.edit_profile else {
            self.in_flight = None;
            self.waiting = false;
            self.import_path = None;
            self.import_carried = None;
            return Vec::new();
        };
        let Some(path) = self.import_path.clone() else {
            self.in_flight = None;
            self.waiting = false;
            self.import_carried = None;
            return Vec::new();
        };
        if check.note.is_none() && !check.device_wide {
            self.import_carried = Some(carried_fields(&check.writes));
            self.in_flight = Some(InFlight::Import(profile));
            self.import_path = None;
            vec![Request::Import {
                profile,
                path,
                check,
            }]
        } else {
            self.prompt = Some(Prompt::Import(check));
            self.in_flight = Some(InFlight::ImportQuestion);
            Vec::new()
        }
    }

    /// Opens the import question queued behind a discard prompt, if any.
    fn open_queued_import(&mut self) -> Vec<Request> {
        if self.pending_import.is_none() {
            return Vec::new();
        }
        // The queued import's continuation is a follow-up: it runs only once
        // the question that held it closed and the line is free.
        if self.start_refused(StartKind::FollowUp).is_some() {
            return Vec::new();
        }
        let Some(check) = self.pending_import.take() else {
            return Vec::new();
        };
        self.run_checked_import(check)
    }

    fn import_confirm(&mut self) -> Vec<Request> {
        let Some(Prompt::Import(check)) = self.prompt.clone() else {
            return Vec::new();
        };
        if self.start_refused(StartKind::ImportConfirm).is_some() {
            return Vec::new();
        }
        let Some(profile) = self.edit_profile else {
            return Vec::new();
        };
        let Some(path) = self.import_path.take() else {
            return Vec::new();
        };
        self.import_carried = Some(carried_fields(&check.writes));
        self.prompt = None;
        self.in_flight = Some(InFlight::Import(profile));
        self.waiting = false;
        vec![Request::Import {
            profile,
            path,
            check,
        }]
    }

    fn import_cancel(&mut self) -> Vec<Request> {
        if matches!(self.prompt, Some(Prompt::Import(_))) {
            self.prompt = None;
            self.import_path = None;
            self.import_carried = None;
            self.in_flight = None;
            self.waiting = false;
        }
        Vec::new()
    }

    fn revert(&mut self) {
        match self.section {
            Section::Performance => self.overlay.performance = PerformanceOverlay::default(),
            Section::Lighting => self.overlay.lighting = LightingOverlay::default(),
            Section::Buttons => self.overlay.buttons = ButtonsOverlay::default(),
            Section::Device => {}
        }
    }

    fn discard(&mut self) -> Vec<Request> {
        let Some(prompt) = self.prompt.clone() else {
            return Vec::new();
        };
        match prompt {
            Prompt::Discard(Leave::Section(section)) => {
                self.prompt = None;
                self.revert();
                self.section = section;
                self.open_queued_import()
            }
            Prompt::Discard(Leave::Profile(profile)) => {
                self.prompt = None;
                self.overlay = Overlay::default();
                // Defensive: a pick is refused while an import runs, so no queued
                // import can be open here; clearing keeps the switch total.
                self.pending_import = None;
                self.import_path = None;
                self.import_carried = None;
                self.edit_profile = Some(profile);
                // The follow-up switch is a Load of the picked profile; start
                // it unless the line is still held (a read already running is
                // dropped and re-issued by `loaded`).
                if self.start_refused(StartKind::FollowUp).is_some() {
                    Vec::new()
                } else {
                    self.in_flight = Some(InFlight::Load);
                    vec![Request::Load(Some(profile))]
                }
            }
            Prompt::Import(_) => Vec::new(),
        }
    }

    fn applied(
        &mut self,
        error: Option<WriteFailure>,
        snapshot: Result<Snapshot, LoadError>,
        remembered: Option<Result<Memory, String>>,
    ) {
        self.in_flight = None;
        self.waiting = false;
        let plan = self.carried_ops.take().unwrap_or_default();
        // The ops that landed: the plan on success, the failed write's landed
        // prefix on failure. A make-active carries no ops (its answer changes
        // `active` only).
        let landed = match &error {
            None => plan.clone(),
            Some(failure) => failure.landed.clone(),
        };
        // A failed write's failing report may have reached the mouse: every
        // field the plan carried is this window's own for the re-read. A
        // failing op that sent nothing (no mouse, a failed open, a lock, a
        // refusal before its report) owns nothing, even after earlier ops
        // landed.
        if error.as_ref().is_some_and(|failure| failure.reached) {
            self.mark_unsure(&carried_fields(&plan));
        }
        let make_active_target = self.make_active_target.take();
        let (good, reload_error) = self.reload_result(snapshot, &landed);
        if !good {
            // A pick is refused while a write runs, so `edit_profile` is still
            // the profile this Apply targeted.
            self.patch_snapshot(&landed, &remembered, self.edit_profile);
        }
        self.end_carried(&carried_fields(&plan));
        if error.is_none()
            && let Some(target) = make_active_target
            && !good
            && let Some(snap) = &mut self.snapshot
        {
            snap.active = target;
        }
        self.set_banner_error(error.map(|failure| failure.message).or(reload_error));
    }

    fn imported(
        &mut self,
        error: Option<String>,
        written: Vec<WriteOp>,
        reached: bool,
        snapshot: Option<Result<Snapshot, LoadError>>,
        remembered: Option<Result<Memory, String>>,
    ) {
        // The running import carries the profile it wrote; a successful answer
        // names it in the notice.
        let import_profile = if let Some(InFlight::Import(profile)) = self.in_flight {
            Some(profile)
        } else {
            None
        };
        self.in_flight = None;
        self.waiting = false;
        self.import_path = None;
        let carried = self.import_carried.take().unwrap_or_default();
        // A failed import's check writes may have reached the mouse: their
        // fields are this window's own for the re-read. Only the failing op's
        // write report shows that; an answer without one owns nothing, though
        // `written` still holds the ops that landed.
        if error.is_some() && reached {
            self.mark_unsure(&carried);
        }
        self.notice = match &error {
            Some(message) => Some(Notice::Failed(message.clone())),
            None => import_profile
                .map(|profile| Notice::Done(format!("imported into profile {}", profile.get()))),
        };
        match snapshot {
            // No re-read: `io::run_import` never ran, so the failure is the
            // notice only. The patch and the end rule still run, as in the
            // re-read arm.
            None => {
                self.patch_snapshot(&written, &remembered, import_profile);
                let fields = import_carried_fields(carried, &written);
                self.end_carried(&fields);
            }
            Some(snapshot) => {
                let (good, reload_error) = self.reload_result(snapshot, &written);
                if !good {
                    self.patch_snapshot(&written, &remembered, import_profile);
                }
                // The check's writes plus the landed ops: the fields a running
                // import carried (whose entries compare at the answer).
                let fields = import_carried_fields(carried, &written);
                self.end_carried(&fields);
                // A re-read failure is a device failure for the banner; the
                // import's own failure stays a notice.
                self.set_banner_error(reload_error);
            }
        }
    }

    /// Ends each entry the carried set covers that now shows exactly the
    /// snapshot (the write-answer clause of the end rule).
    fn end_carried(&mut self, carried: &[Carried]) {
        let Some(snap) = self.snapshot.clone() else {
            return;
        };
        for field in carried {
            if field_matches(&self.overlay, &snap, *field) {
                clear_field(&mut self.overlay, *field);
            }
        }
    }

    /// Applies the ops that landed to the snapshot, so a write whose re-read
    /// failed shows its value at once. Device-readable fields come straight
    /// from the ops; the write-only half comes from the memory the write saved,
    /// re-loaded by the answer. `profile` is the profile the write targeted: a
    /// snapshot of another profile takes only the device-wide ops, never
    /// another profile's values.
    fn patch_snapshot(
        &mut self,
        ops: &[WriteOp],
        remembered: &Option<Result<Memory, String>>,
        profile: Option<Profile>,
    ) {
        let Some(snap) = &mut self.snapshot else {
            return;
        };
        let same_profile = Some(snap.profile) == profile;
        patch_ops(snap, ops, same_profile);
        match remembered {
            Some(Ok(memory)) => fill_write_only(snap, Some(memory), None),
            Some(Err(message)) => fill_write_only(snap, None, Some(message.clone())),
            None => {}
        }
    }

    /// Applies a re-read result: takes the snapshot, updates the connection and
    /// the re-open policy, and returns whether the re-read was a good snapshot
    /// plus the failure message for the banner (a failure that sets only the
    /// connection state has no banner message).
    fn reload_result(
        &mut self,
        snapshot: Result<Snapshot, LoadError>,
        landed: &[WriteOp],
    ) -> (bool, Option<String>) {
        match &snapshot {
            Ok(snap) => {
                self.take_good(snap, landed);
                self.reopen = false;
                (true, None)
            }
            Err(LoadError::Asleep) => {
                self.connection = Connection::Asleep;
                (false, None)
            }
            Err(LoadError::NoMouse) => {
                self.connection = Connection::NoMouse;
                self.reopen = true;
                (false, None)
            }
            Err(LoadError::Failed(message)) => {
                self.connection = Connection::Failed;
                (false, Some(message.clone()))
            }
            Err(LoadError::Transport(message)) => {
                self.connection = Connection::Failed;
                self.reopen = true;
                (false, Some(message.clone()))
            }
        }
    }

    /// Marks a failed write's fields as this window's own until the next good
    /// read explains them.
    fn mark_unsure(&mut self, fields: &[Carried]) {
        for field in fields {
            add_field(&mut self.unsure_fields, *field);
        }
    }

    /// Takes a good snapshot as the next baseline after comparing it with the
    /// old one: the moved lines and the D banner both read that one comparison.
    /// Every path that takes a good snapshot comes through here, with the ops
    /// this window landed since the old one.
    fn take_good(&mut self, snap: &Snapshot, landed: &[WriteOp]) {
        let moved = self.moved(snap, landed);
        self.take_snapshot(snap);
        self.unsure_fields.clear();
        if moved.outside {
            self.outside = true;
        }
        if !moved.changed.is_empty() {
            self.changed = moved.changed;
        }
    }

    /// The one comparison a good read makes: the new snapshot against the
    /// baseline, the previous good snapshot with this window's landed ops
    /// applied. Another model or link compares nothing (it becomes the next
    /// baseline); another profile compares the device-wide values only. A field
    /// a failed write carried is this window's own for this read.
    fn moved(&self, snap: &Snapshot, landed: &[WriteOp]) -> Moved {
        let Some(previous) = &self.snapshot else {
            return Moved::default();
        };
        if previous.model.name != snap.model.name
            || previous.model.product_id != snap.model.product_id
        {
            return Moved::default();
        }
        let same_profile = previous.profile == snap.profile;
        let mut baseline = previous.clone();
        patch_ops(&mut baseline, landed, same_profile);
        Moved {
            outside: (baseline.polling_rate != snap.polling_rate
                && !self.unsure(Carried::PollingRate))
                || (baseline.lift_off != snap.lift_off && !self.unsure(Carried::LiftOff))
                || (baseline.sleep != snap.sleep && !self.unsure(Carried::Sleep))
                || baseline.firmware != snap.firmware
                || (same_profile
                    && ((baseline.dpi != snap.dpi && !self.unsure(Carried::Dpi))
                        || (baseline.debounce != snap.debounce
                            && !self.unsure(Carried::Debounce)))),
            changed: if same_profile {
                self.changed_lines(&baseline, snap)
            } else {
                Vec::new()
            },
        }
    }

    /// Whether a failed write's report may have moved the field and the next
    /// good read must explain it instead of reporting it.
    fn unsure(&self, field: Carried) -> bool {
        self.unsure_fields.contains(&field)
    }

    /// The B lines: one per readable field a pending entry holds that this read
    /// moved, in row order, naming the mouse's new value. Each is stored with
    /// the entry it names.
    fn changed_lines(&self, baseline: &Snapshot, snap: &Snapshot) -> Vec<(Carried, String)> {
        let performance = &self.overlay.performance;
        let mut lines = Vec::new();
        if let Some(dpi) = &performance.dpi {
            if baseline.dpi != snap.dpi && !self.unsure(Carried::Dpi) {
                lines.push((
                    Carried::Dpi,
                    format!(
                        "DPI stages changed on the mouse to {}; your edit is kept",
                        parse::format_dpi_stages(&snap.dpi)
                    ),
                ));
            }
            if dpi.active_stage.is_some()
                && baseline.dpi_stage != snap.dpi_stage
                && !self.unsure(Carried::Dpi)
            {
                lines.push((
                    Carried::Dpi,
                    format!(
                        "active stage changed on the mouse to {}; your edit is kept",
                        parse::format_dpi_stage(snap.dpi_stage)
                    ),
                ));
            }
        }
        if performance.polling_rate.is_some()
            && baseline.polling_rate != snap.polling_rate
            && !self.unsure(Carried::PollingRate)
        {
            lines.push((
                Carried::PollingRate,
                format!(
                    "polling rate changed on the mouse to {} Hz; your edit is kept",
                    parse::format_polling_rate(snap.polling_rate)
                ),
            ));
        }
        if performance.lift_off.is_some()
            && baseline.lift_off != snap.lift_off
            && !self.unsure(Carried::LiftOff)
        {
            lines.push((
                Carried::LiftOff,
                format!(
                    "lift-off changed on the mouse to {} mm; your edit is kept",
                    parse::format_lift_off(snap.lift_off)
                ),
            ));
        }
        if performance.debounce.is_some()
            && baseline.debounce != snap.debounce
            && !self.unsure(Carried::Debounce)
        {
            lines.push((
                Carried::Debounce,
                format!(
                    "debounce changed on the mouse to {} ms; your edit is kept",
                    parse::format_debounce(snap.debounce)
                ),
            ));
        }
        if performance.sleep.is_some()
            && baseline.sleep != snap.sleep
            && !self.unsure(Carried::Sleep)
        {
            lines.push((
                Carried::Sleep,
                format!(
                    "sleep changed on the mouse to {}; your edit is kept",
                    parse::format_sleep(snap.sleep)
                ),
            ));
        }
        lines
    }

    /// Replaces the snapshot and nothing else. The overlay survives a reload;
    /// it is dropped only when the edited profile or the model name changes.
    fn take_snapshot(&mut self, snap: &Snapshot) {
        let changed = match &self.snapshot {
            Some(old) => old.profile != snap.profile || old.model.name != snap.model.name,
            None => true,
        };
        self.snapshot = Some(snap.clone());
        self.edit_profile = Some(snap.profile);
        self.connection = Connection::Ready;
        self.error = None;
        if changed {
            self.overlay = Overlay::default();
        }
    }

    fn apply_edit(&mut self, edit: Edit) -> Vec<Request> {
        let Some(snap) = self.live_snapshot().cloned() else {
            return Vec::new();
        };
        let field = match edit {
            Edit::PollingRate(rate) => {
                self.overlay.performance.polling_rate = Some(rate);
                self.overlay.performance.invalid_polling = false;
                Some(Carried::PollingRate)
            }
            Edit::InvalidPollingRate => {
                self.overlay.performance.invalid_polling = true;
                None
            }
            Edit::LiftOff(lift_off) => {
                self.overlay.performance.lift_off = Some(lift_off);
                self.overlay.performance.invalid_lift_off = false;
                Some(Carried::LiftOff)
            }
            Edit::InvalidLiftOff => {
                self.overlay.performance.invalid_lift_off = true;
                None
            }
            Edit::Debounce(debounce) => {
                self.overlay.performance.debounce = Some(debounce);
                Some(Carried::Debounce)
            }
            Edit::SleepMinutes(text) => {
                // While Never is ticked the minutes and seconds inputs take no
                // text (gui.md §Behaviour), so the state drops the edit
                // whatever the snapshot: unticking then shows the snapshot's
                // own minutes and seconds, never text typed under Never.
                if self.sleep_never().unwrap_or(false) {
                    None
                } else {
                    let sleep = self
                        .overlay
                        .performance
                        .sleep
                        .get_or_insert_with(|| SleepOverlay::from_snapshot(&snap));
                    sleep.minutes = text;
                    Some(Carried::Sleep)
                }
            }
            Edit::SleepSeconds(text) => {
                if self.sleep_never().unwrap_or(false) {
                    None
                } else {
                    let sleep = self
                        .overlay
                        .performance
                        .sleep
                        .get_or_insert_with(|| SleepOverlay::from_snapshot(&snap));
                    sleep.seconds = text;
                    Some(Carried::Sleep)
                }
            }
            Edit::SleepNever(never) => {
                // Unticking Never on an entry seeded from a Never snapshot fills
                // both sleep fields with 0; a user-emptied field still blocks.
                if self.overlay.performance.sleep.is_none()
                    && !never
                    && snap.sleep == SleepDelay::Never
                {
                    self.overlay.performance.sleep = Some(SleepOverlay {
                        never: false,
                        minutes: "0".to_owned(),
                        seconds: "0".to_owned(),
                    });
                } else {
                    let sleep = self
                        .overlay
                        .performance
                        .sleep
                        .get_or_insert_with(|| SleepOverlay::from_snapshot(&snap));
                    sleep.never = never;
                }
                Some(Carried::Sleep)
            }
            Edit::AddStage => {
                let dpi = self
                    .overlay
                    .performance
                    .dpi
                    .get_or_insert_with(|| DpiOverlay::from_snapshot(&snap));
                add_stage(dpi);
                Some(Carried::Dpi)
            }
            Edit::RemoveStage(index) => {
                let dpi = self
                    .overlay
                    .performance
                    .dpi
                    .get_or_insert_with(|| DpiOverlay::from_snapshot(&snap));
                remove_stage(dpi, index);
                Some(Carried::Dpi)
            }
            Edit::DpiText(index, text) => {
                let dpi = self
                    .overlay
                    .performance
                    .dpi
                    .get_or_insert_with(|| DpiOverlay::from_snapshot(&snap));
                if index < dpi.texts.len() {
                    dpi.texts[index] = text;
                }
                Some(Carried::Dpi)
            }
            Edit::ActiveStage(stage) => {
                let dpi = self
                    .overlay
                    .performance
                    .dpi
                    .get_or_insert_with(|| DpiOverlay::from_snapshot(&snap));
                if stage >= 1 && usize::from(stage) <= dpi.texts.len() {
                    dpi.active_stage = Some(stage);
                }
                Some(Carried::Dpi)
            }
            Edit::StageColor(index, color) => {
                let dpi = self
                    .overlay
                    .performance
                    .dpi
                    .get_or_insert_with(|| DpiOverlay::from_snapshot(&snap));
                set_stage_color(dpi, index, color);
                if index < dpi.texts.len() {
                    dpi.hex_mut()[index] = Some(parse::format_color(color));
                }
                Some(Carried::Dpi)
            }
            Edit::StageColorHex(index, text) => {
                let dpi = self
                    .overlay
                    .performance
                    .dpi
                    .get_or_insert_with(|| DpiOverlay::from_snapshot(&snap));
                if index < dpi.texts.len() {
                    dpi.hex_mut()[index] = Some(text.clone());
                    if let Ok(color) = parse::color(&text) {
                        set_stage_color(dpi, index, color);
                    }
                }
                Some(Carried::Dpi)
            }
            Edit::Effect(kind) => {
                let effect = self
                    .overlay
                    .lighting
                    .effect
                    .get_or_insert_with(|| EffectOverlay::from_snapshot(&snap));
                effect.effect = Some(switch_effect(effect.effect.as_ref(), kind));
                effect.hex.clear();
                Some(Carried::Effect)
            }
            Edit::Rate(value) => {
                let effect = self
                    .overlay
                    .lighting
                    .effect
                    .get_or_insert_with(|| EffectOverlay::from_snapshot(&snap));
                if let Some(effect_value) = effect.effect.as_mut() {
                    set_effect_rate(effect_value, rate_of(value));
                }
                Some(Carried::Effect)
            }
            Edit::EffectColor(index, color) => {
                let effect = self
                    .overlay
                    .lighting
                    .effect
                    .get_or_insert_with(|| EffectOverlay::from_snapshot(&snap));
                if let Some(effect_value) = effect.effect.as_mut() {
                    set_effect_color(effect_value, index, color);
                }
                let count = effect_color_count(effect.effect.as_ref());
                let hex = effect.hex_mut(count);
                if index < hex.len() {
                    hex[index] = Some(parse::format_color(color));
                }
                Some(Carried::Effect)
            }
            Edit::EffectColorHex(index, text) => {
                let effect = self
                    .overlay
                    .lighting
                    .effect
                    .get_or_insert_with(|| EffectOverlay::from_snapshot(&snap));
                let count = effect_color_count(effect.effect.as_ref());
                if index < count {
                    effect.hex_mut(count)[index] = Some(text.clone());
                }
                if let Ok(color) = parse::color(&text)
                    && let Some(effect_value) = effect.effect.as_mut()
                {
                    set_effect_color(effect_value, index, color);
                }
                Some(Carried::Effect)
            }
            Edit::AddEffectColor => {
                let effect = self
                    .overlay
                    .lighting
                    .effect
                    .get_or_insert_with(|| EffectOverlay::from_snapshot(&snap));
                if let Some(effect_value) = effect.effect.as_mut() {
                    add_effect_color(effect_value);
                }
                Some(Carried::Effect)
            }
            Edit::RemoveEffectColor(index) => {
                let effect = self
                    .overlay
                    .lighting
                    .effect
                    .get_or_insert_with(|| EffectOverlay::from_snapshot(&snap));
                if let Some(effect_value) = effect.effect.as_mut() {
                    remove_effect_color(effect_value, index);
                }
                if index < effect.hex.len() {
                    effect.hex.remove(index);
                }
                Some(Carried::Effect)
            }
            Edit::BrightnessWired(value) => {
                let brightness = self
                    .overlay
                    .lighting
                    .brightness
                    .get_or_insert_with(|| BrightnessOverlay::from_snapshot(&snap));
                brightness.wired = Some(value);
                Some(Carried::Brightness)
            }
            Edit::BrightnessWireless(value) => {
                let brightness = self
                    .overlay
                    .lighting
                    .brightness
                    .get_or_insert_with(|| BrightnessOverlay::from_snapshot(&snap));
                brightness.wireless = Some(value);
                Some(Carried::Brightness)
            }
            Edit::BrightnessWiredRelease => {
                if let Some(brightness) = &mut self.overlay.lighting.brightness {
                    if brightness.wired.is_none() {
                        brightness.wired = Some(0);
                    }
                } else if snap.brightness.is_none() {
                    self.overlay.lighting.brightness = Some(BrightnessOverlay {
                        wired: Some(0),
                        wireless: None,
                    });
                }
                None
            }
            Edit::BrightnessWirelessRelease => {
                if let Some(brightness) = &mut self.overlay.lighting.brightness {
                    if brightness.wireless.is_none() {
                        brightness.wireless = Some(0);
                    }
                } else if snap.brightness.is_none() {
                    self.overlay.lighting.brightness = Some(BrightnessOverlay {
                        wired: None,
                        wireless: Some(0),
                    });
                }
                None
            }
            Edit::Bind(button, binding) => {
                let Some(index) = BUTTONS.iter().position(|b| *b == button) else {
                    return Vec::new();
                };
                match &mut self.overlay.buttons.binds[index] {
                    Some(entry) => {
                        entry.binding = Some(binding);
                        entry.invalid = false;
                    }
                    slot @ None => {
                        *slot = Some(BindOverlay {
                            binding: Some(binding),
                            invalid: false,
                        });
                    }
                }
                Some(Carried::Bind(button))
            }
            Edit::InvalidBind(button) => {
                let Some(index) = BUTTONS.iter().position(|b| *b == button) else {
                    return Vec::new();
                };
                match &mut self.overlay.buttons.binds[index] {
                    Some(entry) => entry.invalid = true,
                    slot @ None => {
                        *slot = Some(BindOverlay {
                            binding: snap.bind(button),
                            invalid: true,
                        });
                    }
                }
                None
            }
            Edit::Scroll(direction) => {
                self.overlay.buttons.scroll = Some(direction);
                Some(Carried::Scroll)
            }
        };
        if let Some(field) = field {
            self.prune_edit_back(&snap, field);
        }
        Vec::new()
    }

    /// An edit that leaves the entry showing exactly the snapshot's value is a
    /// per-field revert. It holds while a running Apply or import carries the
    /// field (its answer compares against the new snapshot instead).
    fn prune_edit_back(&mut self, snap: &Snapshot, field: Carried) {
        if self.field_stale(field) {
            return;
        }
        if field_matches(&self.overlay, snap, field) {
            clear_field(&mut self.overlay, field);
        }
    }

    /// Whether the field's snapshot value is stale: a running write carries the
    /// field, so editing it back to the (about-to-change) snapshot value must
    /// not end the entry early.
    fn field_stale(&self, field: Carried) -> bool {
        if matches!(self.in_flight, Some(InFlight::Write))
            && self
                .carried_ops
                .as_ref()
                .is_some_and(|ops| carried_fields(ops).contains(&field))
        {
            return true;
        }
        matches!(self.in_flight, Some(InFlight::Import(_)))
            && self
                .import_carried
                .as_ref()
                .is_some_and(|c| c.contains(&field))
    }
}

/// The fields a set of writes carries, in the write order, each once.
fn carried_fields(ops: &[WriteOp]) -> Vec<Carried> {
    let mut fields = Vec::new();
    for op in ops {
        match op {
            WriteOp::PollingRate(_) => add_field(&mut fields, Carried::PollingRate),
            WriteOp::LiftOff(_) => add_field(&mut fields, Carried::LiftOff),
            WriteOp::Sleep(_) => add_field(&mut fields, Carried::Sleep),
            WriteOp::Brightness(_) => add_field(&mut fields, Carried::Brightness),
            WriteOp::Scroll(_) => {
                // a scroll write records the scroll-up/down binds too (cli.md).
                add_field(&mut fields, Carried::Scroll);
                add_field(&mut fields, Carried::Bind(Button::ScrollUp));
                add_field(&mut fields, Carried::Bind(Button::ScrollDown));
            }
            WriteOp::Dpi(_) | WriteOp::DpiStage(_) | WriteOp::DpiColors(_) => {
                add_field(&mut fields, Carried::Dpi);
            }
            WriteOp::Debounce(_) => add_field(&mut fields, Carried::Debounce),
            WriteOp::Effect(_) => add_field(&mut fields, Carried::Effect),
            WriteOp::Bind(button, _) => add_field(&mut fields, Carried::Bind(*button)),
        }
    }
    fields
}

fn add_field(fields: &mut Vec<Carried>, field: Carried) {
    if !fields.contains(&field) {
        fields.push(field);
    }
}

/// The fields a running import ends over: the check's writes plus the ops that
/// landed. Both answers build it, re-read or not.
fn import_carried_fields(carried: Vec<Carried>, written: &[WriteOp]) -> Vec<Carried> {
    let mut fields = carried;
    for field in carried_fields(written) {
        add_field(&mut fields, field);
    }
    fields
}

/// Whether the entry's value equals what the snapshot shows, for both the
/// write-answer settle and the edit-back revert.
fn field_matches(overlay: &Overlay, snap: &Snapshot, field: Carried) -> bool {
    match field {
        Carried::PollingRate => overlay.performance.polling_rate == Some(snap.polling_rate),
        Carried::LiftOff => overlay.performance.lift_off == Some(snap.lift_off),
        Carried::Sleep => overlay
            .performance
            .sleep
            .as_ref()
            .is_some_and(|sleep| sleep.matches(snap)),
        Carried::Dpi => overlay
            .performance
            .dpi
            .as_ref()
            .is_some_and(|dpi| dpi.matches(snap)),
        Carried::Debounce => overlay.performance.debounce == Some(snap.debounce.get()),
        Carried::Effect => overlay
            .lighting
            .effect
            .as_ref()
            .is_some_and(|effect| effect.matches(snap)),
        Carried::Brightness => overlay
            .lighting
            .brightness
            .as_ref()
            .is_some_and(|brightness| brightness.matches(snap)),
        Carried::Bind(button) => {
            let Some(index) = BUTTONS.iter().position(|b| *b == button) else {
                return false;
            };
            overlay.buttons.binds[index]
                .as_ref()
                .is_some_and(|bind| bind.matches(snap.bind(button)))
        }
        Carried::Scroll => overlay.buttons.scroll == snap.scroll,
    }
}

/// Removes an overlay entry, whatever it holds.
fn clear_field(overlay: &mut Overlay, field: Carried) {
    match field {
        Carried::PollingRate => overlay.performance.polling_rate = None,
        Carried::LiftOff => overlay.performance.lift_off = None,
        Carried::Sleep => overlay.performance.sleep = None,
        Carried::Dpi => overlay.performance.dpi = None,
        Carried::Debounce => overlay.performance.debounce = None,
        Carried::Effect => overlay.lighting.effect = None,
        Carried::Brightness => overlay.lighting.brightness = None,
        Carried::Bind(button) => {
            if let Some(index) = BUTTONS.iter().position(|b| *b == button) {
                overlay.buttons.binds[index] = None;
            }
        }
        Carried::Scroll => overlay.buttons.scroll = None,
    }
}

/// Applies the ops a write landed to a snapshot's device-readable fields. The
/// one place a plan's readable half becomes snapshot values, shared by the
/// failed-re-read patch and the baseline a good re-read compares against.
/// `same_profile` is whether the snapshot belongs to the profile the write
/// targeted: device-wide ops are every profile's and always apply, a
/// per-profile op applies only then.
fn patch_ops(snap: &mut Snapshot, ops: &[WriteOp], same_profile: bool) {
    for op in ops {
        if !same_profile && !op.device_wide() {
            continue;
        }
        match op {
            WriteOp::PollingRate(rate) => snap.polling_rate = *rate,
            WriteOp::LiftOff(lift_off) => snap.lift_off = *lift_off,
            WriteOp::Sleep(delay) => snap.sleep = *delay,
            WriteOp::Debounce(debounce) => snap.debounce = *debounce,
            WriteOp::Dpi(stages) => {
                snap.dpi = stages.clone();
                // `set_dpi_stages` resets the active stage to 1 (proto).
                if let Ok(stage) = DpiStage::new(1) {
                    snap.dpi_stage = stage;
                }
            }
            WriteOp::DpiStage(stage) => snap.dpi_stage = *stage,
            WriteOp::Brightness(_)
            | WriteOp::Scroll(_)
            | WriteOp::DpiColors(_)
            | WriteOp::Effect(_)
            | WriteOp::Bind(_, _) => {}
        }
    }
}

/// Sets a snapshot's write-only half (effect, brightness, DPI colours, binds,
/// scroll) from the remembered memory, dropping a remembered colour list whose
/// length no longer matches the snapshot's stage count, and taking the memory
/// error verbatim. The one place memory becomes the snapshot's write-only
/// values, shared by a read and a write's answer. `memory` is `None` when the
/// file was unreadable, so every write-only value reads `None`.
pub(crate) fn fill_write_only(
    snap: &mut Snapshot,
    memory: Option<&Memory>,
    memory_error: Option<String>,
) {
    let name = snap.model.name;
    let profile = snap.profile;
    snap.brightness = memory.and_then(|memory| memory.brightness(name));
    snap.effect = memory
        .and_then(|memory| memory.effect(name, profile))
        .cloned();
    snap.scroll = memory.and_then(|memory| memory.scroll(name));
    snap.dpi_colors = memory
        .and_then(|memory| memory.dpi_colors(name, profile))
        .and_then(|colors| {
            if colors.len() == snap.dpi.as_slice().len() {
                Some(colors.to_vec())
            } else {
                None
            }
        });
    snap.binds = BUTTONS
        .iter()
        .map(|button| {
            (
                *button,
                memory.and_then(|memory| memory.bind(name, profile, *button)),
            )
        })
        .collect();
    snap.memory_error = memory_error;
}

fn plan_performance(overlay: &PerformanceOverlay, snap: &Snapshot) -> Result<Vec<WriteOp>, String> {
    if overlay.invalid_polling {
        return Err("the polling rate did not parse; pick it again".to_owned());
    }
    if overlay.invalid_lift_off {
        return Err("the lift-off did not parse; pick it again".to_owned());
    }
    let mut ops = Vec::new();

    if let Some(rate) = overlay.polling_rate
        && rate != snap.polling_rate
    {
        ops.push(WriteOp::PollingRate(rate));
    }
    if let Some(lift_off) = overlay.lift_off
        && lift_off != snap.lift_off
    {
        ops.push(WriteOp::LiftOff(lift_off));
    }
    if let Some(sleep) = &overlay.sleep {
        let delay = sleep_delay(sleep)?;
        if delay != snap.sleep {
            ops.push(WriteOp::Sleep(delay));
        }
    }

    if let Some(dpi) = &overlay.dpi {
        let mut parsed = Vec::with_capacity(dpi.texts.len());
        for (index, text) in dpi.texts.iter().enumerate() {
            let stage =
                parse::dpi(text).map_err(|error| format!("stage {}: {}", index + 1, error.0))?;
            parsed.push(stage);
        }
        for (index, hex) in dpi.hex.iter().enumerate().take(dpi.texts.len()) {
            if let Some(text) = hex {
                parse::color(text)
                    .map_err(|error| format!("stage {} colour: {}", index + 1, error.0))?;
            }
        }
        let list_changed = parsed.as_slice() != snap.dpi.as_slice();
        let effective = dpi.effective_stage(snap);
        let stage = DpiStage::new(effective).map_err(|error| error.to_string())?;
        // The stage list, active stage and colours must agree before any write:
        // `set_dpi_stages` resets the active stage, and a colour list of another
        // length is refused on the wire, so an out-of-step draft would half-write.
        if let Some(colors) = &dpi.colors
            && colors.len() != parsed.len()
        {
            return Err(format!(
                "got {} stage colour(s) for {} stage(s)",
                colors.len(),
                parsed.len()
            ));
        }
        if list_changed {
            let stages = DpiStages::new(parsed).map_err(|error| error.to_string())?;
            ops.push(WriteOp::Dpi(stages));
            // The list write resets the active stage to 1, so the effective
            // stage follows it; an unchanged list writes one only for a pick.
            ops.push(WriteOp::DpiStage(stage));
        } else if dpi.active_stage.is_some() && stage != snap.dpi_stage {
            ops.push(WriteOp::DpiStage(stage));
        }
        if dpi.colors != snap.dpi_colors
            && let Some(colors) = &dpi.colors
        {
            ops.push(WriteOp::DpiColors(colors.clone()));
        }
    }
    if let Some(debounce) = overlay.debounce
        && debounce != snap.debounce.get()
    {
        ops.push(WriteOp::Debounce(
            Debounce::new(debounce).map_err(|error| error.to_string())?,
        ));
    }
    Ok(ops)
}

fn plan_lighting(overlay: &LightingOverlay, snap: &Snapshot) -> Result<Vec<WriteOp>, String> {
    let mut ops = Vec::new();
    // gctl never invents a link's value: with no remembered brightness, both
    // links must be set before Apply; a remembered pair edits one and keeps
    // the other.
    if let Some(brightness) = &overlay.brightness {
        let value = match (brightness.wired, brightness.wireless) {
            (Some(wired), Some(wireless)) => Some(Brightness { wired, wireless }),
            (Some(_), None) => return Err("set the wireless brightness too".to_owned()),
            (None, Some(_)) => return Err("set the wired brightness too".to_owned()),
            (None, None) => None,
        };
        if let Some(value) = value
            && Some(value) != snap.brightness
        {
            ops.push(WriteOp::Brightness(value));
        }
    }
    if let Some(effect) = &overlay.effect {
        for (index, hex) in effect.hex.iter().enumerate() {
            if let Some(text) = hex {
                parse::color(text).map_err(|error| format!("colour {}: {}", index + 1, error.0))?;
            }
        }
        if effect.effect != snap.effect
            && let Some(effect_value) = &effect.effect
        {
            effect_value.check().map_err(|error| error.to_string())?;
            ops.push(WriteOp::Effect(effect_value.clone()));
        }
    }
    Ok(ops)
}

fn plan_buttons(overlay: &ButtonsOverlay, snap: &Snapshot) -> Result<Vec<WriteOp>, String> {
    for (index, bind) in overlay.binds.iter().enumerate() {
        if let Some(bind) = bind
            && bind.invalid
        {
            return Err(format!(
                "{}'s binding does not parse; pick it again",
                button_label(BUTTONS[index])
            ));
        }
    }
    let mut ops = Vec::new();
    if let Some(direction) = overlay.scroll
        && Some(direction) != snap.scroll
    {
        ops.push(WriteOp::Scroll(direction));
    }
    for (index, bind) in overlay.binds.iter().enumerate() {
        if let Some(bind) = bind
            && bind.binding != snap.bind(BUTTONS[index])
            && let Some(binding) = bind.binding
        {
            ops.push(WriteOp::Bind(BUTTONS[index], binding));
        }
    }
    Ok(ops)
}

fn add_stage(dpi: &mut DpiOverlay) {
    if dpi.texts.len() >= DpiStages::MAX {
        return;
    }
    let last = dpi
        .texts
        .last()
        .cloned()
        .unwrap_or_else(|| "400".to_owned());
    dpi.texts.push(last);
    if let Some(colors) = dpi.colors.as_mut() {
        colors.push(WHITE);
    }
    dpi.hex_mut();
}

fn remove_stage(dpi: &mut DpiOverlay, index: usize) {
    if dpi.texts.len() <= 1 || index >= dpi.texts.len() {
        return;
    }
    dpi.texts.remove(index);
    if let Some(colors) = dpi.colors.as_mut()
        && index < colors.len()
    {
        colors.remove(index);
    }
    if index < dpi.hex.len() {
        dpi.hex.remove(index);
    }
    let len = dpi.texts.len() as u8;
    if let Some(stage) = &mut dpi.active_stage
        && *stage > len
    {
        *stage = len;
    }
}

fn set_stage_color(dpi: &mut DpiOverlay, index: usize, color: Rgb) {
    let len = dpi.texts.len();
    if index >= len {
        return;
    }
    match &mut dpi.colors {
        Some(colors) if colors.len() == len => colors[index] = color,
        _ => {
            let mut colors = vec![WHITE; len];
            colors[index] = color;
            dpi.colors = Some(colors);
        }
    }
}

/// The stage list as text values, one per stage.
fn stage_texts(dpi: &DpiStages) -> Vec<String> {
    dpi.as_slice()
        .iter()
        .map(|stage| stage.get().to_string())
        .collect()
}

/// The three sleep fields the window shows, from one delay.
fn sleep_parts(delay: SleepDelay) -> (bool, String, String) {
    match delay {
        SleepDelay::Never => (true, String::new(), String::new()),
        SleepDelay::Seconds(total) => (false, (total / 60).to_string(), (total % 60).to_string()),
    }
}

/// The delay an overlay's three sleep fields describe, with the GUI's messages.
fn sleep_delay(sleep: &SleepOverlay) -> Result<SleepDelay, String> {
    if sleep.never {
        return Ok(SleepDelay::Never);
    }
    let minutes = sleep_number(&sleep.minutes, "minutes")?;
    let seconds = sleep_number(&sleep.seconds, "seconds")?;
    let text = if seconds == 0 {
        format!("{minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m{seconds}s")
    } else {
        format!("{seconds}s")
    };
    match parse::sleep(&text) {
        Ok(delay) => Ok(delay),
        Err(_) if minutes == 0 && seconds == 0 => {
            Err("sleep delay is zero; tick never instead".to_owned())
        }
        Err(_) => Err("sleep delay exceeds 65534 seconds".to_owned()),
    }
}

fn sleep_number(text: &str, unit: &str) -> Result<u64, String> {
    if text.is_empty() {
        return Err(format!("sleep {unit} must be a whole number, got \"\""));
    }
    if !parse::is_whole_number(text) {
        return Err(format!("sleep {unit} must be a whole number, got {text:?}"));
    }
    text.parse::<u64>()
        .map_err(|_| format!("sleep {unit} is too long, got {text:?}"))
}

/// A slider's 0..=100 value as a `Rate`; the slider bounds make the fallback
/// unreachable, and it keeps the constructor total.
fn rate_of(value: u8) -> Rate {
    Rate::new(value.min(Rate::MAX)).unwrap_or(Rate::DEFAULT)
}

fn switch_effect(current: Option<&Effect>, kind: EffectKind) -> Effect {
    let (rate, colors) = effect_parts(current);
    build_effect(kind, rate, colors)
}

pub(crate) fn kind_of(effect: &Effect) -> EffectKind {
    match effect {
        Effect::Off => EffectKind::Off,
        Effect::Glorious { .. } => EffectKind::Glorious,
        Effect::Cycle { .. } => EffectKind::Cycle,
        Effect::Pulse { .. } => EffectKind::Pulse,
        Effect::Solid { .. } => EffectKind::Solid,
        Effect::PulseOne { .. } => EffectKind::PulseOne,
        Effect::Tail { .. } => EffectKind::Tail,
        Effect::Rave { .. } => EffectKind::Rave,
        Effect::Wave { .. } => EffectKind::Wave,
    }
}

fn effect_parts(effect: Option<&Effect>) -> (Rate, Vec<Rgb>) {
    match effect {
        None | Some(Effect::Off) => (Rate::DEFAULT, Vec::new()),
        Some(Effect::Glorious { rate })
        | Some(Effect::Cycle { rate })
        | Some(Effect::Tail { rate })
        | Some(Effect::Wave { rate }) => (*rate, Vec::new()),
        Some(Effect::Pulse { rate, colors }) | Some(Effect::Rave { rate, colors }) => {
            (*rate, colors.clone())
        }
        Some(Effect::Solid { color }) => (Rate::DEFAULT, vec![*color]),
        Some(Effect::PulseOne { rate, color }) => (*rate, vec![*color]),
    }
}

/// The colour list an effect shows, in slot order.
fn effect_colors_of(effect: Option<&Effect>) -> Vec<Rgb> {
    match effect {
        Some(effect) => effect_parts(Some(effect)).1,
        None => Vec::new(),
    }
}

pub(crate) fn build_effect(kind: EffectKind, rate: Rate, colors: Vec<Rgb>) -> Effect {
    match kind {
        EffectKind::Off => Effect::Off,
        EffectKind::Glorious => Effect::Glorious { rate },
        EffectKind::Cycle => Effect::Cycle { rate },
        EffectKind::Pulse => Effect::Pulse {
            rate,
            colors: fit_colors(colors, 2, 6),
        },
        EffectKind::Solid => Effect::Solid {
            color: colors.first().copied().unwrap_or(WHITE),
        },
        EffectKind::PulseOne => Effect::PulseOne {
            rate,
            color: colors.first().copied().unwrap_or(WHITE),
        },
        EffectKind::Tail => Effect::Tail { rate },
        EffectKind::Rave => Effect::Rave {
            rate,
            colors: fit_colors(colors, 1, 2),
        },
        EffectKind::Wave => Effect::Wave { rate },
    }
}

/// Clamps a colour list into the effect's bounds, padding with the last colour.
fn fit_colors(mut colors: Vec<Rgb>, min: usize, max: usize) -> Vec<Rgb> {
    if colors.len() > max {
        colors.truncate(max);
    }
    let last = colors.last().copied().unwrap_or(WHITE);
    while colors.len() < min {
        colors.push(last);
    }
    colors
}

fn set_effect_rate(effect: &mut Effect, rate: Rate) {
    match effect {
        Effect::Glorious { rate: r }
        | Effect::Cycle { rate: r }
        | Effect::Pulse { rate: r, .. }
        | Effect::PulseOne { rate: r, .. }
        | Effect::Tail { rate: r }
        | Effect::Rave { rate: r, .. }
        | Effect::Wave { rate: r } => *r = rate,
        Effect::Off | Effect::Solid { .. } => {}
    }
}

fn set_effect_color(effect: &mut Effect, index: usize, color: Rgb) {
    match effect {
        Effect::Pulse { colors, .. } | Effect::Rave { colors, .. } => {
            if index < colors.len() {
                colors[index] = color;
            }
        }
        Effect::Solid { color: c } | Effect::PulseOne { color: c, .. } => {
            if index == 0 {
                *c = color;
            }
        }
        Effect::Off
        | Effect::Glorious { .. }
        | Effect::Cycle { .. }
        | Effect::Tail { .. }
        | Effect::Wave { .. } => {}
    }
}

fn add_effect_color(effect: &mut Effect) {
    match effect {
        Effect::Pulse { colors, .. } if colors.len() < 6 => {
            let last = colors.last().copied().unwrap_or(WHITE);
            colors.push(last);
        }
        Effect::Rave { colors, .. } if colors.len() < 2 => {
            let last = colors.last().copied().unwrap_or(WHITE);
            colors.push(last);
        }
        Effect::Off
        | Effect::Glorious { .. }
        | Effect::Cycle { .. }
        | Effect::Solid { .. }
        | Effect::PulseOne { .. }
        | Effect::Tail { .. }
        | Effect::Wave { .. }
        | Effect::Pulse { .. }
        | Effect::Rave { .. } => {}
    }
}

fn remove_effect_color(effect: &mut Effect, index: usize) {
    match effect {
        Effect::Pulse { colors, .. } if colors.len() > 2 && index < colors.len() => {
            colors.remove(index);
        }
        Effect::Rave { colors, .. } if colors.len() > 1 && index < colors.len() => {
            colors.remove(index);
        }
        Effect::Off
        | Effect::Glorious { .. }
        | Effect::Cycle { .. }
        | Effect::Solid { .. }
        | Effect::PulseOne { .. }
        | Effect::Tail { .. }
        | Effect::Wave { .. }
        | Effect::Pulse { .. }
        | Effect::Rave { .. } => {}
    }
}

/// How many colour hex fields an effect shows.
fn effect_color_count(effect: Option<&Effect>) -> usize {
    match effect {
        Some(Effect::Pulse { colors, .. }) | Some(Effect::Rave { colors, .. }) => colors.len(),
        Some(Effect::Solid { .. }) | Some(Effect::PulseOne { .. }) => 1,
        Some(Effect::Off)
        | Some(Effect::Glorious { .. })
        | Some(Effect::Cycle { .. })
        | Some(Effect::Tail { .. })
        | Some(Effect::Wave { .. })
        | None => 0,
    }
}

/// The sentence-case display name of a physical button, shared with the window.
pub(crate) fn button_label(button: Button) -> &'static str {
    match button {
        Button::Left => "Left",
        Button::Right => "Right",
        Button::Scroll => "Wheel click",
        Button::Back => "Back",
        Button::Forward => "Forward",
        Button::ScrollUp => "Scroll up",
        Button::ScrollDown => "Scroll down",
        Button::Dpi => "DPI button",
    }
}

#[cfg(test)]
#[path = "../../tests/unit/gui_state.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/unit/gui_dpi_stage.rs"]
mod dpi_stage_tests;

#[cfg(test)]
#[path = "../../tests/unit/gui_unsure_fields.rs"]
mod unsure_field_tests;

#[cfg(test)]
#[path = "../../tests/unit/gui_import_none.rs"]
mod import_none_tests;
