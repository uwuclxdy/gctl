#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Logic-free render checks of what the window shows. Rendered with iced_test's
//! headless tiny-skia renderer (chosen by `.cargo/config.toml`), and asserted
//! by the exact text a text widget shows.
//! When `GCTL_RENDER_DIR` is set, each test also writes its frame there for the
//! caller to read by eye.

use std::path::Path;

use gctl_proto::{
    Binding, Button, DpiFn, Effect, KeyboardFn, MODELS, MediaFn, MouseFn, PollingRate, Rate,
    SleepDelay,
};

use super::*;
use crate::gui::state::{Edit, Msg, Notice, Section, Snapshot, State};
use crate::gui::theme::Scheme;

use crate::gui::testutil::{
    click, on_section, profile, shell, shell_with_machine, simulator, simulator_sized, snapshot,
};

#[path = "gui_layout.rs"]
mod layout;

#[path = "gui_motion.rs"]
mod motion;

#[path = "gui_live.rs"]
mod live;

const BUTTONS: [Button; 8] = [
    Button::Left,
    Button::Right,
    Button::Scroll,
    Button::Back,
    Button::Forward,
    Button::ScrollUp,
    Button::ScrollDown,
    Button::Dpi,
];

fn asleep_shell() -> Shell {
    let (mut machine, _) = State::new();
    machine.update(Msg::Loaded(Err(crate::gui::state::LoadError::Asleep)));
    shell_with_machine(machine)
}

fn no_mouse_shell() -> Shell {
    let (mut machine, _) = State::new();
    machine.update(Msg::Loaded(Err(crate::gui::state::LoadError::NoMouse)));
    shell_with_machine(machine)
}

fn contains(shell: &Shell, text: &str) -> bool {
    simulator(shell.view()).find(text).is_ok()
}

fn lacks(shell: &Shell, text: &str) -> bool {
    simulator(shell.view()).find(text).is_err()
}

/// Whether a widget with the given layout id is in the frame.
fn contains_id(shell: &Shell, id: &'static str) -> bool {
    simulator(shell.view())
        .find(iced::widget::Id::new(id))
        .is_ok()
}

/// How many text widgets show exactly `text`. Used to pin a mark to its own
/// row instead of "some row has it".
fn count(shell: &Shell, text: &str) -> usize {
    let mut ui = simulator(shell.view());
    let mut count = 0usize;
    let _ = ui.find(|candidate: iced_test::selector::Candidate<'_>| {
        if let iced_test::selector::Candidate::Text { content, .. } = candidate
            && content == text
        {
            count += 1;
        }
        None::<()>
    });
    count
}

fn rgb(hex: &str) -> gctl_proto::Rgb {
    hex.parse().unwrap()
}

fn render_frame(shell: &Shell, name: &str) {
    let Ok(dir) = std::env::var("GCTL_RENDER_DIR") else {
        return;
    };
    for (scheme, suffix) in [(Scheme::Dark, "dark"), (Scheme::Light, "light")] {
        let mut ui = simulator(shell.render(scheme));
        if let Ok(snapshot) = ui.snapshot(&scheme.theme()) {
            let _ = snapshot.matches_image(Path::new(&dir).join(format!("{name}-{suffix}")));
        }
    }
}

// --- the four sections from the fake snapshot ---------------------------------

#[test]
fn performance_marks_device_wide_rows_and_unknown_colours() {
    let shell = shell(snapshot());
    render_frame(&shell, "performance");
    assert_eq!(count(&shell, "all profiles"), 3);
    // each stage's tag names what is unknown: its colour
    assert_eq!(count(&shell, "Colour unknown"), 3);
    assert_eq!(count(&shell, "Unknown"), 0);
    assert!(contains(&shell, "per profile"));
    assert!(contains(&shell, "Add stage"));
    assert!(contains(&shell, "Stage 1"));
    assert!(contains(&shell, "Active stage"));
    assert!(contains(&shell, "Polling rate"));
}

#[test]
fn lighting_shows_unknown_when_nothing_is_remembered() {
    let shell = on_section(snapshot(), Section::Lighting);
    render_frame(&shell, "lighting");
    assert_eq!(count(&shell, "Unknown"), 3);
    assert!(lacks(&shell, "last set by gctl"));
}

#[test]
fn buttons_show_unknown_for_every_button() {
    let shell = on_section(snapshot(), Section::Buttons);
    render_frame(&shell, "buttons");
    assert!(lacks(&shell, "last set by gctl"));
    assert_eq!(count(&shell, "Unknown"), 9);
    assert!(contains(&shell, "Invert scroll"));
    assert!(contains(&shell, "Wheel click"));
    assert!(contains(&shell, "DPI button"));
}

#[test]
fn device_shows_model_link_firmware_and_battery() {
    let shell = on_section(snapshot(), Section::Device);
    render_frame(&shell, "device");
    assert!(contains(&shell, "Model O Wireless"));
    assert!(contains(&shell, "Wireless"));
    assert!(contains(&shell, "0.3.8.0"));
}

#[test]
fn device_pane_shows_the_export_result() {
    let mut shell = on_section(snapshot(), Section::Device);
    shell
        .machine
        .update(Msg::Export(std::path::PathBuf::from("/x/p.toml")));
    shell
        .machine
        .update(Msg::Exported(Err("\"/x/p.toml\" exists".to_owned())));
    render_frame(&shell, "device-export-failed");
    assert!(contains(&shell, "\"/x/p.toml\" exists"));
}

// --- remembered values ---------------------------------------------------------

#[test]
fn lighting_marks_a_remembered_effect_and_its_brightness() {
    let mut snap = snapshot();
    snap.effect = Some(Effect::Wave {
        rate: Rate::new(40).unwrap(),
    });
    snap.brightness = Some(gctl_proto::Brightness {
        wired: 200,
        wireless: 50,
    });
    let shell = on_section(snap, Section::Lighting);
    render_frame(&shell, "lighting-remembered");
    // effect marks its Effect row; brightness marks both of its rows
    assert_eq!(count(&shell, "last set by gctl"), 3);
}

#[test]
fn lighting_marks_a_remembered_effect_alone() {
    let mut snap = snapshot();
    snap.effect = Some(Effect::Wave {
        rate: Rate::new(40).unwrap(),
    });
    let shell = on_section(snap, Section::Lighting);
    assert_eq!(count(&shell, "last set by gctl"), 1);
}

#[test]
fn lighting_marks_a_remembered_brightness_alone() {
    let mut snap = snapshot();
    snap.brightness = Some(gctl_proto::Brightness {
        wired: 200,
        wireless: 50,
    });
    let shell = on_section(snap, Section::Lighting);
    assert_eq!(count(&shell, "last set by gctl"), 2);
}

#[test]
fn buttons_mark_every_remembered_bind() {
    let mut snap = snapshot();
    for (index, button) in BUTTONS.iter().enumerate() {
        snap.binds[index] = (*button, Some(Binding::Mouse(MouseFn::Forward)));
    }
    let shell = on_section(snap, Section::Buttons);
    render_frame(&shell, "buttons-all-remembered");
    assert_eq!(count(&shell, "last set by gctl"), 8);
}

#[test]
fn buttons_render_key_binds_on_two_lines() {
    let mut snap = snapshot();
    for (index, (button, text)) in BUTTONS
        .iter()
        .zip([
            "key:ctrl+KeyA",
            "key:ctrl+shift+KeyB",
            "key:meta-right+KeyC",
            "key:ctrl+shift+alt+meta+F12",
            "key:NumpadSubtract",
            "mouse:forward",
            "media:volume-up",
            "dpi:stage-up",
        ])
        .enumerate()
    {
        snap.binds[index] = (*button, Some(crate::parse::binding(text).unwrap()));
    }
    let shell = on_section(snap, Section::Buttons);
    render_frame(&shell, "buttons-key-binds");
    // The key bind rows carry their modifier labels on a second line, so the
    // widest (`meta-right`) stays on one line.
    assert!(contains(&shell, "meta-right"));
    assert!(contains(&shell, "shift"));
}

#[test]
fn lighting_renders_the_pulse_colour_rows() {
    let mut snap = snapshot();
    snap.effect = Some(Effect::Pulse {
        rate: Rate::new(40).unwrap(),
        colors: ["ff0000", "00ff00", "0000ff", "ffff00", "00ffff", "ff00ff"]
            .into_iter()
            .map(rgb)
            .collect(),
    });
    let shell = on_section(snap, Section::Lighting);
    render_frame(&shell, "lighting-pulse-colours");
}

#[test]
fn performance_marks_remembered_stage_colours() {
    let mut snap = snapshot();
    snap.dpi_colors = Some(vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")]);
    let shell = shell(snap);
    render_frame(&shell, "performance-remembered-colours");
    assert_eq!(count(&shell, "last set by gctl"), 1);
}

#[test]
fn every_offered_bind_token_round_trips() {
    for (family, names) in [
        ("mouse", &crate::parse::MOUSE_FN_NAMES[..]),
        ("media", &crate::parse::MEDIA_FN_NAMES[..]),
        ("dpi", &crate::parse::DPI_FN_NAMES[..]),
        ("keyboard", &crate::parse::KEYBOARD_FN_NAMES[..]),
    ] {
        for name in names {
            let text = format!("{family}:{name}");
            let binding = crate::parse::binding(&text).unwrap();
            assert_eq!(crate::parse::format_binding(binding), text, "{text}");
        }
    }
    for name in crate::parse::MODIFIER_NAMES {
        let text = format!("key:{name}+KeyA");
        let binding = crate::parse::binding(&text).unwrap();
        assert_eq!(crate::parse::format_binding(binding), text, "{text}");
    }
}

#[test]
fn memory_unreadable_shows_device_rows_and_the_banner() {
    // The CLI's own text, lowercase; the banner capitalizes at the draw site.
    let refusal = "cannot read remembered values from \"/home/c/.local/state/gctl/memory.toml\": unknown effect \"rainbow\"; fix or delete the file";
    let mut snap = snapshot();
    snap.memory_error = Some(refusal.to_owned());
    let shell = shell(snap);
    render_frame(&shell, "memory-unreadable");
    assert!(contains(&shell, "Model O Wireless"));
    assert!(contains(&shell, "10 ms"));
    assert!(contains(&shell, "400"));
    assert!(contains(
        &shell,
        "Cannot read remembered values from \"/home/c/.local/state/gctl/memory.toml\": unknown effect \"rainbow\"; fix or delete the file"
    ));
    assert!(contains(&shell, "Unknown"));
    assert!(lacks(&shell, "last set by gctl"));
}

// --- the empty states ----------------------------------------------------------

#[test]
fn asleep_start_shows_the_asleep_state_not_invented_values() {
    let shell = asleep_shell();
    render_frame(&shell, "asleep");
    assert!(contains(&shell, "The mouse is asleep. Move it to wake it."));
    assert!(lacks(&shell, "1000 Hz"));
    assert!(lacks(&shell, "1000"));
}

#[test]
fn no_mouse_shows_the_udev_hint() {
    let shell = no_mouse_shell();
    render_frame(&shell, "no-mouse");
    assert!(contains(&shell, "No mouse found"));
    assert!(contains(
        &shell,
        "Plug in a Glorious mouse, or install gctl's udev rule (70-gctl.rules) so your user can open its hidraw node."
    ));
}

// --- the modals ----------------------------------------------------------------

#[test]
fn discard_modal_appears_over_pending_edits() {
    let mut machine = State::new().0;
    machine.update(Msg::Loaded(Ok(snapshot())));
    machine.update(Msg::Edit(crate::gui::state::Edit::Debounce(0)));
    machine.update(Msg::Select(Section::Lighting));
    let shell = shell_with_machine(machine);
    render_frame(&shell, "discard-modal");
    // the prompt names the section being LEFT, which holds the edits
    assert!(contains(&shell, "Discard edits to Performance?"));
    assert!(contains(&shell, "Keep editing"));
    assert!(contains(&shell, "Discard"));
}

#[test]
fn import_modal_shows_the_note_and_the_device_wide_line() {
    let mut shell = shell(snapshot());
    let note = "note: \"/other.toml\" holds settings for \"Model D Wireless\"; writing them to Model O Wireless"
        .to_owned();
    shell
        .machine
        .update(Msg::Import(std::path::PathBuf::from("/other.toml")));
    shell.machine.update(Msg::ImportChecked(Ok(ImportCheck {
        note: Some(note.clone()),
        device_wide: true,
        writes: Vec::new(),
    })));
    render_frame(&shell, "import-modal");
    assert!(contains(&shell, "Import profile?"));
    assert!(contains(
        &shell,
        "Note: \"/other.toml\" holds settings for \"Model D Wireless\"; writing them to Model O Wireless"
    ));
    assert!(contains(
        &shell,
        "This overwrites device-wide settings (polling rate, lift-off, sleep, brightness, scroll)."
    ));
    assert!(contains(&shell, "Import"));
    assert!(contains(&shell, "Cancel"));
}

#[test]
fn import_cancel_closes_the_modal_and_frees_the_line() {
    let mut shell = shell(snapshot());
    shell
        .machine
        .update(Msg::Import(std::path::PathBuf::from("/other.toml")));
    shell.machine.update(Msg::ImportChecked(Ok(ImportCheck {
        note: Some(
            "note: \"/other.toml\" holds settings for \"Model D Wireless\"; writing them to Model O Wireless"
                .to_owned(),
        ),
        device_wide: true,
        writes: Vec::new(),
    })));
    assert!(contains(&shell, "Import profile?"));

    // Click the modal's Cancel button and feed the emitted message, so the
    // button's wiring is what the test drives.
    let mut ui = simulator(shell.view());
    ui.click("Cancel").unwrap();
    let messages: Vec<Message> = ui.into_messages().collect();
    assert!(!messages.is_empty(), "Cancel emits a message");
    for message in messages {
        let _ = shell.update(message);
    }

    assert!(lacks(&shell, "Import profile?"));
    // the line is free again: a tick asks to reload
    assert_eq!(
        shell.machine.update(Msg::Tick),
        vec![crate::gui::state::Request::Load(Some(profile(2)))]
    );
}

// --- the sidebar ----------------------------------------------------------------

/// A caps eyebrow as the view draws it: every character upper-cased, a hair
/// space (U+200A) between characters, since iced has no letter-spacing.
fn eyebrow(word: &str) -> String {
    word.chars()
        .flat_map(char::to_uppercase)
        .map(String::from)
        .collect::<Vec<_>>()
        .join("\u{200A}")
}

#[test]
fn eyebrow_spaces_every_character_with_a_hair_space() {
    assert_eq!(eyebrow("Dpi"), "D\u{200A}P\u{200A}I");
    assert_eq!(eyebrow("a b"), "A\u{200A} \u{200A}B");
}

/// The sidebar per gui.md's sketch: the link as an eyebrow on the battery
/// line, the battery as a bare %, `Profile` and `Settings` eyebrows; the old
/// `Edit` row and `Active profile:` line are gone.
#[test]
fn the_sidebar_shows_the_link_eyebrow_battery_and_group_eyebrows() {
    let shell = shell(snapshot());
    render_frame(&shell, "sidebar");
    for text in [
        eyebrow("Wireless"),
        "35%".to_owned(),
        eyebrow("Profile"),
        eyebrow("Settings"),
    ] {
        assert!(contains(&shell, &text), "{text:?}");
    }
    for text in ["Edit", "Active profile: 2", "Battery 35%", "Wireless"] {
        assert!(lacks(&shell, text), "{text:?}");
    }
}

/// `Make N active` names the edited profile and shows only while the edited
/// profile is not the active one.
#[test]
fn make_n_active_shows_only_while_the_edited_profile_is_not_active() {
    let same = shell(snapshot());
    for text in [
        "Make 1 active",
        "Make 2 active",
        "Make 3 active",
        "Make active",
    ] {
        assert!(lacks(&same, text), "edited == active: {text:?}");
    }
    let mut other = shell(snapshot());
    other.machine.update(Msg::PickProfile(profile(3)));
    other.machine.update(Msg::Loaded(Ok(Snapshot {
        profile: profile(3),
        ..snapshot()
    })));
    assert!(contains(&other, "Make 3 active"));
    assert!(lacks(&other, "Make active"));
}

#[test]
fn the_sidebar_marks_a_pending_section() {
    let mut shell = shell(snapshot());
    shell
        .machine
        .update(Msg::Edit(crate::gui::state::Edit::Debounce(0)));
    render_frame(&shell, "sidebar-pending");
    assert!(contains(&shell, "Performance •"));
}

#[test]
fn make_active_click_sends_the_request() {
    let mut shell = shell(snapshot());
    shell.machine.update(Msg::PickProfile(profile(3)));
    shell.machine.update(Msg::Loaded(Ok(Snapshot {
        profile: profile(3),
        ..snapshot()
    })));
    render_frame(&shell, "sidebar-make-active");
    assert!(contains(&shell, "Make 3 active"));
    let mut ui = simulator(shell.view());
    ui.click("Make 3 active").unwrap();
    let messages: Vec<Message> = ui.into_messages().collect();
    assert!(!messages.is_empty(), "Make active emits a message");
    let mut requests = Vec::new();
    for message in messages {
        if let Message::Machine(msg) = message {
            requests.extend(shell.machine.update(msg));
        }
    }
    assert_eq!(
        requests,
        vec![crate::gui::state::Request::MakeActive(profile(3))]
    );
}

#[test]
fn a_failed_bind_composition_blocks_apply() {
    let mut shell = on_section(snapshot(), Section::Buttons);
    shell
        .machine
        .update(Msg::Edit(crate::gui::state::Edit::InvalidBind(
            Button::Back,
        )));
    assert!(shell.machine.pending(Section::Buttons));
    assert!(shell.machine.apply_blocked().is_some());
    assert!(shell.machine.update(Msg::Apply).is_empty());
}

// --- the untested badge --------------------------------------------------------

#[test]
fn an_untested_model_wears_the_badge() {
    let mut snap = snapshot();
    snap.model = &MODELS[3]; // Model O- Wireless, untested
    let shell = shell(snap);
    render_frame(&shell, "untested");
    assert!(contains(&shell, "Untested"));
}

#[test]
fn the_longest_model_name_does_not_clip_the_badge() {
    let mut snap = snapshot();
    snap.model = &MODELS[11]; // "Unknown wired model (0x2031)", untested
    let shell = shell(snap);
    render_frame(&shell, "longest-model");
    assert!(contains(&shell, "Unknown wired model (0x2031)"));
    assert!(contains(&shell, "Untested"));
}

// --- unknown sliders, scroll marks and binds ------------------------------------

/// A click inside an unknown brightness slider picks a value and releases: the
/// release must keep the picked value, never clobber it to 0.
#[test]
fn releasing_an_unknown_slider_keeps_a_picked_value() {
    let shell = on_section(snapshot(), Section::Lighting);
    let mut ui = simulator(shell.view());
    let slider = ui
        .find(iced::widget::Id::new("wired-slider"))
        .expect("the wired slider is on the Lighting pane");
    let bounds = slider
        .visible_bounds()
        .expect("the wired slider is visible");
    let y = bounds.y + bounds.height / 2.0;
    ui.point_at(iced::Point::new(bounds.x + bounds.width / 2.0, y));
    ui.simulate(click());
    let messages: Vec<Message> = ui.into_messages().collect();

    let picked = messages
        .iter()
        .find_map(|message| match message {
            Message::Machine(Msg::Edit(crate::gui::state::Edit::BrightnessWired(value))) => {
                Some(*value)
            }
            _ => None,
        })
        .expect("the click picked a wired brightness value");
    assert!(picked > 0, "the click landed inside the slider, not at 0");

    let mut shell = shell;
    for message in messages {
        let _ = shell.update(message);
    }
    assert_eq!(shell.machine.brightness_wired(), Some(picked));
}

/// A click at the left edge of an unknown slider changes nothing, so only the
/// release speaks: the link becomes 0.
#[test]
fn a_click_at_the_left_edge_of_an_unknown_slider_sets_zero() {
    let shell = on_section(snapshot(), Section::Lighting);
    let mut ui = simulator(shell.view());
    let slider = ui
        .find(iced::widget::Id::new("wired-slider"))
        .expect("the wired slider is on the Lighting pane");
    let bounds = slider
        .visible_bounds()
        .expect("the wired slider is visible");
    let y = bounds.y + bounds.height / 2.0;
    ui.point_at(iced::Point::new(bounds.x + 0.5, y));
    ui.simulate(click());
    let messages: Vec<Message> = ui.into_messages().collect();

    let mut shell = shell;
    for message in messages {
        let _ = shell.update(message);
    }
    assert_eq!(shell.machine.brightness_wired(), Some(0));
}

/// Picking a scroll direction drops the row's `Unknown` mark: the dimming
/// and the mark follow the shown value, not the snapshot.
#[test]
fn a_picked_scroll_direction_drops_the_unknown_mark() {
    let mut shell = on_section(snapshot(), Section::Buttons);
    assert_eq!(count(&shell, "Unknown"), 9);
    shell
        .machine
        .update(Msg::Edit(crate::gui::state::Edit::Scroll(
            gctl_proto::ScrollDirection::Invert,
        )));
    assert_eq!(count(&shell, "Unknown"), 8);
    assert!(contains(&shell, "Invert scroll"));
}

/// A bind text that fails to parse becomes an invalid entry, not a silent
/// `Binding::None`.
#[test]
fn an_invalid_bind_text_is_an_invalid_entry() {
    assert_eq!(
        bind_edit(Button::Back, "media:nope"),
        crate::gui::state::Edit::InvalidBind(Button::Back)
    );
}

/// Every listed proto fn variant passes its exhaustive match arm's membership
/// assert, and a brand-new variant stops the match from compiling until an arm
/// is added. (A variant added everywhere except the ALL list cannot be caught
/// here: Rust cannot iterate the enum and proto exposes no fn decoder.)
#[test]
fn every_fn_variant_is_in_its_all_list() {
    fn mouse(function: MouseFn) {
        match function {
            MouseFn::Left => assert!(crate::parse::MOUSE_FN_ALL.contains(&MouseFn::Left)),
            MouseFn::Right => assert!(crate::parse::MOUSE_FN_ALL.contains(&MouseFn::Right)),
            MouseFn::Scroll => assert!(crate::parse::MOUSE_FN_ALL.contains(&MouseFn::Scroll)),
            MouseFn::Back => assert!(crate::parse::MOUSE_FN_ALL.contains(&MouseFn::Back)),
            MouseFn::Forward => assert!(crate::parse::MOUSE_FN_ALL.contains(&MouseFn::Forward)),
            MouseFn::ScrollUp => assert!(crate::parse::MOUSE_FN_ALL.contains(&MouseFn::ScrollUp)),
            MouseFn::ScrollDown => {
                assert!(crate::parse::MOUSE_FN_ALL.contains(&MouseFn::ScrollDown));
            }
            MouseFn::ProfileCycleUp => {
                assert!(crate::parse::MOUSE_FN_ALL.contains(&MouseFn::ProfileCycleUp));
            }
            MouseFn::ProfileCycleDown => {
                assert!(crate::parse::MOUSE_FN_ALL.contains(&MouseFn::ProfileCycleDown));
            }
            MouseFn::BatteryStatus => {
                assert!(crate::parse::MOUSE_FN_ALL.contains(&MouseFn::BatteryStatus));
            }
        }
    }
    fn media(function: MediaFn) {
        match function {
            MediaFn::Player => assert!(crate::parse::MEDIA_FN_ALL.contains(&MediaFn::Player)),
            MediaFn::PlayPause => assert!(crate::parse::MEDIA_FN_ALL.contains(&MediaFn::PlayPause)),
            MediaFn::Next => assert!(crate::parse::MEDIA_FN_ALL.contains(&MediaFn::Next)),
            MediaFn::Previous => assert!(crate::parse::MEDIA_FN_ALL.contains(&MediaFn::Previous)),
            MediaFn::Stop => assert!(crate::parse::MEDIA_FN_ALL.contains(&MediaFn::Stop)),
            MediaFn::Mute => assert!(crate::parse::MEDIA_FN_ALL.contains(&MediaFn::Mute)),
            MediaFn::VolumeUp => assert!(crate::parse::MEDIA_FN_ALL.contains(&MediaFn::VolumeUp)),
            MediaFn::VolumeDown => {
                assert!(crate::parse::MEDIA_FN_ALL.contains(&MediaFn::VolumeDown))
            }
        }
    }
    fn dpi(function: DpiFn) {
        match function {
            DpiFn::StageUp => assert!(crate::parse::DPI_FN_ALL.contains(&DpiFn::StageUp)),
            DpiFn::StageDown => assert!(crate::parse::DPI_FN_ALL.contains(&DpiFn::StageDown)),
            DpiFn::CycleUp => assert!(crate::parse::DPI_FN_ALL.contains(&DpiFn::CycleUp)),
            DpiFn::CycleDown => assert!(crate::parse::DPI_FN_ALL.contains(&DpiFn::CycleDown)),
        }
    }
    fn keyboard(function: KeyboardFn) {
        match function {
            KeyboardFn::ProfileCycleUp => {
                assert!(crate::parse::KEYBOARD_FN_ALL.contains(&KeyboardFn::ProfileCycleUp));
            }
            KeyboardFn::ProfileCycleDown => {
                assert!(crate::parse::KEYBOARD_FN_ALL.contains(&KeyboardFn::ProfileCycleDown));
            }
            KeyboardFn::LayerCycleUp => {
                assert!(crate::parse::KEYBOARD_FN_ALL.contains(&KeyboardFn::LayerCycleUp));
            }
            KeyboardFn::LayerCycleDown => {
                assert!(crate::parse::KEYBOARD_FN_ALL.contains(&KeyboardFn::LayerCycleDown));
            }
        }
    }

    for function in crate::parse::MOUSE_FN_ALL {
        mouse(function);
    }
    for function in crate::parse::MEDIA_FN_ALL {
        media(function);
    }
    for function in crate::parse::DPI_FN_ALL {
        dpi(function);
    }
    for function in crate::parse::KEYBOARD_FN_ALL {
        keyboard(function);
    }
}

/// Every offered polling-rate and lift-off label maps through the picker's own
/// on_select mapping to its edit (K9); a pick that silently fell back to a
/// wrong rate or an invalid entry would red here.
#[test]
fn every_polling_and_lift_off_label_round_trips() {
    for rate in POLLING_RATES {
        let label = polling_label(rate);
        assert_eq!(
            polling_pick(&label),
            crate::gui::state::Edit::PollingRate(rate),
            "{label}"
        );
    }
    for lift_off in LIFT_OFFS {
        let label = lift_off_label(lift_off);
        assert_eq!(
            lift_off_pick(&label),
            crate::gui::state::Edit::LiftOff(lift_off),
            "{label}"
        );
    }
}

/// A picker that lost its value blocks Apply with the reason, never a silent
/// fallback to some rate (J8).
#[test]
fn an_invalid_polling_or_lift_off_pick_blocks_apply() {
    let mut polling = State::new().0;
    polling.update(Msg::Loaded(Ok(snapshot())));
    polling.update(Msg::Edit(crate::gui::state::Edit::InvalidPollingRate));
    assert!(polling.pending(Section::Performance));
    assert_eq!(
        polling.apply_blocked().as_deref(),
        Some("the polling rate did not parse; pick it again")
    );
    assert!(polling.update(Msg::Apply).is_empty());

    let mut lift_off = State::new().0;
    lift_off.update(Msg::Loaded(Ok(snapshot())));
    lift_off.update(Msg::Edit(crate::gui::state::Edit::InvalidLiftOff));
    assert_eq!(
        lift_off.apply_blocked().as_deref(),
        Some("the lift-off did not parse; pick it again")
    );
    assert!(lift_off.update(Msg::Apply).is_empty());
}

/// Every family row pairs its token with its category, so `family_token`'s
/// discriminant index and the row order cannot drift (J13).
#[test]
fn bind_family_rows_pair_each_token_with_its_category() {
    for (category, token) in crate::parse::BIND_FAMILIES {
        assert_eq!(crate::parse::family_token(category), token, "{category:?}");
        assert_eq!(
            crate::parse::bind_category(token),
            Some(category),
            "{token}"
        );
    }
}

// --- pickers driven through iced_test ---------------------------

/// A dropdown row's height, the panel's gutter above its first and below its
/// last row, the tallest a panel draws (the design's select), and the gap
/// between the panel and its trigger.
const MENU_ROW: f32 = 36.0;
const MENU_GUTTER: f32 = 4.0;
const MENU_MAX: f32 = 240.0;
const MENU_GAP: f32 = 6.0;

/// Where an open dropdown's panel sits, per gui.md: `MENU_GAP` clear of its
/// trigger, below it whenever the room below holds the panel and that gap,
/// else on the side with more room; the panel is its rows plus the gutter, at
/// most `MENU_MAX` tall. Returns the panel's top edge and height in a window
/// `height` tall.
fn menu_panel(bounds: iced::Rectangle, option_count: usize, height: f32) -> (f32, f32) {
    let panel = (option_count as f32 * MENU_ROW + 2.0 * MENU_GUTTER).min(MENU_MAX);
    let room_below = height - (bounds.y + bounds.height);
    if room_below >= panel + MENU_GAP || room_below > bounds.y {
        (bounds.y + bounds.height + MENU_GAP, panel)
    } else {
        (bounds.y - MENU_GAP - panel, panel)
    }
}

/// The y of the centre of row `index` of an unscrolled open panel.
fn menu_row_y(bounds: iced::Rectangle, option_count: usize, index: usize, height: f32) -> f32 {
    let (top, _) = menu_panel(bounds, option_count, height);
    top + MENU_GUTTER + MENU_ROW * (index as f32 + 0.5)
}

fn picked_edits(
    shell: &Shell,
    picker_id: &'static str,
    option_index: usize,
    option_count: usize,
) -> Vec<Edit> {
    let mut ui = simulator(shell.view());
    let picker = ui
        .find(iced::widget::Id::new(picker_id))
        .expect("the picker is on the pane");
    let bounds = picker.visible_bounds().expect("the picker is visible");
    let center_x = bounds.x + bounds.width / 2.0;
    let center_y = bounds.y + bounds.height / 2.0;
    ui.point_at(iced::Point::new(center_x, center_y));
    ui.simulate(click());
    let option_position = iced::Point::new(
        center_x,
        menu_row_y(
            bounds,
            option_count,
            option_index,
            window_settings().size.height,
        ),
    );
    ui.point_at(option_position);
    ui.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
        position: option_position,
    })]);
    ui.simulate(click());
    ui.into_messages()
        .filter_map(|message| match message {
            Message::Machine(Msg::Edit(edit)) => Some(edit),
            _ => None,
        })
        .collect()
}

/// Every polling-rate option maps through the picker's own on_select closure to
/// its edit (K9/L6), opened and selected through iced_test.
#[test]
fn the_polling_picker_emits_each_options_edit() {
    let shell = shell(snapshot());
    for (index, rate) in POLLING_RATES.iter().copied().enumerate() {
        let edits = picked_edits(&shell, "polling-picker", index, POLLING_RATES.len());
        assert_eq!(edits, vec![Edit::PollingRate(rate)], "option {index}");
    }
}

/// Every lift-off option maps through the picker's own on_select closure to its
/// edit (K9/L6), opened and selected through iced_test.
#[test]
fn the_lift_off_picker_emits_each_options_edit() {
    let shell = shell(snapshot());
    for (index, lift_off) in LIFT_OFFS.iter().copied().enumerate() {
        let edits = picked_edits(&shell, "lift-off-picker", index, LIFT_OFFS.len());
        assert_eq!(edits, vec![Edit::LiftOff(lift_off)], "option {index}");
    }
}

// --- a pick empties the panes, the sidebar stays -----------------

#[test]
fn a_pick_waiting_on_its_read_shows_the_picked_profile_and_loading_panes() {
    let mut machine = State::new().0;
    machine.update(Msg::Loaded(Ok(snapshot())));
    machine.update(Msg::PickProfile(profile(3)));
    let shell = shell_with_machine(machine);
    render_frame(&shell, "pick-loading");
    assert_eq!(shell.machine.edit_profile(), Some(profile(3)));
    assert!(contains(&shell, "Loading"));
    assert!(contains(&shell, "Model O Wireless"));
    // profile 3 is the highlighted pick, profile 2 is not
    assert!(contains_id(&shell, "edited-profile"));
    assert!(contains_id(&shell, "profile-pick-2"));
    // Make 3 active is disabled with its reason
    assert!(contains(&shell, "Make 3 active"));
    assert!(contains(&shell, "Reading the mouse"));
}

#[test]
fn a_pick_answered_asleep_shows_the_asleep_state_in_the_panes() {
    let mut machine = State::new().0;
    machine.update(Msg::Loaded(Ok(snapshot())));
    machine.update(Msg::PickProfile(profile(3)));
    machine.update(Msg::Loaded(Err(crate::gui::state::LoadError::Asleep)));
    let shell = shell_with_machine(machine);
    render_frame(&shell, "pick-asleep");
    assert!(contains(&shell, "The mouse is asleep. Move it to wake it."));
    assert!(contains(&shell, "Model O Wireless"));
    assert!(contains_id(&shell, "edited-profile"));
    assert!(contains_id(&shell, "profile-pick-2"));
    // Make 3 active is disabled with the asleep reason
    assert!(contains(&shell, "Make 3 active"));
    assert!(contains(
        &shell,
        "The mouse is asleep; wake it to change the active profile"
    ));
}

#[test]
fn the_sleep_inputs_take_no_text_while_never_is_ticked() {
    let shell = shell(Snapshot {
        sleep: SleepDelay::Never,
        ..snapshot()
    });
    let mut ui = simulator(shell.view());
    ui.click(iced::widget::Id::new("sleep-minutes")).unwrap();
    ui.typewrite("5");
    ui.click(iced::widget::Id::new("sleep-seconds")).unwrap();
    ui.typewrite("9");
    let messages: Vec<Message> = ui.into_messages().collect();
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::Machine(Msg::Edit(Edit::SleepMinutes(_))))),
        "the minutes input is disabled while Never is ticked"
    );
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::Machine(Msg::Edit(Edit::SleepSeconds(_))))),
        "the seconds input is disabled while Never is ticked"
    );
}

/// The picker buttons read the start gate: while a write runs, a click on
/// another profile emits no `PickProfile` (the state refuses it, and the view
/// never offers it).
#[test]
fn a_pick_during_a_write_emits_no_message() {
    let mut machine = State::new().0;
    machine.update(Msg::Loaded(Ok(snapshot())));
    machine.update(Msg::Edit(Edit::Debounce(0)));
    assert_eq!(machine.update(Msg::Apply).len(), 1, "Apply starts a write");
    let shell = shell_with_machine(machine);
    assert!(contains_id(&shell, "profile-pick-3"));
    let mut ui = simulator(shell.view());
    ui.click(iced::widget::Id::new("profile-pick-3")).unwrap();
    let messages: Vec<Message> = ui.into_messages().collect();
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::Machine(Msg::PickProfile(_)))),
        "a pick during a write is refused: {messages:?}"
    );
}

/// The discard modal's footer names the open question, beside the scrim.
#[test]
fn a_discard_prompt_names_the_question_in_the_footer() {
    let mut machine = State::new().0;
    machine.update(Msg::Loaded(Ok(snapshot())));
    machine.update(Msg::Edit(Edit::Debounce(0)));
    machine.update(Msg::Select(Section::Lighting));
    let shell = shell_with_machine(machine);
    assert!(contains(&shell, "Answering a question"));
}

// --- capitalize-on-render and theme following -----------------------------------

/// The state keeps the CLI's lowercase text; the draw-site helper capitalizes
/// the first letter only.
#[test]
fn capitalize_capitalizes_the_first_letter_only() {
    assert_eq!(
        capitalize("exported to \"/home/c/p.toml\""),
        "Exported to \"/home/c/p.toml\""
    );
    assert_eq!(
        capitalize("the mouse is asleep; wake it to apply"),
        "The mouse is asleep; wake it to apply"
    );
}

/// A string that starts with a quote or a digit is drawn as it is.
#[test]
fn capitalize_leaves_quotes_and_digits_alone() {
    assert_eq!(capitalize("\"/x/p.toml\" exists"), "\"/x/p.toml\" exists");
    assert_eq!(capitalize("404 not found"), "404 not found");
}

/// A drawn notice starts with a capital while the state notice stays lowercase.
#[test]
fn a_drawn_notice_starts_with_a_capital() {
    let mut shell = on_section(snapshot(), Section::Device);
    shell
        .machine
        .update(Msg::Export(std::path::PathBuf::from("/home/c/p.toml")));
    shell
        .machine
        .update(Msg::Exported(Ok(std::path::PathBuf::from(
            "/home/c/p.toml",
        ))));
    assert_eq!(
        shell.machine.notice(),
        Some(&Notice::Done("exported to \"/home/c/p.toml\"".to_owned()))
    );
    assert!(contains(&shell, "Exported to \"/home/c/p.toml\""));
}

fn hex(color: iced::Color) -> String {
    let [r, g, b, _] = color.into_rgba8();
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// A light desktop mode drives the shell's scheme, which `run`'s theme hook
/// reads through `Shell::theme`.
#[test]
fn a_light_mode_switches_the_shells_theme() {
    let mut shell = shell(snapshot());
    assert_eq!(
        hex(iced::Theme::palette(&shell.theme()).background),
        hex(Scheme::Dark.tokens().bg)
    );
    let _ = shell.update(Message::Theme(iced::theme::Mode::Light));
    assert_eq!(
        hex(iced::Theme::palette(&shell.theme()).background),
        hex(Scheme::Light.tokens().bg)
    );
}

/// A banner joining two failures capitalizes every line, never just the first.
#[test]
fn a_banner_joining_two_failures_capitalizes_every_line() {
    let mut snap = snapshot();
    snap.memory_error = Some(
        "cannot read remembered values from \"/m.toml\": bad; fix or delete the file".to_owned(),
    );
    let mut shell = shell(snap);
    shell
        .machine
        .update(Msg::Loaded(Err(crate::gui::state::LoadError::Failed(
            "hid read failed".to_owned(),
        ))));
    assert!(contains(
        &shell,
        "Hid read failed\nCannot read remembered values from \"/m.toml\": bad; fix or delete the file"
    ));
}

/// The import prompt's model note is capitalized at the draw site.
#[test]
fn an_import_note_starts_with_a_capital() {
    let mut shell = shell(snapshot());
    let note = "note: \"/other.toml\" holds settings for \"Model D Wireless\"; writing them to Model O Wireless".to_owned();
    shell
        .machine
        .update(Msg::Import(std::path::PathBuf::from("/other.toml")));
    shell
        .machine
        .update(Msg::ImportChecked(Ok(crate::gui::state::ImportCheck {
            note: Some(note.clone()),
            device_wide: true,
            writes: Vec::new(),
        })));
    assert!(contains(
        &shell,
        "Note: \"/other.toml\" holds settings for \"Model D Wireless\"; writing them to Model O Wireless"
    ));
}

/// A two-line failed notice capitalizes every line, so the second failure is
/// never drawn lowercase.
#[test]
fn a_two_line_failed_notice_capitalizes_every_line() {
    let mut shell = on_section(snapshot(), Section::Device);
    shell
        .machine
        .update(Msg::Export(std::path::PathBuf::from("/x/p.toml")));
    shell.machine.update(Msg::Exported(Err(
        "write failed\ncannot save remembered values".to_owned(),
    )));
    assert!(contains(
        &shell,
        "Write failed\nCannot save remembered values"
    ));
}

/// The sidebar's battery with no reading is an `Unknown` tag on the link's
/// line, not prose and not a %.
#[test]
fn the_sidebar_battery_with_no_reading_is_an_unknown_tag() {
    let mut snap = snapshot();
    snap.battery.percent = None;
    let shell = shell(snap);
    render_frame(&shell, "sidebar-battery-unknown");
    assert!(contains(&shell, &eyebrow("Wireless")));
    assert_eq!(count(&shell, "Unknown"), 1);
    assert!(lacks(&shell, "Battery unknown"));
    assert!(lacks(&shell, "35%"));
}

/// The Device pane reads `file_blocked`, so an asleep mouse greys export and
/// import and names its own reason.
#[test]
fn a_sleep_device_pane_blocks_export_and_import() {
    let mut machine = State::new().0;
    machine.update(Msg::Loaded(Ok(snapshot())));
    machine.update(Msg::Select(Section::Device));
    machine.update(Msg::Loaded(Err(crate::gui::state::LoadError::Asleep)));
    let shell = shell_with_machine(machine);
    render_frame(&shell, "device-asleep");
    assert!(contains(
        &shell,
        "The mouse is asleep; wake it to export or import"
    ));
}

// --- other writers ----------------------------------------------------------

const OUTSIDE_BANNER: &str = "The mouse was changed outside this window. Values marked \"Last set by gctl\" may be out of date.";

/// A shell whose mouse moved a readable value no write of this window explains.
fn outside_change_shell() -> Shell {
    let mut machine = State::new().0;
    machine.update(Msg::Loaded(Ok(snapshot())));
    machine.update(Msg::Tick);
    machine.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz250,
        ..snapshot()
    })));
    shell_with_machine(machine)
}

/// The banner names its cause with the exact copy and offers Dismiss in both
/// themes; the click clears the state's flag through the shell's mapping.
#[test]
fn the_outside_change_banner_draws_in_both_themes_and_dismisses() {
    let mut shell = outside_change_shell();
    render_frame(&shell, "outside-banner");
    for scheme in [Scheme::Dark, Scheme::Light] {
        let mut ui = simulator(shell.render(scheme));
        assert!(ui.find(OUTSIDE_BANNER).is_ok(), "banner text: {scheme:?}");
        assert!(ui.find("Dismiss").is_ok(), "dismiss button: {scheme:?}");
    }

    let mut ui = simulator(shell.render(Scheme::Dark));
    ui.click("Dismiss").unwrap();
    let messages: Vec<Message> = ui.into_messages().collect();
    assert!(!messages.is_empty(), "Dismiss emits a message");
    for message in messages {
        let _ = shell.update(message);
    }
    assert!(!shell.machine.outside_change());
    assert!(lacks(&shell, OUTSIDE_BANNER));
}

/// The moved lines draw in their own footer slot under the notice, one
/// capitalized line per field, in both themes.
#[test]
fn the_moved_lines_draw_one_line_per_field_in_both_themes() {
    let mut machine = State::new().0;
    machine.update(Msg::Loaded(Ok(snapshot())));
    machine.update(Msg::Edit(Edit::PollingRate(PollingRate::Hz500)));
    machine.update(Msg::Edit(Edit::Debounce(4)));
    machine.update(Msg::Tick);
    machine.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz250,
        debounce: gctl_proto::Debounce::new(7).unwrap(),
        ..snapshot()
    })));
    let shell = shell_with_machine(machine);
    assert_eq!(
        shell.machine.changed(),
        vec![
            "polling rate changed on the mouse to 250 Hz; your edit is kept".to_owned(),
            "debounce changed on the mouse to 7 ms; your edit is kept".to_owned(),
        ]
    );
    render_frame(&shell, "outside-notice");
    let drawn = "Polling rate changed on the mouse to 250 Hz; your edit is kept\nDebounce changed on the mouse to 7 ms; your edit is kept";
    for scheme in [Scheme::Dark, Scheme::Light] {
        assert!(
            simulator(shell.render(scheme)).find(drawn).is_ok(),
            "moved lines: {scheme:?}"
        );
    }
}

/// The Device pane draws a live moved line in its profile card: the mouse
/// moved a field to the value the entry held (so the entry is not pending and
/// selecting Device asks nothing), then moved it again.
#[test]
fn the_device_panes_profile_card_draws_a_live_moved_line() {
    let mut machine = State::new().0;
    machine.update(Msg::Loaded(Ok(snapshot())));
    machine.update(Msg::Edit(Edit::PollingRate(PollingRate::Hz500)));
    machine.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz500,
        ..snapshot()
    })));
    assert!(
        !machine.pending(Section::Performance),
        "the mouse moved to the entry's value, so nothing is pending"
    );
    machine.update(Msg::Select(Section::Device));
    assert!(
        machine.prompt().is_none(),
        "selecting Device asks nothing with nothing pending"
    );
    machine.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz250,
        ..snapshot()
    })));
    let shell = shell_with_machine(machine);
    assert_eq!(
        shell.machine.changed(),
        vec!["polling rate changed on the mouse to 250 Hz; your edit is kept".to_owned()]
    );
    render_frame(&shell, "device-moved-line");
    let line = "Polling rate changed on the mouse to 250 Hz; your edit is kept";
    for scheme in [Scheme::Dark, Scheme::Light] {
        assert!(
            simulator(shell.render(scheme)).find(line).is_ok(),
            "device moved line: {scheme:?}"
        );
    }
}
