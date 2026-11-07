#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The one simulator and the one fake mouse the window's tests draw through:
//! a change to the guard or the fixture reaches every module that uses them.

use std::time::Duration;

use iced::time::Instant;
use iced::{Element, Size};

use gctl_proto::{
    Battery, Debounce, DeviceState, Dpi, DpiStage, DpiStages, Firmware, LiftOff, MODELS,
    PollingRate, Profile, SleepDelay,
};

use super::state::{Msg, Section, Snapshot, State};
use super::view::{Message, Shell, window_settings};

/// Far past every duration the design uses (the longest, a slide, is 440 ms).
pub(crate) const SETTLED: Duration = Duration::from_secs(10);

/// Inside every animation a message starts: the shortest entrance is 120 ms.
pub(crate) const INSIDE: Duration = Duration::from_millis(40);

/// The guarded simulator at the window's shipped size.
pub(crate) fn simulator<'a>(
    view: Element<'a, Message>,
) -> iced_test::Simulator<'a, Message, iced::Theme, iced::Renderer> {
    simulator_sized(view, window_settings().size)
}

/// `simulator` at another window size.
pub(crate) fn simulator_sized<'a>(
    view: Element<'a, Message>,
    size: Size,
) -> iced_test::Simulator<'a, Message, iced::Theme, iced::Renderer> {
    assert!(
        std::env::var("ICED_TEST_BACKEND").as_deref() == Ok("tiny-skia"),
        "the view tests render through tiny-skia; set ICED_TEST_BACKEND=tiny-skia in .cargo/config.toml and run cargo from the repo root"
    );
    let settings = iced_test::core::Settings {
        fonts: crate::gui::theme::FONTS
            .iter()
            .map(|font| std::borrow::Cow::Borrowed(*font))
            .collect(),
        ..iced_test::core::Settings::default()
    };
    iced_test::Simulator::with_size(settings, size, view)
}

/// One rendered frame of `shell` in its own scheme.
pub(crate) fn frame(shell: &Shell) -> iced_test::simulator::Snapshot {
    let mut ui = simulator(shell.view());
    ui.snapshot(&shell.theme()).unwrap()
}

/// Whether two frames hold the same pixels: the first writes its hash to a
/// fresh temp dir, the second compares against it.
pub(crate) fn same(a: &iced_test::simulator::Snapshot, b: &iced_test::simulator::Snapshot) -> bool {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("frame");
    assert!(a.matches_hash(&path).unwrap(), "the first hash is written");
    b.matches_hash(&path).unwrap()
}

/// The click event sequence.
pub(crate) fn click() -> impl Iterator<Item = iced::Event> {
    [
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        )),
    ]
    .into_iter()
}

pub(crate) fn redraw(at: Instant) -> iced::Event {
    iced::Event::Window(iced::window::Event::RedrawRequested(at))
}

/// Hands a simulator a redraw at `at`, then one past every animation that
/// redraw started: whatever the cursor or a key changed is settled after it.
pub(crate) fn settle(
    ui: &mut iced_test::Simulator<'_, Message, iced::Theme, iced::Renderer>,
    at: Instant,
) {
    let _ = ui.simulate([redraw(at), redraw(at + SETTLED)]);
}

pub(crate) fn profile(id: u8) -> Profile {
    Profile::new(id).unwrap()
}

fn stages(values: &[u16]) -> DpiStages {
    DpiStages::new(values.iter().map(|v| Dpi::new(*v).unwrap()).collect()).unwrap()
}

/// The fake mouse of the unit tests, with nothing remembered.
pub(crate) fn snapshot() -> Snapshot {
    Snapshot {
        model: &MODELS[0],
        battery: Battery {
            state: DeviceState::Awake,
            percent: Some(35),
        },
        firmware: Firmware([0, 3, 8, 0]),
        active: profile(2),
        profile: profile(2),
        polling_rate: PollingRate::Hz1000,
        lift_off: LiftOff::Mm1,
        sleep: SleepDelay::Seconds(120),
        brightness: None,
        scroll: None,
        dpi: stages(&[400, 800, 1600]),
        dpi_stage: DpiStage::new(2).unwrap(),
        dpi_colors: None,
        debounce: Debounce::new(10).unwrap(),
        effect: None,
        binds: crate::get::BUTTONS
            .iter()
            .map(|button| (*button, None))
            .collect(),
        memory_error: None,
    }
}

/// A shell over `machine` with no device, no paths and the dark scheme.
pub(crate) fn shell_with_machine(machine: State) -> Shell {
    Shell::new(machine)
}

/// A shell whose machine has loaded `snap`.
pub(crate) fn shell(snap: Snapshot) -> Shell {
    let (mut machine, _) = State::new();
    machine.update(Msg::Loaded(Ok(snap)));
    shell_with_machine(machine)
}

/// A shell on a given section.
pub(crate) fn on_section(snap: Snapshot, section: Section) -> Shell {
    let mut machine = State::new().0;
    machine.update(Msg::Loaded(Ok(snap)));
    machine.update(Msg::Select(section));
    shell_with_machine(machine)
}
