#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The GUI's section state machine, driven by messages with no device and no
//! window: pending edits per section, Apply/Revert, the discard prompt, the
//! edit vs active profile, and the write plan each Apply sends.

use gctl_proto::{
    Battery, Binding, Brightness, Button, Debounce, DeviceState, Dpi, DpiStage, DpiStages, Effect,
    Firmware, LiftOff, MODELS, MediaFn, Model, MouseFn, PollingRate, Profile, Rate, Rgb,
    ScrollDirection, SleepDelay,
};

use std::path::PathBuf;

use super::*;
use crate::memory::Memory;
use crate::writes::WriteOp;

fn profile(id: u8) -> Profile {
    Profile::new(id).unwrap()
}

fn stages(values: &[u16]) -> DpiStages {
    DpiStages::new(values.iter().map(|v| Dpi::new(*v).unwrap()).collect()).unwrap()
}

fn rgb(hex: &str) -> Rgb {
    hex.parse().unwrap()
}

fn rate(value: u8) -> Rate {
    Rate::new(value).unwrap()
}

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

/// The fake mouse of `tests/cli.rs` as a snapshot: active profile 2, editing
/// profile 2, stages 400 800 1600 on stage 2, nothing remembered.
fn snapshot() -> Snapshot {
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
        binds: BUTTONS.iter().map(|button| (*button, None)).collect(),
        memory_error: None,
    }
}

/// A state that has loaded `snap`, on the Performance section.
fn ready(snap: Snapshot) -> State {
    let (mut state, requests) = State::new();
    assert_eq!(requests, vec![Request::Load(None)]);
    let requests = state.update(Msg::Loaded(Ok(snap)));
    assert!(requests.is_empty());
    state
}

fn edit(state: &mut State, edit: Edit) -> Vec<Request> {
    state.update(Msg::Edit(edit))
}

// --- loading and connection -------------------------------------------------

#[test]
fn a_new_state_loads_the_active_profile_and_shows_nothing_yet() {
    let (state, requests) = State::new();
    assert_eq!(requests, vec![Request::Load(None)]);
    assert_eq!(state.connection(), Connection::Loading);
    assert!(state.snapshot().is_none());
    assert_eq!(state.section(), Section::Performance);
}

#[test]
fn a_loaded_snapshot_is_ready_with_no_pending_edits() {
    let state = ready(snapshot());
    assert_eq!(state.connection(), Connection::Ready);
    assert_eq!(state.snapshot(), Some(&snapshot()));
    assert_eq!(state.edit_profile(), Some(profile(2)));
    for section in [
        Section::Performance,
        Section::Lighting,
        Section::Buttons,
        Section::Device,
    ] {
        assert!(!state.pending(section), "{section:?}");
        assert_eq!(state.plan(section), Ok(Vec::new()), "{section:?}");
    }
    assert_eq!(state.apply_blocked(), None);
}

#[test]
fn no_mouse_shows_the_empty_state_and_every_tick_retries_the_open() {
    let (mut state, _) = State::new();
    state.update(Msg::Loaded(Err(LoadError::NoMouse)));
    assert_eq!(state.connection(), Connection::NoMouse);
    assert!(state.snapshot().is_none());
    assert_eq!(state.update(Msg::Tick), vec![Request::Load(None)]);
    state.update(Msg::Loaded(Err(LoadError::NoMouse)));
    assert_eq!(state.update(Msg::Tick), vec![Request::Load(None)]);
}

#[test]
fn nothing_else_starts_while_a_load_is_in_flight() {
    let mut state = ready(snapshot());
    assert_eq!(
        state.update(Msg::Tick),
        vec![Request::Load(Some(profile(2)))]
    );
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.apply_blocked().as_deref(), Some("reading the mouse"));
    assert!(state.update(Msg::Apply).is_empty());
    assert!(state.update(Msg::Tick).is_empty());
    assert!(state.update(Msg::MakeActive).is_empty());
    assert!(
        state
            .update(Msg::Export(PathBuf::from("/x/p.toml")))
            .is_empty()
    );
    // the answer frees the line, and the edit is still there to apply
    state.update(Msg::Loaded(Ok(snapshot())));
    assert_eq!(state.apply_blocked(), None);
    assert_eq!(
        state.update(Msg::Apply),
        vec![Request::Apply {
            profile: profile(2),
            ops: vec![WriteOp::Debounce(Debounce::new(4).unwrap())],
        }]
    );
}

// --- export -------------------------------------------------------------------

#[test]
fn export_requests_the_edited_profile_and_holds_the_line_until_it_answers() {
    let mut state = ready(snapshot());
    state.update(Msg::PickProfile(profile(3)));
    let mut three = snapshot();
    three.profile = profile(3);
    state.update(Msg::Loaded(Ok(three)));
    let path = PathBuf::from("/home/c/p.toml");
    assert_eq!(
        state.update(Msg::Export(path.clone())),
        vec![Request::Export {
            profile: profile(3),
            path: path.clone(),
        }]
    );
    assert!(state.update(Msg::Tick).is_empty());
    state.update(Msg::Exported(Ok(path)));
    assert_eq!(
        state.notice(),
        Some(&Notice::Done("exported to \"/home/c/p.toml\"".to_owned()))
    );
    assert_eq!(
        state.update(Msg::Tick),
        vec![Request::Load(Some(profile(3)))]
    );
}

#[test]
fn a_failed_export_is_an_error_notice() {
    let mut state = ready(snapshot());
    state.update(Msg::Export(PathBuf::from("/home/c/p.toml")));
    state.update(Msg::Exported(Err("\"/home/c/p.toml\" exists".to_owned())));
    assert_eq!(
        state.notice(),
        Some(&Notice::Failed("\"/home/c/p.toml\" exists".to_owned()))
    );
}

// --- import -------------------------------------------------------------------

fn import_path() -> PathBuf {
    PathBuf::from("/home/c/p.toml")
}

#[test]
fn an_import_checks_the_file_before_anything_touches_the_mouse() {
    let mut state = ready(snapshot());
    assert_eq!(
        state.update(Msg::Import(import_path())),
        vec![Request::PrepareImport {
            path: import_path()
        }]
    );
    // the check holds the line
    assert!(state.update(Msg::Tick).is_empty());
}

#[test]
fn a_plain_import_of_the_same_model_runs_without_asking() {
    let mut state = ready(snapshot());
    state.update(Msg::Import(import_path()));
    assert_eq!(
        state.update(Msg::ImportChecked(Ok(ImportCheck {
            note: None,
            device_wide: false,
            writes: Vec::new(),
        }))),
        vec![Request::Import {
            profile: profile(2),
            path: import_path(),
            check: ImportCheck {
                note: None,
                device_wide: false,
                writes: Vec::new(),
            },
        }]
    );
    assert_eq!(state.prompt(), None);
}

#[test]
fn an_import_with_device_settings_asks_and_cancel_frees_the_line() {
    let mut state = ready(snapshot());
    state.update(Msg::Import(import_path()));
    let check = ImportCheck {
        note: Some(
            "note: \"/home/c/p.toml\" holds settings for \"Model D Wireless\"; writing them to Model O Wireless"
                .to_owned(),
        ),
        device_wide: true,
        writes: Vec::new(),
    };
    assert!(
        state
            .update(Msg::ImportChecked(Ok(check.clone())))
            .is_empty()
    );
    assert_eq!(state.prompt(), Some(Prompt::Import(check)));
    // the open question holds the line
    assert!(state.update(Msg::Tick).is_empty());

    assert!(state.update(Msg::ImportCancel).is_empty());
    assert_eq!(state.prompt(), None);
    // a confirm after the cancel runs nothing
    assert!(state.update(Msg::ImportConfirm).is_empty());
    assert_eq!(
        state.update(Msg::Tick),
        vec![Request::Load(Some(profile(2)))]
    );
}

#[test]
fn confirming_an_import_runs_it_on_the_edited_profile() {
    let mut state = ready(snapshot());
    state.update(Msg::Import(import_path()));
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: true,
        writes: Vec::new(),
    })));
    assert_eq!(
        state.update(Msg::ImportConfirm),
        vec![Request::Import {
            profile: profile(2),
            path: import_path(),
            // the run re-checks the file against what the user confirmed
            check: ImportCheck {
                note: None,
                device_wide: true,
                writes: Vec::new(),
            },
        }]
    );
    assert_eq!(state.prompt(), None);
    assert!(state.update(Msg::Tick).is_empty());
    state.update(Msg::Imported {
        error: None,
        reached: false,
        written: Vec::new(),
        remembered: None,
        snapshot: Some(Ok(snapshot())),
    });
    assert_eq!(
        state.notice(),
        Some(&Notice::Done("imported into profile 2".to_owned()))
    );
    assert_eq!(
        state.update(Msg::Tick),
        vec![Request::Load(Some(profile(2)))]
    );
}

#[test]
fn a_refused_import_file_is_a_notice_and_touches_nothing_else() {
    let mut state = ready(snapshot());
    state.update(Msg::PickProfile(profile(3)));
    let mut three = snapshot();
    three.profile = profile(3);
    state.update(Msg::Loaded(Ok(three)));
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::Import(import_path()));
    let refusal = "\"/home/c/p.toml\": profile.dpi_stage: DPI stage 9 is out of range 1..=6";
    assert!(
        state
            .update(Msg::ImportChecked(Err(refusal.to_owned())))
            .is_empty()
    );
    assert_eq!(state.notice(), Some(&Notice::Failed(refusal.to_owned())));
    // a file error is no device error, and no reload moves the window
    assert_eq!(state.error(), None);
    assert_eq!(state.edit_profile(), Some(profile(3)));
    assert!(state.pending(Section::Performance));
}

// --- notices --------------------------------------------------------------------

#[test]
fn a_notice_stays_through_ticks_and_clears_on_the_next_user_action() {
    let mut state = ready(snapshot());
    state.update(Msg::Export(PathBuf::from("/home/c/p.toml")));
    state.update(Msg::Exported(Err("\"/home/c/p.toml\" exists".to_owned())));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(snapshot())));
    assert!(state.notice().is_some());
    state.update(Msg::Select(Section::Lighting));
    assert_eq!(state.notice(), None);
}

// --- discard while a read runs -----------------------------------------------

#[test]
fn discarding_to_switch_sections_works_while_a_read_runs() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::Tick);
    state.update(Msg::Select(Section::Buttons));
    assert!(state.update(Msg::Discard).is_empty());
    assert_eq!(state.section(), Section::Buttons);
    assert!(!state.pending(Section::Performance));
}

// --- a reload keeps pending edits and nothing else ----------------------------

#[test]
fn a_pending_section_takes_the_reloaded_values_of_fields_nobody_touched() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    let mut fresh = snapshot();
    fresh.debounce = Debounce::new(4).unwrap();
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(fresh)));
    // Apply must not write the stale debounce 10 back over the device's 4
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::PollingRate(PollingRate::Hz500)])
    );
}

#[test]
fn pending_edits_survive_a_moment_with_no_mouse() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Err(LoadError::NoMouse)));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(snapshot())));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::Debounce(Debounce::new(4).unwrap())])
    );
}

#[test]
fn pending_edits_survive_switching_between_cable_and_dongle() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    let mut wired = snapshot();
    wired.model = Model::from_pid(0x2011).unwrap();
    assert_eq!(wired.model.name, "Model O Wireless");
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(wired)));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::Debounce(Debounce::new(4).unwrap())])
    );
}

// --- stage colour texts follow their stages ---------------------------------

#[test]
fn removing_a_stage_takes_its_colour_text_with_it() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::StageColorHex(1, "123456".to_owned()));
    edit(&mut state, Edit::StageColorHex(2, "ff".to_owned()));
    edit(&mut state, Edit::RemoveStage(2));
    // the half-typed colour left with stage 3
    assert_eq!(state.stage_color_hex(1), "123456");
    edit(&mut state, Edit::RemoveStage(0));
    assert_eq!(state.stage_color_hex(0), "123456");
    assert!(state.plan(Section::Performance).is_ok());
}

#[test]
fn adding_a_stage_keeps_the_other_stages_typed_colours() {
    let mut snap = snapshot();
    snap.dpi_colors = Some(vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")]);
    let mut state = ready(snap);
    edit(&mut state, Edit::StageColorHex(0, "12".to_owned()));
    edit(&mut state, Edit::AddStage);
    edit(&mut state, Edit::StageColorHex(3, "abcdef".to_owned()));
    assert_eq!(state.stage_color_hex(0), "12");
    assert_eq!(state.stage_color_hex(1), "00ff00");
}

// --- a new snapshot replaces untouched drafts --------------------------------

#[test]
fn picking_another_profile_after_an_apply_shows_that_profile() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::Apply);
    let mut applied = snapshot();
    applied.debounce = Debounce::new(4).unwrap();
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(applied),
    });
    assert_eq!(
        state.update(Msg::PickProfile(profile(3))),
        vec![Request::Load(Some(profile(3)))]
    );
    let mut three = snapshot();
    three.profile = profile(3);
    three.dpi = stages(&[800, 1600]);
    three.dpi_stage = DpiStage::new(1).unwrap();
    state.update(Msg::Loaded(Ok(three)));
    assert!(!state.pending(Section::Performance));
    assert_eq!(state.plan(Section::Performance), Ok(Vec::new()));
}

#[test]
fn a_touched_but_unchanged_section_follows_a_reload() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    edit(&mut state, Edit::PollingRate(PollingRate::Hz1000));
    let mut fresh = snapshot();
    fresh.polling_rate = PollingRate::Hz250;
    state.update(Msg::Loaded(Ok(fresh)));
    assert!(!state.pending(Section::Performance));
    // a later edit diffs against the device's new value, never the stale draft
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::Debounce(Debounce::new(4).unwrap())])
    );
}

#[test]
fn an_asleep_mouse_keeps_the_last_snapshot_and_blocks_apply_with_the_reason() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    state.update(Msg::Loaded(Err(LoadError::Asleep)));
    assert_eq!(state.connection(), Connection::Asleep);
    assert_eq!(state.snapshot(), Some(&snapshot()));
    assert_eq!(
        state.apply_blocked().as_deref(),
        Some("the mouse is asleep; wake it to apply")
    );
    assert!(state.update(Msg::Apply).is_empty());
    // the edit survives the nap, and the next good load unblocks it
    assert!(state.pending(Section::Performance));
    state.update(Msg::Loaded(Ok(snapshot())));
    assert_eq!(state.connection(), Connection::Ready);
    assert_eq!(state.apply_blocked(), None);
    assert!(state.pending(Section::Performance));
}

#[test]
fn an_asleep_mouse_at_startup_shows_the_asleep_state_not_loading() {
    let (mut state, _) = State::new();
    state.update(Msg::Loaded(Err(LoadError::Asleep)));
    assert_eq!(state.connection(), Connection::Asleep);
    assert!(state.snapshot().is_none());
    assert_eq!(
        state.apply_blocked().as_deref(),
        Some("the mouse is asleep; wake it to apply")
    );
    assert_eq!(state.update(Msg::Tick), vec![Request::Load(None)]);
}

#[test]
fn a_failed_first_load_shows_the_error_not_loading() {
    let (mut state, _) = State::new();
    state.update(Msg::Loaded(Err(LoadError::Transport(
        "HID transport failed: gone".to_owned(),
    ))));
    assert_ne!(state.connection(), Connection::Loading);
    assert_eq!(state.error(), Some("HID transport failed: gone"));
    assert_eq!(state.update(Msg::Tick), vec![Request::Load(None)]);
}

#[test]
fn a_failed_load_shows_its_message_and_the_next_tick_reopens() {
    let mut state = ready(snapshot());
    state.update(Msg::Loaded(Err(LoadError::Transport(
        "HID transport failed: gone".to_owned(),
    ))));
    assert_eq!(state.error(), Some("HID transport failed: gone"));
    assert!(state.reopen());
    assert_eq!(
        state.update(Msg::Tick),
        vec![Request::Load(Some(profile(2)))]
    );
    state.update(Msg::Loaded(Ok(snapshot())));
    assert_eq!(state.error(), None);
    assert!(!state.reopen());
}

// --- re-opening the device ----------------------------------------------------

#[test]
fn a_new_state_opens_the_device_before_its_first_read() {
    let (state, _) = State::new();
    assert!(state.reopen());
}

#[test]
fn only_no_mouse_or_a_transport_error_drops_the_open_handle() {
    let cases = [
        (LoadError::NoMouse, true),
        (
            LoadError::Transport("HID transport failed: gone".to_owned()),
            true,
        ),
        (LoadError::Asleep, false),
        (
            LoadError::Failed(
                "cannot find a place for the lock file; set XDG_RUNTIME_DIR or HOME".to_owned(),
            ),
            false,
        ),
        (
            LoadError::Failed("device reported an implausible polling rate (9)".to_owned()),
            false,
        ),
    ];
    for (error, reopen) in cases {
        let mut state = ready(snapshot());
        assert!(!state.reopen(), "a good read keeps the handle");
        state.update(Msg::Tick);
        state.update(Msg::Loaded(Err(error.clone())));
        assert_eq!(state.reopen(), reopen, "{error:?}");
    }
}

#[test]
fn a_transport_error_shows_its_message_like_any_failed_read() {
    let mut state = ready(snapshot());
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Err(LoadError::Transport(
        "HID transport failed: gone".to_owned(),
    ))));
    assert_eq!(state.connection(), Connection::Failed);
    assert_eq!(state.error(), Some("HID transport failed: gone"));
}

#[test]
fn a_write_whose_reread_hits_a_dead_handle_reopens_the_device() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: None,
        snapshot: Err(LoadError::Transport(
            "HID transport failed: gone".to_owned(),
        )),
    });
    assert!(state.reopen());

    // the next good read takes the new handle into use
    assert_eq!(
        state.update(Msg::Tick),
        vec![Request::Load(Some(profile(2)))]
    );
    state.update(Msg::Loaded(Ok(snapshot())));
    assert!(!state.reopen());
}

#[test]
fn an_import_whose_reread_hits_a_dead_handle_reopens_the_device() {
    let mut state = ready(snapshot());
    state.update(Msg::Import(PathBuf::from("/home/c/p.toml")));
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: Vec::new(),
    })));
    state.update(Msg::Imported {
        error: Some("HID transport failed: gone".to_owned()),
        reached: true,
        written: Vec::new(),
        remembered: None,
        snapshot: Some(Err(LoadError::Transport(
            "HID transport failed: gone".to_owned(),
        ))),
    });
    assert!(state.reopen());
}

#[test]
fn a_write_that_fails_on_a_live_handle_keeps_it() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    state.update(Msg::Apply);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "device is asleep".to_owned(),
            landed: Vec::new(),
            reached: false,
        }),
        remembered: None,
        snapshot: Err(LoadError::Asleep),
    });
    assert!(!state.reopen());
}

// --- an unreadable memory file -------------------------------------------------

const MEMORY_REFUSAL: &str = "cannot read remembered values from \"/home/c/.local/state/gctl/memory.toml\": unknown effect \"rainbow\"; fix or delete the file";

/// The fake mouse's snapshot as read with an unreadable memory file: every
/// device value, no write-only value, and the CLI's refusal.
fn memory_unreadable() -> Snapshot {
    Snapshot {
        memory_error: Some(MEMORY_REFUSAL.to_owned()),
        ..snapshot()
    }
}

#[test]
fn an_unreadable_memory_file_shows_the_device_values_and_names_the_file() {
    let state = ready(memory_unreadable());
    assert_eq!(state.connection(), Connection::Ready);
    assert_eq!(state.error(), Some(MEMORY_REFUSAL));
    assert_eq!(state.polling_rate(), Some(PollingRate::Hz1000));
    assert_eq!(
        state.stage_texts(),
        Some(vec!["400".to_owned(), "800".to_owned(), "1600".to_owned()])
    );
    assert_eq!(state.effect_kind(), None);
    assert_eq!(state.bind(Button::Back), None);
}

#[test]
fn with_unreadable_memory_apply_of_a_device_value_still_runs() {
    let mut state = ready(memory_unreadable());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    edit(&mut state, Edit::ActiveStage(3));
    assert_eq!(state.apply_blocked(), None);
    assert_eq!(
        state.update(Msg::Apply),
        vec![Request::Apply {
            profile: profile(2),
            ops: vec![
                WriteOp::PollingRate(PollingRate::Hz500),
                WriteOp::DpiStage(DpiStage::new(3).unwrap()),
            ],
        }]
    );
}

#[test]
fn with_unreadable_memory_apply_of_a_remembered_setting_is_refused_with_the_cli_text() {
    let mut state = ready(memory_unreadable());
    state.update(Msg::Select(Section::Lighting));
    edit(&mut state, Edit::Effect(EffectKind::Off));
    assert!(state.pending(Section::Lighting));
    assert_eq!(state.apply_blocked().as_deref(), Some(MEMORY_REFUSAL));
    assert!(state.update(Msg::Apply).is_empty());

    // a stage-count change drops remembered colours, so it needs the memory too
    let mut state = ready(memory_unreadable());
    edit(&mut state, Edit::AddStage);
    edit(&mut state, Edit::DpiText(3, "3200".to_owned()));
    assert_eq!(state.apply_blocked().as_deref(), Some(MEMORY_REFUSAL));
    assert!(state.update(Msg::Apply).is_empty());

    // the next read with a readable file lifts the refusal
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(snapshot())));
    assert_eq!(state.error(), None);
    assert_eq!(state.apply_blocked(), None);
}

// --- waiting for another gctl --------------------------------------------------

#[test]
fn a_lock_wait_shows_its_own_reason_until_the_answer_arrives() {
    let mut state = ready(snapshot());
    state.update(Msg::Tick);
    assert_eq!(state.apply_blocked().as_deref(), Some("reading the mouse"));
    assert!(state.update(Msg::LockWait).is_empty());
    assert_eq!(
        state.apply_blocked().as_deref(),
        Some("waiting for another gctl to finish")
    );
    state.update(Msg::Loaded(Ok(snapshot())));
    assert_eq!(state.apply_blocked(), None);

    // the next transaction starts with its own reason, not the old wait
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    state.update(Msg::Apply);
    assert_eq!(
        state.apply_blocked().as_deref(),
        Some("writing to the mouse")
    );
}

#[test]
fn a_lock_wait_with_nothing_in_flight_changes_nothing() {
    let mut state = ready(snapshot());
    assert!(state.update(Msg::LockWait).is_empty());
    assert_eq!(state.apply_blocked(), None);
}

// --- export and import paths ---------------------------------------------------

#[test]
fn an_empty_export_or_import_path_is_a_notice_and_starts_nothing() {
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Device));
    assert!(state.update(Msg::Export(PathBuf::new())).is_empty());
    assert_eq!(
        state.notice(),
        Some(&Notice::Failed("Enter a file path.".to_owned()))
    );
    assert_eq!(state.apply_blocked(), None);
    assert!(state.update(Msg::Import(PathBuf::new())).is_empty());
    assert_eq!(
        state.notice(),
        Some(&Notice::Failed("Enter a file path.".to_owned()))
    );
    assert_eq!(state.apply_blocked(), None);
    assert_eq!(state.prompt(), None);
}

#[test]
fn an_empty_path_during_a_read_leaves_the_read_running() {
    let mut state = ready(snapshot());
    state.update(Msg::Tick);
    assert!(state.update(Msg::Export(PathBuf::new())).is_empty());
    assert_eq!(state.apply_blocked().as_deref(), Some("reading the mouse"));
    // the line is still held: a second read cannot start
    assert!(state.update(Msg::Tick).is_empty());
}

#[test]
fn an_asleep_mouse_blocks_export_and_import_with_their_own_reason() {
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Device));
    assert_eq!(state.file_blocked(), None);
    state.update(Msg::Loaded(Err(LoadError::Asleep)));
    assert_eq!(
        state.file_blocked().as_deref(),
        Some("the mouse is asleep; wake it to export or import")
    );
    // the gate the buttons read is the gate the messages obey
    assert!(
        state
            .update(Msg::Export(PathBuf::from("/x/p.toml")))
            .is_empty()
    );
    assert!(
        state
            .update(Msg::Import(PathBuf::from("/x/p.toml")))
            .is_empty()
    );
    assert_eq!(state.prompt(), None);
    state.update(Msg::Loaded(Ok(snapshot())));
    assert_eq!(state.file_blocked(), None);
    assert_eq!(
        state.update(Msg::Export(PathBuf::from("/x/p.toml"))).len(),
        1
    );
}

#[test]
fn a_running_read_blocks_export_and_import_with_the_running_reason() {
    let mut state = ready(snapshot());
    state.update(Msg::Tick);
    assert_eq!(state.file_blocked().as_deref(), Some("reading the mouse"));
}

#[test]
fn an_asleep_mouse_blocks_make_active_with_its_own_reason() {
    let mut state = ready(snapshot());
    state.update(Msg::PickProfile(profile(3)));
    let mut three = snapshot();
    three.profile = profile(3);
    state.update(Msg::Loaded(Ok(three)));
    assert_eq!(state.make_active_blocked(), None);
    state.update(Msg::Loaded(Err(LoadError::Asleep)));
    assert_eq!(
        state.make_active_blocked().as_deref(),
        Some("the mouse is asleep; wake it to change the active profile")
    );
    assert!(state.update(Msg::MakeActive).is_empty());
}

#[test]
fn a_tick_while_ready_reloads_the_edited_profile_and_keeps_pending_edits() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(
        state.update(Msg::Tick),
        vec![Request::Load(Some(profile(2)))]
    );
    let mut fresh = snapshot();
    fresh.battery.percent = Some(34);
    state.update(Msg::Loaded(Ok(fresh.clone())));
    assert_eq!(state.snapshot(), Some(&fresh));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::Debounce(Debounce::new(4).unwrap())])
    );
}

#[test]
fn a_reload_shows_a_value_changed_elsewhere_in_a_section_without_edits() {
    let mut state = ready(snapshot());
    let mut fresh = snapshot();
    fresh.polling_rate = PollingRate::Hz250;
    state.update(Msg::Loaded(Ok(fresh)));
    // the draft follows the device, so nothing is pending and Revert is a no-op
    assert!(!state.pending(Section::Performance));
    state.update(Msg::Revert);
    assert_eq!(state.plan(Section::Performance), Ok(Vec::new()));
}

// --- pending edits, Revert ---------------------------------------------------

#[test]
fn pending_tracks_the_difference_not_the_act_of_editing() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    assert!(state.pending(Section::Performance));
    assert!(!state.pending(Section::Lighting));
    edit(&mut state, Edit::PollingRate(PollingRate::Hz1000));
    assert!(!state.pending(Section::Performance));
}

#[test]
fn revert_drops_the_current_sections_edits() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::LiftOff(LiftOff::Mm2));
    edit(&mut state, Edit::Debounce(0));
    assert!(state.update(Msg::Revert).is_empty());
    assert!(!state.pending(Section::Performance));
    assert_eq!(state.plan(Section::Performance), Ok(Vec::new()));
}

// --- leaving a section or a profile with pending edits ----------------------

#[test]
fn switching_sections_without_pending_edits_switches_at_once() {
    let mut state = ready(snapshot());
    assert!(state.update(Msg::Select(Section::Lighting)).is_empty());
    assert_eq!(state.section(), Section::Lighting);
    assert_eq!(state.prompt(), None);
}

#[test]
fn leaving_a_section_with_pending_edits_asks_first() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(0));
    assert!(state.update(Msg::Select(Section::Buttons)).is_empty());
    assert_eq!(state.section(), Section::Performance);
    assert_eq!(
        state.prompt(),
        Some(Prompt::Discard(Leave::Section(Section::Buttons)))
    );

    // keep editing: nothing moves, the edit stays
    assert!(state.update(Msg::KeepEditing).is_empty());
    assert_eq!(state.prompt(), None);
    assert_eq!(state.section(), Section::Performance);
    assert!(state.pending(Section::Performance));

    // discard: the edit goes, the section switches
    state.update(Msg::Select(Section::Buttons));
    assert!(state.update(Msg::Discard).is_empty());
    assert_eq!(state.prompt(), None);
    assert_eq!(state.section(), Section::Buttons);
    assert!(!state.pending(Section::Performance));
}

#[test]
fn picking_another_profile_loads_it() {
    let mut state = ready(snapshot());
    assert_eq!(
        state.update(Msg::PickProfile(profile(3))),
        vec![Request::Load(Some(profile(3)))]
    );
    // the same profile again is not a reload
    assert!(state.update(Msg::PickProfile(profile(2))).is_empty());
}

#[test]
fn picking_a_profile_with_pending_edits_asks_first() {
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Buttons));
    edit(
        &mut state,
        Edit::Bind(Button::Back, Binding::Mouse(MouseFn::Forward)),
    );
    assert!(state.update(Msg::PickProfile(profile(1))).is_empty());
    assert_eq!(
        state.prompt(),
        Some(Prompt::Discard(Leave::Profile(profile(1))))
    );
    assert_eq!(
        state.update(Msg::Discard),
        vec![Request::Load(Some(profile(1)))]
    );
    assert!(!state.pending(Section::Buttons));
}

#[test]
fn make_active_sets_the_edited_profile_active_and_keeps_editing_it() {
    let mut state = ready(snapshot());
    state.update(Msg::PickProfile(profile(3)));
    let mut three = snapshot();
    three.profile = profile(3);
    state.update(Msg::Loaded(Ok(three)));
    assert_eq!(state.edit_profile(), Some(profile(3)));
    assert_eq!(state.snapshot().unwrap().active, profile(2));
    assert_eq!(
        state.update(Msg::MakeActive),
        vec![Request::MakeActive(profile(3))]
    );
    // the reply is a fresh snapshot of the same edited profile
    let mut after = snapshot();
    after.profile = profile(3);
    after.active = profile(3);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(after),
    });
    assert_eq!(state.edit_profile(), Some(profile(3)));
    assert_eq!(state.snapshot().unwrap().active, profile(3));
}

#[test]
fn make_active_on_the_active_profile_does_nothing() {
    let mut state = ready(snapshot());
    assert!(state.update(Msg::MakeActive).is_empty());
}

// --- Apply ------------------------------------------------------------------

#[test]
fn apply_sends_only_the_current_sections_changed_writes_device_wide_first() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    assert_eq!(
        state.update(Msg::Apply),
        vec![Request::Apply {
            profile: profile(2),
            ops: vec![
                WriteOp::PollingRate(PollingRate::Hz500),
                WriteOp::Debounce(Debounce::new(4).unwrap()),
            ],
        }]
    );
}

#[test]
fn apply_with_nothing_pending_sends_nothing() {
    let mut state = ready(snapshot());
    assert!(state.update(Msg::Apply).is_empty());
}

#[test]
fn a_second_apply_while_writing_is_refused() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    assert_eq!(
        state.apply_blocked().as_deref(),
        Some("writing to the mouse")
    );
    assert!(state.update(Msg::Apply).is_empty());
    // neither a tick nor a profile pick reloads under a write in flight
    assert!(state.update(Msg::Tick).is_empty());
    assert!(state.update(Msg::PickProfile(profile(1))).is_empty());
    assert!(state.update(Msg::MakeActive).is_empty());
}

#[test]
fn a_successful_apply_takes_the_fresh_snapshot_and_clears_the_section() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::Apply);
    let mut fresh = snapshot();
    fresh.debounce = Debounce::new(4).unwrap();
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(fresh.clone()),
    });
    assert_eq!(state.snapshot(), Some(&fresh));
    assert!(!state.pending(Section::Performance));
    assert_eq!(state.apply_blocked(), None);
    assert_eq!(state.error(), None);
}

#[test]
fn a_failed_apply_shows_the_error_and_keeps_what_did_not_land_pending() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::Apply);
    // the polling rate landed, the debounce write failed
    let mut fresh = snapshot();
    fresh.polling_rate = PollingRate::Hz500;
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: None,
        snapshot: Ok(fresh),
    });
    assert_eq!(state.error(), Some("HID transport failed: gone"));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::Debounce(Debounce::new(4).unwrap())])
    );
    assert_eq!(state.apply_blocked(), None);
}

#[test]
fn an_apply_whose_reload_finds_the_mouse_asleep_keeps_the_edits() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::Apply);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "device is asleep".to_owned(),
            landed: Vec::new(),
            reached: false,
        }),
        remembered: None,
        snapshot: Err(LoadError::Asleep),
    });
    assert_eq!(state.connection(), Connection::Asleep);
    assert_eq!(state.error(), Some("device is asleep"));
    assert!(state.pending(Section::Performance));
}

// --- Performance: DPI stages, sleep -----------------------------------------

#[test]
fn a_new_stage_list_rewrites_the_active_stage_since_the_write_resets_it_to_1() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::AddStage);
    edit(&mut state, Edit::DpiText(3, "3200".to_owned()));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![
            WriteOp::Dpi(stages(&[400, 800, 1600, 3200])),
            WriteOp::DpiStage(DpiStage::new(2).unwrap()),
        ])
    );
}

#[test]
fn changing_only_the_active_stage_writes_only_the_stage() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::ActiveStage(3));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::DpiStage(DpiStage::new(3).unwrap())])
    );
    // a stage beyond the list is ignored
    edit(&mut state, Edit::ActiveStage(4));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::DpiStage(DpiStage::new(3).unwrap())])
    );
}

#[test]
fn removing_the_active_stage_moves_the_active_stage_into_range() {
    let mut snap = snapshot();
    snap.dpi_stage = DpiStage::new(3).unwrap();
    let mut state = ready(snap);
    edit(&mut state, Edit::RemoveStage(2));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![
            WriteOp::Dpi(stages(&[400, 800])),
            WriteOp::DpiStage(DpiStage::new(2).unwrap()),
        ])
    );
}

#[test]
fn the_stage_list_holds_one_to_six_stages() {
    let mut state = ready(snapshot());
    for _ in 0..10 {
        edit(&mut state, Edit::AddStage);
    }
    match &state.plan(Section::Performance).unwrap()[0] {
        WriteOp::Dpi(list) => assert_eq!(list.as_slice().len(), 6),
        other => panic!("first write is not the stage list: {other:?}"),
    }
    for _ in 0..10 {
        edit(&mut state, Edit::RemoveStage(0));
    }
    match &state.plan(Section::Performance).unwrap()[0] {
        WriteOp::Dpi(list) => assert_eq!(list.as_slice().len(), 1),
        other => panic!("first write is not the stage list: {other:?}"),
    }
}

#[test]
fn remembered_stage_colours_follow_the_stage_list() {
    let mut snap = snapshot();
    snap.dpi_colors = Some(vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")]);
    let mut state = ready(snap);
    edit(&mut state, Edit::RemoveStage(1));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![
            WriteOp::Dpi(stages(&[400, 1600])),
            WriteOp::DpiStage(DpiStage::new(2).unwrap()),
            WriteOp::DpiColors(vec![rgb("ff0000"), rgb("0000ff")]),
        ])
    );
}

#[test]
fn colouring_one_stage_of_unknown_colours_fills_the_rest_white() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::StageColor(1, rgb("123456")));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::DpiColors(vec![
            rgb("ffffff"),
            rgb("123456"),
            rgb("ffffff"),
        ])])
    );
}

#[test]
fn colouring_a_stage_past_a_remembered_list_of_the_wrong_length_treats_it_as_unknown() {
    // another tool changed the stage count after gctl remembered three colours
    let mut snap = snapshot();
    snap.dpi = stages(&[400, 800, 1600, 3200]);
    snap.dpi_colors = Some(vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")]);
    let mut state = ready(snap);
    edit(&mut state, Edit::StageColor(3, rgb("123456")));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::DpiColors(vec![
            rgb("ffffff"),
            rgb("ffffff"),
            rgb("ffffff"),
            rgb("123456"),
        ])])
    );
}

#[test]
fn a_signed_sleep_number_is_refused_like_the_cli() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::SleepMinutes("+5".to_owned()));
    assert_eq!(
        state.plan(Section::Performance),
        Err("sleep minutes must be a whole number, got \"+5\"".to_owned())
    );
}

#[test]
fn an_invalid_stage_value_blocks_apply_and_names_the_stage() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::DpiText(1, "99".to_owned()));
    assert!(state.pending(Section::Performance));
    let expected = "stage 2: DPI 99 is out of range 100..=19000";
    assert_eq!(state.plan(Section::Performance), Err(expected.to_owned()));
    assert_eq!(state.apply_blocked().as_deref(), Some(expected));
    assert!(state.update(Msg::Apply).is_empty());

    edit(&mut state, Edit::DpiText(1, "8x0".to_owned()));
    assert_eq!(
        state.plan(Section::Performance),
        Err("stage 2: DPI must be in 100..=19000, got \"8x0\"".to_owned())
    );
}

#[test]
fn sleep_takes_never_or_minutes_and_seconds() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::SleepMinutes("1".to_owned()));
    edit(&mut state, Edit::SleepSeconds("30".to_owned()));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::Sleep(SleepDelay::Seconds(90))])
    );
    edit(&mut state, Edit::SleepNever(true));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::Sleep(SleepDelay::Never)])
    );
}

#[test]
fn a_zero_or_too_long_sleep_blocks_apply() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::SleepMinutes("0".to_owned()));
    edit(&mut state, Edit::SleepSeconds("0".to_owned()));
    assert_eq!(
        state.plan(Section::Performance),
        Err("sleep delay is zero; tick never instead".to_owned())
    );
    edit(&mut state, Edit::SleepMinutes("1092".to_owned()));
    edit(&mut state, Edit::SleepSeconds("15".to_owned()));
    // 65535 s is the wire's `never`; 65534 s is the longest delay
    assert_eq!(
        state.plan(Section::Performance),
        Err("sleep delay exceeds 65534 seconds".to_owned())
    );
    edit(&mut state, Edit::SleepSeconds("14".to_owned()));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::Sleep(SleepDelay::Seconds(65534))])
    );
}

// --- Lighting ---------------------------------------------------------------

#[test]
fn an_unknown_effect_stays_unwritten_until_one_is_picked() {
    let mut state = ready(snapshot());
    // a speed or colour with no effect chosen yet changes nothing
    edit(&mut state, Edit::Rate(80));
    edit(&mut state, Edit::EffectColor(0, rgb("ff0000")));
    assert!(!state.pending(Section::Lighting));
    edit(&mut state, Edit::Effect(EffectKind::Wave));
    assert_eq!(
        state.plan(Section::Lighting),
        Ok(vec![WriteOp::Effect(Effect::Wave {
            rate: Rate::DEFAULT
        })])
    );
}

#[test]
fn switching_effect_keeps_the_speed_and_fits_the_colour_count() {
    let mut snap = snapshot();
    snap.effect = Some(Effect::Pulse {
        rate: rate(70),
        colors: vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")],
    });
    let mut state = ready(snap);
    edit(&mut state, Edit::Effect(EffectKind::Rave));
    assert_eq!(
        state.plan(Section::Lighting),
        Ok(vec![WriteOp::Effect(Effect::Rave {
            rate: rate(70),
            colors: vec![rgb("ff0000"), rgb("00ff00")],
        })])
    );
    edit(&mut state, Edit::Effect(EffectKind::Solid));
    assert_eq!(
        state.plan(Section::Lighting),
        Ok(vec![WriteOp::Effect(Effect::Solid {
            color: rgb("ff0000")
        })])
    );
    edit(&mut state, Edit::Effect(EffectKind::Pulse));
    let plan = state.plan(Section::Lighting).unwrap();
    let [WriteOp::Effect(effect)] = plan.as_slice() else {
        panic!("{plan:?}");
    };
    effect.check().unwrap();
}

#[test]
fn effect_colours_stay_within_the_effects_bounds() {
    let mut snap = snapshot();
    snap.effect = Some(Effect::Rave {
        rate: rate(40),
        colors: vec![rgb("ff0000"), rgb("00ff00")],
    });
    let mut state = ready(snap);
    edit(&mut state, Edit::AddEffectColor);
    assert!(!state.pending(Section::Lighting), "rave takes at most 2");
    edit(&mut state, Edit::RemoveEffectColor(0));
    edit(&mut state, Edit::RemoveEffectColor(0));
    assert_eq!(
        state.plan(Section::Lighting),
        Ok(vec![WriteOp::Effect(Effect::Rave {
            rate: rate(40),
            colors: vec![rgb("00ff00")],
        })])
    );
}

#[test]
fn effect_labels_title_case_the_cli_token() {
    assert_eq!(EffectKind::PulseOne.label(), "Pulse one");
    assert_eq!(EffectKind::Off.label(), "Off");
    assert_eq!(EffectKind::Glorious.label(), "Glorious");
    assert_eq!(EffectKind::ALL.len(), 9);
}

#[test]
fn brightness_writes_both_links_and_goes_before_the_effect() {
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Lighting));
    edit(&mut state, Edit::Effect(EffectKind::Off));
    edit(&mut state, Edit::BrightnessWireless(50));
    edit(&mut state, Edit::BrightnessWired(200));
    assert_eq!(
        state.plan(Section::Lighting),
        Ok(vec![
            WriteOp::Brightness(Brightness {
                wired: 200,
                wireless: 50
            }),
            WriteOp::Effect(Effect::Off),
        ])
    );
}

#[test]
fn an_unknown_brightness_needs_both_links_before_apply() {
    // gctl never invents the link the user did not set
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Lighting));
    edit(&mut state, Edit::BrightnessWired(200));
    assert!(state.pending(Section::Lighting));
    assert_eq!(
        state.apply_blocked().as_deref(),
        Some("set the wireless brightness too")
    );
    assert!(state.update(Msg::Apply).is_empty());
    edit(&mut state, Edit::BrightnessWireless(50));
    assert_eq!(state.apply_blocked(), None);
    assert_eq!(
        state.update(Msg::Apply),
        vec![Request::Apply {
            profile: profile(2),
            ops: vec![WriteOp::Brightness(Brightness {
                wired: 200,
                wireless: 50
            })],
        }]
    );

    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Lighting));
    edit(&mut state, Edit::BrightnessWireless(50));
    assert_eq!(
        state.apply_blocked().as_deref(),
        Some("set the wired brightness too")
    );
}

#[test]
fn releasing_an_unknown_brightness_slider_sets_zero_only_when_nothing_was_picked() {
    // a press at 121 and its release can land in one batch: the release keeps 121
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Lighting));
    edit(&mut state, Edit::BrightnessWired(121));
    edit(&mut state, Edit::BrightnessWiredRelease);
    assert_eq!(state.brightness_wired(), Some(121));
    // a click at the left edge changes nothing, so only the release says "0"
    edit(&mut state, Edit::BrightnessWirelessRelease);
    assert_eq!(state.brightness_wireless(), Some(0));
    assert_eq!(
        state.plan(Section::Lighting),
        Ok(vec![WriteOp::Brightness(Brightness {
            wired: 121,
            wireless: 0
        })])
    );
}

#[test]
fn releasing_a_remembered_brightness_slider_changes_nothing() {
    let mut snap = snapshot();
    snap.brightness = Some(Brightness {
        wired: 200,
        wireless: 50,
    });
    let mut state = ready(snap);
    state.update(Msg::Select(Section::Lighting));
    edit(&mut state, Edit::BrightnessWiredRelease);
    assert_eq!(state.brightness_wired(), Some(200));
    assert!(!state.pending(Section::Lighting));
}

#[test]
fn a_remembered_brightness_changes_one_link_and_keeps_the_other() {
    let mut snap = snapshot();
    snap.brightness = Some(Brightness {
        wired: 200,
        wireless: 50,
    });
    let mut state = ready(snap);
    state.update(Msg::Select(Section::Lighting));
    edit(&mut state, Edit::BrightnessWired(100));
    assert_eq!(
        state.plan(Section::Lighting),
        Ok(vec![WriteOp::Brightness(Brightness {
            wired: 100,
            wireless: 50
        })])
    );
}

// --- Buttons ----------------------------------------------------------------

#[test]
fn binds_write_in_button_order_after_the_scroll_direction() {
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Buttons));
    edit(
        &mut state,
        Edit::Bind(Button::Dpi, Binding::Media(MediaFn::Mute)),
    );
    edit(&mut state, Edit::Bind(Button::Back, Binding::None));
    edit(&mut state, Edit::Scroll(ScrollDirection::Invert));
    assert_eq!(
        state.plan(Section::Buttons),
        Ok(vec![
            WriteOp::Scroll(ScrollDirection::Invert),
            WriteOp::Bind(Button::Back, Binding::None),
            WriteOp::Bind(Button::Dpi, Binding::Media(MediaFn::Mute)),
        ])
    );
}

#[test]
fn rebinding_a_remembered_button_to_its_value_is_not_pending() {
    let mut snap = snapshot();
    snap.binds[3] = (Button::Back, Some(Binding::Mouse(MouseFn::Back)));
    let mut state = ready(snap);
    state.update(Msg::Select(Section::Buttons));
    edit(
        &mut state,
        Edit::Bind(Button::Back, Binding::Mouse(MouseFn::Back)),
    );
    assert!(!state.pending(Section::Buttons));
}

#[test]
fn the_device_section_never_holds_pending_edits() {
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Device));
    assert!(!state.pending(Section::Device));
    assert_eq!(state.plan(Section::Device), Ok(Vec::new()));
}

// --- edits across reloads (review round 3) ----------------------------------------

/// The fake mouse with profile 3 open for editing while profile 2 stays active.
fn editing_three(state: &mut State) {
    assert_eq!(
        state.update(Msg::PickProfile(profile(3))),
        vec![Request::Load(Some(profile(3)))]
    );
    state.update(Msg::Loaded(Ok(Snapshot {
        profile: profile(3),
        ..snapshot()
    })));
    assert_eq!(state.edit_profile(), Some(profile(3)));
}

#[test]
fn a_no_mouse_moment_reloads_the_edited_profile_and_keeps_its_edits() {
    let mut state = ready(snapshot());
    editing_three(&mut state);
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Err(LoadError::NoMouse)));
    assert_eq!(
        state.update(Msg::Tick),
        vec![Request::Load(Some(profile(3)))]
    );
    state.update(Msg::Loaded(Ok(Snapshot {
        profile: profile(3),
        ..snapshot()
    })));
    assert_eq!(state.edit_profile(), Some(profile(3)));
    assert_eq!(state.debounce(), Some(4));
    assert!(state.pending(Section::Performance));
}

#[test]
fn a_reload_keeps_a_half_typed_stage_colour() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::StageColorHex(0, "ff".to_owned()));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(snapshot())));
    assert_eq!(state.stage_color_hex(0), "ff");
    assert!(
        state.apply_blocked().is_some(),
        "a half-typed colour blocks Apply"
    );
}

#[test]
fn a_reload_keeps_a_half_typed_effect_colour() {
    let mut snap = snapshot();
    snap.effect = Some(Effect::Pulse {
        rate: rate(40),
        colors: vec![rgb("ff0000"), rgb("0000ff")],
    });
    let mut state = ready(snap.clone());
    state.update(Msg::Select(Section::Lighting));
    edit(&mut state, Edit::EffectColorHex(0, "ff".to_owned()));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(snap)));
    assert_eq!(state.effect_color_hex(0), "ff");
    assert!(
        state.apply_blocked().is_some(),
        "a half-typed colour blocks Apply"
    );
}

#[test]
fn an_edit_survives_two_outside_changes() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    for outside in [4, 7] {
        state.update(Msg::Tick);
        state.update(Msg::Loaded(Ok(Snapshot {
            debounce: Debounce::new(outside).unwrap(),
            ..snapshot()
        })));
    }
    assert_eq!(state.debounce(), Some(4));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::Debounce(Debounce::new(4).unwrap())])
    );
}

#[test]
fn an_edit_made_during_an_apply_survives_its_reread() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    // back to the old value while the write runs
    edit(&mut state, Edit::Debounce(10));
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(Snapshot {
            debounce: Debounce::new(4).unwrap(),
            ..snapshot()
        }),
    });
    assert_eq!(state.debounce(), Some(10));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::Debounce(Debounce::new(10).unwrap())])
    );
}

#[test]
fn a_revert_forgets_which_fields_were_edited() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::Revert);
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: Debounce::new(7).unwrap(),
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(7));
    assert!(!state.pending(Section::Performance));
}

// The stage list, the active stage and the stage colours are one edited unit:
// the first DPI edit captures the list and the colours, and reloads never move
// them. The active stage is the entry's only once the user picks one; until
// then it follows the latest read, clamped to the entry's list.

#[test]
fn an_unpicked_active_stage_past_the_shorter_list_is_the_last_stage() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::RemoveStage(2));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        dpi_stage: DpiStage::new(3).unwrap(),
        ..snapshot()
    })));
    assert_eq!(state.active_stage(), Some(2));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![
            WriteOp::Dpi(stages(&[400, 800])),
            WriteOp::DpiStage(DpiStage::new(2).unwrap()),
        ])
    );
}

#[test]
fn a_colour_edit_keeps_the_whole_dpi_unit() {
    let mut snap = snapshot();
    snap.dpi_colors = Some(vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")]);
    let mut state = ready(snap.clone());
    edit(&mut state, Edit::StageColorHex(0, "123456".to_owned()));
    // `gctl set dpi 400 800 1600 3200` elsewhere drops the remembered colours
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        dpi: stages(&[400, 800, 1600, 3200]),
        dpi_colors: None,
        ..snap
    })));
    assert_eq!(
        state.stage_texts(),
        Some(vec!["400".to_owned(), "800".to_owned(), "1600".to_owned()])
    );
    assert_eq!(state.stage_color_hex(0), "123456");
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![
            WriteOp::Dpi(stages(&[400, 800, 1600])),
            WriteOp::DpiStage(DpiStage::new(2).unwrap()),
            WriteOp::DpiColors(vec![rgb("123456"), rgb("00ff00"), rgb("0000ff")]),
        ])
    );
}

#[test]
fn a_stage_value_edit_keeps_the_edited_active_stage() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::ActiveStage(3));
    // the mouse's DPI button lands on stage 3; the tick reads it
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        dpi_stage: DpiStage::new(3).unwrap(),
        ..snapshot()
    })));
    edit(&mut state, Edit::DpiText(0, "500".to_owned()));
    // the DPI button again, to stage 1
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        dpi_stage: DpiStage::new(1).unwrap(),
        ..snapshot()
    })));
    assert_eq!(state.active_stage(), Some(3));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![
            WriteOp::Dpi(stages(&[500, 800, 1600])),
            WriteOp::DpiStage(DpiStage::new(3).unwrap()),
        ])
    );
}

// --- what the window says (review round 3) ----------------------------------------

#[test]
fn a_write_failure_shows_above_the_memory_refusal() {
    let mut state = ready(memory_unreadable());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: None,
        snapshot: Ok(memory_unreadable()),
    });
    assert_eq!(
        state.error(),
        Some(format!("HID transport failed: gone\n{MEMORY_REFUSAL}").as_str())
    );
}

#[test]
fn the_import_check_and_question_say_what_they_wait_for() {
    let mut state = ready(snapshot());
    state.update(Msg::Import(import_path()));
    assert_eq!(
        state.apply_blocked().as_deref(),
        Some("checking the import file")
    );
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: true,
        writes: Vec::new(),
    })));
    assert_eq!(
        state.apply_blocked().as_deref(),
        Some("waiting for the import answer")
    );
    state.update(Msg::ImportConfirm);
    assert_eq!(
        state.apply_blocked().as_deref(),
        Some("writing to the mouse")
    );
}

#[test]
fn an_import_failure_shows_once_as_a_notice() {
    let mut state = ready(snapshot());
    state.update(Msg::Import(import_path()));
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: Vec::new(),
    })));
    state.update(Msg::Imported {
        error: Some("device is asleep".to_owned()),
        reached: false,
        written: Vec::new(),
        remembered: None,
        snapshot: Some(Ok(snapshot())),
    });
    assert_eq!(
        state.notice(),
        Some(&Notice::Failed("device is asleep".to_owned()))
    );
    assert_eq!(state.error(), None);
}

#[test]
fn only_the_import_answers_close_the_import_question() {
    let mut state = ready(snapshot());
    state.update(Msg::Import(import_path()));
    let check = ImportCheck {
        note: None,
        device_wide: true,
        writes: Vec::new(),
    };
    state.update(Msg::ImportChecked(Ok(check.clone())));
    state.update(Msg::KeepEditing);
    assert_eq!(state.prompt(), Some(Prompt::Import(check.clone())));
    state.update(Msg::Select(Section::Lighting));
    assert_eq!(state.prompt(), Some(Prompt::Import(check)));
    assert!(
        state.update(Msg::Tick).is_empty(),
        "the question still holds the line"
    );
    state.update(Msg::ImportCancel);
    assert_eq!(state.prompt(), None);
    assert_eq!(
        state.update(Msg::Tick),
        vec![Request::Load(Some(profile(2)))]
    );
}

// --- after a successful Apply (review round 4) ------------------------------------

fn apply_stage_three(state: &mut State) {
    edit(state, Edit::ActiveStage(3));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(Snapshot {
            dpi_stage: DpiStage::new(3).unwrap(),
            ..snapshot()
        }),
    });
    assert!(!state.pending(Section::Performance));
}

#[test]
fn an_applied_field_follows_the_mouse_again() {
    let mut state = ready(snapshot());
    apply_stage_three(&mut state);
    // the mouse's DPI button moves the stage to 1; the next tick reads it
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        dpi_stage: DpiStage::new(1).unwrap(),
        ..snapshot()
    })));
    assert_eq!(state.active_stage(), Some(1));
    assert!(!state.pending(Section::Performance));
}

#[test]
fn a_later_apply_never_rewrites_an_applied_field() {
    let mut state = ready(snapshot());
    apply_stage_three(&mut state);
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        dpi_stage: DpiStage::new(1).unwrap(),
        ..snapshot()
    })));
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::PollingRate(PollingRate::Hz500)])
    );
}

#[test]
fn a_failed_apply_keeps_its_fields_edited() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::ActiveStage(3));
    state.update(Msg::Apply);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: None,
        snapshot: Ok(snapshot()),
    });
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(snapshot())));
    assert_eq!(state.active_stage(), Some(3));
    assert!(state.pending(Section::Performance));
}

#[test]
fn an_edit_outside_the_applied_plan_stays_edited() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(7));
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    // `gctl set debounce 7` elsewhere; the tick reads it
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: Debounce::new(7).unwrap(),
        ..snapshot()
    })));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::PollingRate(PollingRate::Hz500)])
    );
    state.update(Msg::Apply);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(Snapshot {
            debounce: Debounce::new(7).unwrap(),
            polling_rate: PollingRate::Hz500,
            ..snapshot()
        }),
    });
    // `gctl set debounce 4` elsewhere; the user's 7 still stands
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: Debounce::new(4).unwrap(),
        polling_rate: PollingRate::Hz500,
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(7));
    assert_eq!(state.polling_rate(), Some(PollingRate::Hz500));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::Debounce(Debounce::new(7).unwrap())])
    );
}

#[test]
fn an_applied_stage_colour_text_follows_the_mouse() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::StageColorHex(0, "123456".to_owned()));
    state.update(Msg::Apply);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(Snapshot {
            dpi_colors: Some(vec![rgb("123456"), rgb("ffffff"), rgb("ffffff")]),
            ..snapshot()
        }),
    });
    assert!(!state.pending(Section::Performance));
    // `gctl set dpi-colors ff0000 00ff00 0000ff` elsewhere
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        dpi_colors: Some(vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")]),
        ..snapshot()
    })));
    assert_eq!(state.stage_colors().unwrap()[0], rgb("ff0000"));
    assert_eq!(state.stage_color_hex(0), "ff0000");
    assert!(!state.pending(Section::Performance));
}

#[test]
fn an_applied_effect_colour_text_follows_the_mouse() {
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Lighting));
    edit(&mut state, Edit::Effect(EffectKind::Solid));
    edit(&mut state, Edit::EffectColorHex(0, "123456".to_owned()));
    state.update(Msg::Apply);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(Snapshot {
            effect: Some(Effect::Solid {
                color: rgb("123456"),
            }),
            ..snapshot()
        }),
    });
    // `gctl set effect solid 00ff00` elsewhere
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        effect: Some(Effect::Solid {
            color: rgb("00ff00"),
        }),
        ..snapshot()
    })));
    assert_eq!(state.effect_colors().unwrap()[0], rgb("00ff00"));
    assert_eq!(state.effect_color_hex(0), "00ff00");
    assert!(!state.pending(Section::Lighting));
}

#[test]
fn an_import_after_an_applied_colour_shows_the_imported_colour() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::StageColorHex(0, "123456".to_owned()));
    state.update(Msg::Apply);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(Snapshot {
            dpi_colors: Some(vec![rgb("123456"), rgb("ffffff"), rgb("ffffff")]),
            ..snapshot()
        }),
    });
    state.update(Msg::Select(Section::Device));
    state.update(Msg::Import(import_path()));
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: Vec::new(),
    })));
    state.update(Msg::Imported {
        error: None,
        reached: false,
        written: Vec::new(),
        remembered: None,
        snapshot: Some(Ok(Snapshot {
            dpi_colors: Some(vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")]),
            ..snapshot()
        })),
    });
    assert_eq!(state.stage_colors().unwrap()[0], rgb("ff0000"));
    assert_eq!(state.stage_color_hex(0), "ff0000");
}

#[test]
fn a_failed_import_ends_no_edit() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::Import(import_path()));
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: Vec::new(),
    })));
    state.update(Msg::Imported {
        error: Some("device is asleep".to_owned()),
        reached: false,
        written: Vec::new(),
        remembered: None,
        snapshot: Some(Ok(Snapshot {
            debounce: Debounce::new(4).unwrap(),
            ..snapshot()
        })),
    });
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: Debounce::new(7).unwrap(),
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(4));
}

#[test]
fn a_queued_import_question_holds_the_line_until_it_is_answered() {
    let mut state = ready(snapshot());
    state.update(Msg::Import(import_path()));
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::Select(Section::Lighting));
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: true,
        writes: Vec::new(),
    })));
    // no read starts under the discard question while the import waits
    assert!(state.update(Msg::Tick).is_empty());
    state.update(Msg::KeepEditing);
    assert_eq!(
        state.apply_blocked().as_deref(),
        Some("waiting for the import answer")
    );
    assert!(state.update(Msg::Tick).is_empty());
    assert!(state.update(Msg::Apply).is_empty());
    let confirm = state.update(Msg::ImportConfirm);
    assert_eq!(confirm.len(), 1, "{confirm:?}");
    assert!(matches!(confirm[0], Request::Import { .. }));
    // nothing else starts while the import writes
    assert!(state.update(Msg::Tick).is_empty());
    assert!(state.update(Msg::Apply).is_empty());
    state.update(Msg::Imported {
        error: None,
        reached: false,
        written: Vec::new(),
        remembered: None,
        snapshot: Some(Ok(snapshot())),
    });
    assert_eq!(
        state.update(Msg::Tick),
        vec![Request::Load(Some(profile(2)))]
    );
}

// --- import edges (review round 4) ------------------------------------------------

#[test]
fn an_import_that_never_started_shows_its_failure_once() {
    let mut state = ready(snapshot());
    state.update(Msg::Import(import_path()));
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: Vec::new(),
    })));
    // the mouse could not be opened: nothing was written, nothing re-read
    let message = "cannot list HID devices: gone".to_owned();
    state.update(Msg::Imported {
        error: Some(message.clone()),
        reached: false,
        written: Vec::new(),
        remembered: None,
        snapshot: None,
    });
    assert_eq!(state.notice(), Some(&Notice::Failed(message)));
    assert_eq!(state.error(), None);
    assert_eq!(state.connection(), Connection::Ready);
    assert_eq!(state.snapshot(), Some(&snapshot()));
    assert_eq!(
        state.update(Msg::Tick),
        vec![Request::Load(Some(profile(2)))]
    );
}

#[test]
fn an_import_question_waits_for_an_open_discard_question() {
    let mut state = ready(snapshot());
    state.update(Msg::Import(import_path()));
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::Select(Section::Lighting));
    let discard = Prompt::Discard(Leave::Section(Section::Lighting));
    assert_eq!(state.prompt(), Some(discard.clone()));
    let check = ImportCheck {
        note: None,
        device_wide: true,
        writes: Vec::new(),
    };
    state.update(Msg::ImportChecked(Ok(check.clone())));
    assert_eq!(state.prompt(), Some(discard));
    // answering the discard question brings the import question up
    state.update(Msg::KeepEditing);
    assert_eq!(state.prompt(), Some(Prompt::Import(check)));
    assert_eq!(state.section(), Section::Performance);
}

// --- when an edit ends (review round 6) -------------------------------------------
//
// an edit ends at the first good read after a successful write that carried its
// field, and only when that read shows exactly the edit's value; or when the user
// edits it back to what the read shows. nothing else ends it.

fn import_then(state: &mut State, written: Vec<WriteOp>, after: Snapshot) {
    state.update(Msg::Import(import_path()));
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: Vec::new(),
    })));
    state.update(Msg::Imported {
        error: None,
        reached: false,
        written,
        remembered: None,
        snapshot: Some(Ok(after)),
    });
}

#[test]
fn an_import_ends_only_the_edits_it_wrote() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(7));
    // `gctl set debounce 7` elsewhere; the tick reads it
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: Debounce::new(7).unwrap(),
        ..snapshot()
    })));
    // a file holding only a polling rate
    import_then(
        &mut state,
        vec![WriteOp::PollingRate(PollingRate::Hz500)],
        Snapshot {
            debounce: Debounce::new(7).unwrap(),
            polling_rate: PollingRate::Hz500,
            ..snapshot()
        },
    );
    // `gctl set debounce 4` elsewhere; the user's 7 still stands
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: Debounce::new(4).unwrap(),
        polling_rate: PollingRate::Hz500,
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(7));
}

#[test]
fn an_import_that_wrote_a_typed_colour_ends_that_edit() {
    let mut snap = snapshot();
    snap.dpi_colors = Some(vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")]);
    let mut state = ready(snap.clone());
    edit(&mut state, Edit::StageColorHex(0, "123456".to_owned()));
    let written = vec![rgb("123456"), rgb("00ff00"), rgb("0000ff")];
    import_then(
        &mut state,
        vec![WriteOp::DpiColors(written.clone())],
        Snapshot {
            dpi_colors: Some(written),
            ..snap.clone()
        },
    );
    assert!(!state.pending(Section::Performance));
    // `gctl set dpi-colors` elsewhere: the window follows the mouse again
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(snap)));
    assert_eq!(state.stage_color_hex(0), "ff0000");
}

#[test]
fn an_import_that_wrote_another_value_keeps_the_edit_pending() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(7));
    import_then(
        &mut state,
        vec![WriteOp::Debounce(Debounce::new(4).unwrap())],
        Snapshot {
            debounce: Debounce::new(4).unwrap(),
            ..snapshot()
        },
    );
    assert_eq!(state.debounce(), Some(7));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::Debounce(Debounce::new(7).unwrap())])
    );
}

#[test]
fn editing_back_to_the_mouse_value_ends_the_edit_even_during_a_read() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    // the tick's read is running
    assert_eq!(state.update(Msg::Tick).len(), 1);
    edit(&mut state, Edit::PollingRate(PollingRate::Hz1000));
    assert!(!state.pending(Section::Performance));
    state.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz250,
        ..snapshot()
    })));
    assert_eq!(state.polling_rate(), Some(PollingRate::Hz250));
    assert!(!state.pending(Section::Performance));
}

#[test]
fn an_emptied_sleep_field_stays_empty() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::SleepSeconds(String::new()));
    assert_eq!(state.sleep_seconds(), Some(String::new()));
    assert_eq!(state.sleep_minutes(), Some("2".to_owned()));
}

fn apply_debounce_four_then(state: &mut State, reread: Result<Snapshot, LoadError>) {
    edit(state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: reread,
    });
}

#[test]
fn a_landed_write_whose_reread_failed_ends_at_the_next_good_read() {
    let mut state = ready(snapshot());
    apply_debounce_four_then(
        &mut state,
        Err(LoadError::Transport(
            "HID transport failed: gone".to_owned(),
        )),
    );
    // the write landed; until a good read confirms it, the window keeps showing it
    assert_eq!(state.debounce(), Some(4));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: Debounce::new(4).unwrap(),
        ..snapshot()
    })));
    assert!(!state.pending(Section::Performance));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: Debounce::new(7).unwrap(),
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(7));
}

#[test]
fn a_landed_write_whose_reread_found_the_mouse_asleep_never_shows_the_old_value() {
    let mut state = ready(snapshot());
    apply_debounce_four_then(&mut state, Err(LoadError::Asleep));
    assert_eq!(state.debounce(), Some(4));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: Debounce::new(4).unwrap(),
        ..snapshot()
    })));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: Debounce::new(7).unwrap(),
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(7));
}

#[test]
fn discarding_to_switch_profile_during_a_read_switches_after_it() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    assert!(state.update(Msg::PickProfile(profile(3))).is_empty());
    assert_eq!(
        state.prompt(),
        Some(Prompt::Discard(Leave::Profile(profile(3))))
    );
    assert_eq!(state.update(Msg::Tick).len(), 1);
    // Discard while the read runs: the question closes, the edits go
    assert!(state.update(Msg::Discard).is_empty());
    assert_eq!(state.prompt(), None);
    assert!(!state.pending(Section::Performance));
    // the switch starts once the read answers
    assert_eq!(
        state.update(Msg::Loaded(Ok(snapshot()))),
        vec![Request::Load(Some(profile(3)))]
    );
}

// --- written entries wait for a good read (review round 7) ----------------------

fn transport_error() -> LoadError {
    LoadError::Transport("HID transport failed: gone".to_owned())
}

fn debounce(value: u8) -> Debounce {
    Debounce::new(value).unwrap()
}

/// Runs a plain import (no prompt) that wrote `written` and re-read `reread`.
fn import_landed(state: &mut State, written: Vec<WriteOp>, reread: Result<Snapshot, LoadError>) {
    assert_eq!(state.update(Msg::Import(import_path())).len(), 1);
    assert_eq!(
        state
            .update(Msg::ImportChecked(Ok(ImportCheck {
                note: None,
                device_wide: false,
                writes: written.clone(),
            })))
            .len(),
        1
    );
    state.update(Msg::Imported {
        error: None,
        reached: false,
        written,
        remembered: None,
        snapshot: Some(reread),
    });
}

#[test]
fn two_writes_with_failed_rereads_both_end_at_the_next_good_read() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(7));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: debounce(7),
        ..snapshot()
    })));
    state.update(Msg::Select(Section::Device));
    assert_eq!(state.section(), Section::Device);
    import_landed(
        &mut state,
        vec![WriteOp::Debounce(debounce(7))],
        Err(transport_error()),
    );
    import_landed(
        &mut state,
        vec![WriteOp::PollingRate(PollingRate::Hz500)],
        Err(transport_error()),
    );
    // the first good read shows both written values: both entries end
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: debounce(7),
        polling_rate: PollingRate::Hz500,
        ..snapshot()
    })));
    // so a later outside change shows through
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: debounce(4),
        polling_rate: PollingRate::Hz500,
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(4));
}

#[test]
fn one_write_with_a_failed_reread_ends_at_the_next_good_read() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(7));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: debounce(7),
        ..snapshot()
    })));
    state.update(Msg::Select(Section::Device));
    import_landed(
        &mut state,
        vec![WriteOp::Debounce(debounce(7))],
        Err(transport_error()),
    );
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: debounce(7),
        ..snapshot()
    })));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: debounce(4),
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(4));
}

#[test]
fn a_good_reread_after_a_failed_write_ends_the_entries_a_landed_write_left() {
    let base = Snapshot {
        active: profile(1),
        ..snapshot()
    };
    let mut state = ready(base.clone());
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Err(transport_error()),
    });
    assert_eq!(
        state.update(Msg::MakeActive),
        vec![Request::MakeActive(profile(2))]
    );
    // make-active fails, but its re-read is good and shows the Apply's value
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "the mouse refused the profile".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: None,
        snapshot: Ok(Snapshot {
            debounce: debounce(4),
            ..base.clone()
        }),
    });
    assert!(!state.pending(Section::Performance));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: debounce(7),
        ..base
    })));
    assert_eq!(state.debounce(), Some(7));
}

#[test]
fn an_edit_back_while_an_import_runs_keeps_the_entry() {
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Device));
    assert_eq!(state.update(Msg::Import(import_path())).len(), 1);
    assert_eq!(
        state
            .update(Msg::ImportChecked(Ok(ImportCheck {
                note: None,
                device_wide: false,
                writes: vec![WriteOp::Debounce(debounce(4))],
            })))
            .len(),
        1
    );
    state.update(Msg::Select(Section::Performance));
    assert_eq!(state.section(), Section::Performance);
    edit(&mut state, Edit::Debounce(7));
    // back to the snapshot's 10, which the running import is about to change
    edit(&mut state, Edit::Debounce(10));
    state.update(Msg::Imported {
        error: None,
        reached: false,
        written: vec![WriteOp::Debounce(debounce(4))],
        remembered: None,
        snapshot: Some(Ok(Snapshot {
            debounce: debounce(4),
            ..snapshot()
        })),
    });
    assert_eq!(state.debounce(), Some(10));
    assert!(state.pending(Section::Performance));
}

#[test]
fn an_edit_back_while_an_import_runs_ends_an_entry_the_import_does_not_write() {
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Device));
    assert_eq!(state.update(Msg::Import(import_path())).len(), 1);
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: vec![WriteOp::PollingRate(PollingRate::Hz500)],
    })));
    state.update(Msg::Select(Section::Performance));
    edit(&mut state, Edit::Debounce(7));
    edit(&mut state, Edit::Debounce(10));
    assert!(!state.pending(Section::Performance));
    state.update(Msg::Imported {
        error: None,
        reached: false,
        written: vec![WriteOp::PollingRate(PollingRate::Hz500)],
        remembered: None,
        snapshot: Some(Ok(Snapshot {
            polling_rate: PollingRate::Hz500,
            ..snapshot()
        })),
    });
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz500,
        debounce: debounce(4),
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(4));
}

#[test]
fn an_edit_back_while_a_landed_write_waits_for_its_reread_keeps_the_entry() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Err(transport_error()),
    });
    // the snapshot still says 10, but the mouse holds 4
    edit(&mut state, Edit::Debounce(10));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: debounce(4),
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(10));
    assert!(state.pending(Section::Performance));
}

#[test]
fn a_stage_typed_with_a_leading_zero_ends_at_its_write() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::DpiText(0, "0500".to_owned()));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![
            WriteOp::Dpi(stages(&[500, 800, 1600])),
            WriteOp::DpiStage(DpiStage::new(2).unwrap())
        ])
    );
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(Snapshot {
            dpi: stages(&[500, 800, 1600]),
            ..snapshot()
        }),
    });
    assert!(!state.pending(Section::Performance));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(snapshot())));
    assert_eq!(
        state.stage_texts(),
        Some(vec!["400".to_owned(), "800".to_owned(), "1600".to_owned()])
    );
}

#[test]
fn a_stage_typed_plain_ends_at_its_write() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::DpiText(0, "500".to_owned()));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(Snapshot {
            dpi: stages(&[500, 800, 1600]),
            ..snapshot()
        }),
    });
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(snapshot())));
    assert_eq!(
        state.stage_texts(),
        Some(vec!["400".to_owned(), "800".to_owned(), "1600".to_owned()])
    );
}

fn three_stage_colours() -> Snapshot {
    Snapshot {
        dpi_colors: Some(vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")]),
        ..snapshot()
    }
}

#[test]
fn an_emptied_stage_colour_stays_empty_and_blocks_apply() {
    let mut state = ready(three_stage_colours());
    edit(&mut state, Edit::StageColorHex(0, String::new()));
    assert_eq!(state.stage_color_hex(0), "");
    assert!(state.pending(Section::Performance));
    assert!(
        state.apply_blocked().is_some(),
        "an emptied colour blocks Apply"
    );
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(three_stage_colours())));
    assert_eq!(state.stage_color_hex(0), "");
}

#[test]
fn an_emptied_typed_stage_colour_stays_empty() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::StageColorHex(0, "123456".to_owned()));
    edit(&mut state, Edit::StageColorHex(0, String::new()));
    assert_eq!(state.stage_color_hex(0), "");
}

#[test]
fn an_emptied_effect_colour_stays_empty_and_blocks_apply() {
    let snap = Snapshot {
        effect: Some(Effect::Solid {
            color: rgb("ff0000"),
        }),
        ..snapshot()
    };
    let mut state = ready(snap);
    state.update(Msg::Select(Section::Lighting));
    edit(&mut state, Edit::EffectColorHex(0, String::new()));
    assert_eq!(state.effect_color_hex(0), "");
    assert!(state.pending(Section::Lighting));
    assert!(
        state.apply_blocked().is_some(),
        "an emptied colour blocks Apply"
    );
}

#[test]
fn a_stage_colour_typed_in_capitals_ends_like_its_value() {
    let mut state = ready(three_stage_colours());
    edit(&mut state, Edit::StageColorHex(0, "FF0000".to_owned()));
    assert!(!state.pending(Section::Performance));
    // nothing pins the unit: an outside change to another stage shows through
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        dpi_colors: Some(vec![rgb("ff0000"), rgb("111111"), rgb("0000ff")]),
        ..three_stage_colours()
    })));
    assert_eq!(state.stage_color_hex(1), "111111");
}

#[test]
fn an_effect_colour_typed_in_capitals_ends_like_its_value() {
    let snap = Snapshot {
        effect: Some(Effect::Solid {
            color: rgb("ff0000"),
        }),
        ..snapshot()
    };
    let mut state = ready(snap);
    state.update(Msg::Select(Section::Lighting));
    edit(&mut state, Edit::EffectColorHex(0, "FF0000".to_owned()));
    assert!(!state.pending(Section::Lighting));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        effect: Some(Effect::Solid {
            color: rgb("111111"),
        }),
        ..snapshot()
    })));
    assert_eq!(state.effect_color_hex(0), "111111");
}

#[test]
fn a_profile_pick_answered_asleep_is_retried_on_the_next_tick() {
    let mut state = ready(snapshot());
    assert_eq!(
        state.update(Msg::PickProfile(profile(3))),
        vec![Request::Load(Some(profile(3)))]
    );
    state.update(Msg::Loaded(Err(LoadError::Asleep)));
    assert_eq!(
        state.update(Msg::Tick),
        vec![Request::Load(Some(profile(3)))]
    );
}

#[test]
fn a_queued_switch_answered_by_no_mouse_is_retried_on_the_next_tick() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::PickProfile(profile(3)));
    assert_eq!(state.update(Msg::Tick).len(), 1);
    state.update(Msg::Discard);
    assert_eq!(
        state.update(Msg::Loaded(Ok(snapshot()))),
        vec![Request::Load(Some(profile(3)))]
    );
    state.update(Msg::Loaded(Err(LoadError::NoMouse)));
    assert_eq!(
        state.update(Msg::Tick),
        vec![Request::Load(Some(profile(3)))]
    );
}

#[test]
fn a_scroll_write_ends_scroll_wheel_bind_entries_it_wrote() {
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Buttons));
    // invert writes scroll-up = mouse:scroll-down and scroll-down = mouse:scroll-up
    edit(
        &mut state,
        Edit::Bind(Button::ScrollUp, Binding::Mouse(MouseFn::ScrollDown)),
    );
    edit(&mut state, Edit::Scroll(ScrollDirection::Invert));
    assert_eq!(
        state.plan(Section::Buttons),
        Ok(vec![
            WriteOp::Scroll(ScrollDirection::Invert),
            WriteOp::Bind(Button::ScrollUp, Binding::Mouse(MouseFn::ScrollDown)),
        ])
    );
    let inverted = |up: MouseFn| {
        let mut snap = snapshot();
        snap.scroll = Some(ScrollDirection::Invert);
        snap.binds[5] = (Button::ScrollUp, Some(Binding::Mouse(up)));
        snap.binds[6] = (Button::ScrollDown, Some(Binding::Mouse(MouseFn::ScrollUp)));
        snap
    };
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(inverted(MouseFn::ScrollDown)),
    });
    assert!(!state.pending(Section::Buttons));
    // a later outside rebind of scroll-up shows through
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(inverted(MouseFn::Back))));
    assert_eq!(
        state.bind(Button::ScrollUp),
        Some(Binding::Mouse(MouseFn::Back))
    );
}

#[test]
fn an_import_of_the_scroll_direction_ends_an_equal_scroll_wheel_bind_entry() {
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Buttons));
    edit(
        &mut state,
        Edit::Bind(Button::ScrollUp, Binding::Mouse(MouseFn::ScrollDown)),
    );
    let inverted = |up: MouseFn| {
        let mut snap = snapshot();
        snap.scroll = Some(ScrollDirection::Invert);
        snap.binds[5] = (Button::ScrollUp, Some(Binding::Mouse(up)));
        snap.binds[6] = (Button::ScrollDown, Some(Binding::Mouse(MouseFn::ScrollUp)));
        snap
    };
    import_landed(
        &mut state,
        vec![WriteOp::Scroll(ScrollDirection::Invert)],
        Ok(inverted(MouseFn::ScrollDown)),
    );
    assert!(!state.pending(Section::Buttons));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(inverted(MouseFn::Back))));
    assert_eq!(
        state.bind(Button::ScrollUp),
        Some(Binding::Mouse(MouseFn::Back))
    );
}

// --- the snapshot follows landed writes (review round 8) --------------------------

#[test]
fn a_landed_write_whose_reread_failed_shows_its_value_at_once() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Err(transport_error()),
    });
    assert_eq!(state.debounce(), Some(4));
    assert!(!state.pending(Section::Performance));
}

#[test]
fn revert_after_a_landed_write_shows_the_written_value() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    // the user types on while the write runs
    edit(&mut state, Edit::Debounce(6));
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Err(transport_error()),
    });
    assert_eq!(state.debounce(), Some(6));
    assert!(state.pending(Section::Performance));
    state.update(Msg::Revert);
    // the mouse holds 4 now, not the 10 of the last read
    assert_eq!(state.debounce(), Some(4));
    assert!(!state.pending(Section::Performance));
}

#[test]
fn an_edit_to_the_pre_write_value_after_a_landed_write_stays_an_edit() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Err(transport_error()),
    });
    state.update(Msg::Revert);
    edit(&mut state, Edit::Debounce(7));
    edit(&mut state, Edit::Debounce(10));
    assert!(state.pending(Section::Performance));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: debounce(4),
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(10));
    assert!(state.pending(Section::Performance));
}

#[test]
fn an_import_with_a_failed_reread_shows_the_sleep_it_wrote() {
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Device));
    import_landed(
        &mut state,
        vec![WriteOp::Sleep(SleepDelay::Seconds(300))],
        Err(transport_error()),
    );
    assert_eq!(state.sleep_minutes(), Some("5".to_owned()));
    state.update(Msg::Select(Section::Performance));
    // back to the 2 the last read showed: an edit, since the mouse holds 5m
    edit(&mut state, Edit::SleepMinutes("2".to_owned()));
    assert!(state.pending(Section::Performance));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        sleep: SleepDelay::Seconds(300),
        ..snapshot()
    })));
    assert_eq!(state.sleep_minutes(), Some("2".to_owned()));
    assert!(state.pending(Section::Performance));
}

#[test]
fn an_import_with_a_failed_reread_shows_the_stages_it_wrote() {
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Device));
    import_landed(
        &mut state,
        vec![WriteOp::Dpi(stages(&[500, 800, 1600]))],
        Err(transport_error()),
    );
    assert_eq!(
        state.stage_texts(),
        Some(vec!["500".to_owned(), "800".to_owned(), "1600".to_owned()])
    );
    state.update(Msg::Select(Section::Performance));
    edit(&mut state, Edit::DpiText(0, "400".to_owned()));
    assert!(state.pending(Section::Performance));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        dpi: stages(&[500, 800, 1600]),
        ..snapshot()
    })));
    assert_eq!(
        state.stage_texts(),
        Some(vec!["400".to_owned(), "800".to_owned(), "1600".to_owned()])
    );
}

#[test]
fn a_landed_effect_whose_reread_failed_shows_as_set_by_gctl() {
    let mut state = ready(snapshot());
    state.update(Msg::Select(Section::Lighting));
    edit(&mut state, Edit::Effect(EffectKind::Wave));
    let ops = state.plan(Section::Lighting).unwrap();
    let [WriteOp::Effect(wave)] = ops.as_slice() else {
        panic!("one effect write: {ops:?}");
    };
    let wave = wave.clone();
    assert_eq!(state.update(Msg::Apply).len(), 1);
    // the memory file the write saved, re-loaded by the answer
    let mut memory = Memory::default();
    memory.profile(MODELS[0].name, profile(2)).effect = Some(wave.clone());
    state.update(Msg::Applied {
        error: None,
        remembered: Some(Ok(memory)),
        snapshot: Err(transport_error()),
    });
    assert!(!state.pending(Section::Lighting));
    assert_eq!(state.effect_kind(), Some(EffectKind::Wave));
    // the value is the snapshot's, so it carries the `set by gctl` mark
    assert_eq!(state.snapshot().unwrap().effect, Some(wave));
}

#[test]
fn a_failed_apply_ends_the_entries_its_landed_ops_carried() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: vec![WriteOp::PollingRate(PollingRate::Hz500)],
            reached: true,
        }),
        remembered: None,
        snapshot: Err(transport_error()),
    });
    assert_eq!(state.polling_rate(), Some(PollingRate::Hz500));
    assert_eq!(state.debounce(), Some(4));
    assert!(
        state.pending(Section::Performance),
        "the debounce never landed"
    );
    // polling has no entry left: an outside change shows through
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz1000,
        ..snapshot()
    })));
    assert_eq!(state.polling_rate(), Some(PollingRate::Hz1000));
    assert_eq!(state.debounce(), Some(4));
}

#[test]
fn a_failed_apply_with_a_good_reread_ends_the_entries_its_landed_ops_carried() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: vec![WriteOp::PollingRate(PollingRate::Hz500)],
            reached: true,
        }),
        remembered: None,
        snapshot: Ok(Snapshot {
            polling_rate: PollingRate::Hz500,
            ..snapshot()
        }),
    });
    assert_eq!(state.debounce(), Some(4));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz1000,
        ..snapshot()
    })));
    assert_eq!(state.polling_rate(), Some(PollingRate::Hz1000));
}

#[test]
fn a_failed_import_ends_the_entries_its_landed_ops_carried() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    assert_eq!(state.update(Msg::Import(import_path())).len(), 1);
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: vec![
            WriteOp::PollingRate(PollingRate::Hz500),
            WriteOp::Debounce(debounce(4)),
        ],
    })));
    state.update(Msg::Imported {
        error: Some("HID transport failed: gone".to_owned()),
        reached: true,
        written: vec![WriteOp::PollingRate(PollingRate::Hz500)],
        remembered: None,
        snapshot: Some(Err(transport_error())),
    });
    assert_eq!(state.polling_rate(), Some(PollingRate::Hz500));
    assert!(!state.pending(Section::Performance));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz1000,
        ..snapshot()
    })));
    assert_eq!(state.polling_rate(), Some(PollingRate::Hz1000));
}

// --- a pick switches the window at once (review round 8) --------------------------

#[test]
fn a_pick_switches_the_edited_profile_at_once() {
    let mut state = ready(snapshot());
    assert_eq!(
        state.update(Msg::PickProfile(profile(3))),
        vec![Request::Load(Some(profile(3)))]
    );
    assert_eq!(state.edit_profile(), Some(profile(3)));
    // nothing of profile 2 shows while profile 3 loads
    assert_eq!(state.debounce(), None);
    assert_eq!(state.stage_texts(), None);
}

#[test]
fn a_pick_answered_asleep_shows_no_values_and_takes_no_edits() {
    let mut state = ready(snapshot());
    state.update(Msg::PickProfile(profile(3)));
    state.update(Msg::Loaded(Err(LoadError::Asleep)));
    assert_eq!(state.edit_profile(), Some(profile(3)));
    assert_eq!(state.debounce(), None);
    edit(&mut state, Edit::Debounce(4));
    assert!(!state.pending(Section::Performance));
    assert_eq!(
        state.update(Msg::Tick),
        vec![Request::Load(Some(profile(3)))]
    );
    state.update(Msg::Loaded(Ok(Snapshot {
        profile: profile(3),
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(10));
    assert!(!state.pending(Section::Performance));
}

#[test]
fn picking_back_after_a_failed_pick_loads_the_profile_picked_last() {
    let mut state = ready(snapshot());
    state.update(Msg::PickProfile(profile(3)));
    state.update(Msg::Loaded(Err(LoadError::Asleep)));
    assert_eq!(
        state.update(Msg::PickProfile(profile(2))),
        vec![Request::Load(Some(profile(2)))]
    );
    assert_eq!(state.edit_profile(), Some(profile(2)));
}

#[test]
fn a_read_of_the_profile_left_behind_never_shows() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::PickProfile(profile(3)));
    assert_eq!(state.update(Msg::Tick).len(), 1);
    state.update(Msg::Discard);
    assert_eq!(state.edit_profile(), Some(profile(3)));
    // the read that was running answers for profile 2
    assert_eq!(
        state.update(Msg::Loaded(Ok(snapshot()))),
        vec![Request::Load(Some(profile(3)))]
    );
    assert_eq!(state.edit_profile(), Some(profile(3)));
    assert_eq!(state.debounce(), None);
}

// --- emptied sleep fields (review round 8) ----------------------------------------

#[test]
fn an_emptied_sleep_minutes_field_blocks_apply() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::SleepSeconds("30".to_owned()));
    edit(&mut state, Edit::SleepMinutes(String::new()));
    assert_eq!(state.sleep_minutes(), Some(String::new()));
    assert!(state.pending(Section::Performance));
    assert!(state.apply_blocked().is_some());
    assert!(state.plan(Section::Performance).is_err());
}

#[test]
fn an_emptied_sleep_seconds_field_is_pending_and_blocks_apply() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::SleepSeconds(String::new()));
    assert_eq!(state.sleep_seconds(), Some(String::new()));
    assert!(state.pending(Section::Performance));
    assert!(state.apply_blocked().is_some());
    assert!(state.plan(Section::Performance).is_err());
}

#[test]
fn unticking_never_fills_the_sleep_fields_with_zero() {
    let mut state = ready(Snapshot {
        sleep: SleepDelay::Never,
        ..snapshot()
    });
    edit(&mut state, Edit::SleepNever(false));
    assert_eq!(state.sleep_minutes(), Some("0".to_owned()));
    assert_eq!(state.sleep_seconds(), Some("0".to_owned()));
    edit(&mut state, Edit::SleepMinutes("2".to_owned()));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::Sleep(SleepDelay::Seconds(120))])
    );
}

// --- the write-only half comes from the memory file (review round 9) -------------

fn invert_binds(memory: &mut Memory) {
    for id in 1..=3 {
        let binds = &mut memory.profile(MODELS[0].name, profile(id)).binds;
        binds.insert("scroll-up".to_owned(), Binding::Mouse(MouseFn::ScrollDown));
        binds.insert("scroll-down".to_owned(), Binding::Mouse(MouseFn::ScrollUp));
    }
}

#[test]
fn a_failed_effect_write_whose_reread_failed_shows_what_memory_holds() {
    let wave = Effect::Wave { rate: rate(40) };
    let mut state = ready(Snapshot {
        effect: Some(wave),
        ..snapshot()
    });
    state.update(Msg::Select(Section::Lighting));
    edit(&mut state, Edit::Effect(EffectKind::Off));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    // the failed write cleared the remembered effect
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: Some(Ok(Memory::default())),
        snapshot: Err(transport_error()),
    });
    assert_eq!(state.snapshot().unwrap().effect, None);
    assert!(state.pending(Section::Lighting), "the user's Off stays");
}

#[test]
fn a_landed_scroll_bind_whose_reread_failed_shows_the_scroll_memory_derives() {
    let mut snap = snapshot();
    snap.scroll = Some(ScrollDirection::Invert);
    snap.binds[5] = (Button::ScrollUp, Some(Binding::Mouse(MouseFn::ScrollDown)));
    snap.binds[6] = (Button::ScrollDown, Some(Binding::Mouse(MouseFn::ScrollUp)));
    let mut state = ready(snap);
    state.update(Msg::Select(Section::Buttons));
    edit(
        &mut state,
        Edit::Bind(Button::ScrollUp, Binding::Mouse(MouseFn::Back)),
    );
    assert_eq!(state.update(Msg::Apply).len(), 1);
    let mut memory = Memory::default();
    invert_binds(&mut memory);
    memory
        .profile(MODELS[0].name, profile(2))
        .binds
        .insert("scroll-up".to_owned(), Binding::Mouse(MouseFn::Back));
    state.update(Msg::Applied {
        error: None,
        remembered: Some(Ok(memory)),
        snapshot: Err(transport_error()),
    });
    assert!(!state.pending(Section::Buttons));
    assert_eq!(
        state.bind(Button::ScrollUp),
        Some(Binding::Mouse(MouseFn::Back))
    );
    // profile 2 no longer matches invert, so the direction is unknown
    assert_eq!(state.snapshot().unwrap().scroll, None);
}

#[test]
fn a_failed_stage_count_change_whose_reread_failed_drops_the_colours() {
    let mut state = ready(Snapshot {
        dpi_colors: Some(vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")]),
        ..snapshot()
    });
    edit(&mut state, Edit::RemoveStage(2));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: Some(Ok(Memory::default())),
        snapshot: Err(transport_error()),
    });
    assert_eq!(state.snapshot().unwrap().dpi_colors, None);
}

#[test]
fn an_unreadable_memory_after_a_write_reads_write_only_values_as_unknown() {
    let mut state = ready(Snapshot {
        effect: Some(Effect::Wave { rate: rate(40) }),
        ..snapshot()
    });
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    let message = "cannot read remembered values from \"/m.toml\": bad; fix or delete the file";
    state.update(Msg::Applied {
        error: None,
        remembered: Some(Err(message.to_owned())),
        snapshot: Err(transport_error()),
    });
    let snap = state.snapshot().unwrap();
    assert_eq!(snap.effect, None);
    assert_eq!(snap.memory_error.as_deref(), Some(message));
    assert_eq!(state.debounce(), Some(4));
}

// --- a write's answer compares every field its plan carried (review round 9) ------

#[test]
fn an_edit_back_during_a_write_whose_op_failed_ends_at_the_answer() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    // back to the mouse's 10 while the write runs
    edit(&mut state, Edit::Debounce(10));
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: vec![WriteOp::PollingRate(PollingRate::Hz500)],
            reached: true,
        }),
        remembered: None,
        snapshot: Err(transport_error()),
    });
    assert!(!state.pending(Section::Performance));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz500,
        debounce: debounce(7),
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(7));
}

#[test]
fn an_edit_back_during_an_import_whose_op_failed_ends_at_the_answer() {
    let mut state = ready(snapshot());
    assert_eq!(state.update(Msg::Import(import_path())).len(), 1);
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: vec![
            WriteOp::PollingRate(PollingRate::Hz500),
            WriteOp::Debounce(debounce(4)),
        ],
    })));
    edit(&mut state, Edit::Debounce(7));
    edit(&mut state, Edit::Debounce(10));
    state.update(Msg::Imported {
        error: Some("HID transport failed: gone".to_owned()),
        reached: true,
        written: vec![WriteOp::PollingRate(PollingRate::Hz500)],
        remembered: None,
        snapshot: Some(Err(transport_error())),
    });
    assert!(!state.pending(Section::Performance));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz500,
        debounce: debounce(7),
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(7));
}

#[test]
fn a_failed_write_whose_reread_shows_its_value_ends_the_entry() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    // the write reported a failure, but the mouse took the value
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: None,
        snapshot: Ok(Snapshot {
            debounce: debounce(4),
            ..snapshot()
        }),
    });
    assert!(!state.pending(Section::Performance));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: debounce(7),
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(7));
}

// --- picks and questions (review round 9) ----------------------------------------

#[test]
fn a_pick_during_a_background_read_switches_at_once() {
    let mut state = ready(snapshot());
    assert_eq!(state.update(Msg::Tick).len(), 1);
    assert!(state.update(Msg::PickProfile(profile(3))).is_empty());
    assert_eq!(state.edit_profile(), Some(profile(3)));
    // the read that was running answers for profile 2
    assert_eq!(
        state.update(Msg::Loaded(Ok(snapshot()))),
        vec![Request::Load(Some(profile(3)))]
    );
    assert_eq!(state.debounce(), None);
}

#[test]
fn make_active_waits_for_a_read_of_the_picked_profile() {
    let mut state = ready(snapshot());
    state.update(Msg::PickProfile(profile(3)));
    assert!(state.update(Msg::MakeActive).is_empty());
    state.update(Msg::Loaded(Err(LoadError::Asleep)));
    assert!(state.update(Msg::MakeActive).is_empty());
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        profile: profile(3),
        ..snapshot()
    })));
    assert_eq!(
        state.update(Msg::MakeActive),
        vec![Request::MakeActive(profile(3))]
    );
}

#[test]
fn nothing_starts_while_a_question_is_open() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::PickProfile(profile(3)));
    assert_eq!(
        state.prompt(),
        Some(Prompt::Discard(Leave::Profile(profile(3))))
    );
    assert!(state.update(Msg::Apply).is_empty());
    assert!(state.update(Msg::MakeActive).is_empty());
    assert!(
        state
            .update(Msg::Export(PathBuf::from("/x/p.toml")))
            .is_empty()
    );
    assert!(state.update(Msg::Import(import_path())).is_empty());
    assert_eq!(
        state.prompt(),
        Some(Prompt::Discard(Leave::Profile(profile(3))))
    );
}

// --- what may start, decided in one place (review round 10) -----------------------

#[test]
fn a_pick_during_a_write_is_refused_even_with_pending_edits() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    assert!(state.update(Msg::PickProfile(profile(3))).is_empty());
    assert_eq!(state.prompt(), None);
    assert_eq!(state.edit_profile(), Some(profile(2)));
}

#[test]
fn a_pick_during_an_import_is_refused_even_with_pending_edits() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    assert_eq!(state.update(Msg::Import(import_path())).len(), 1);
    assert_eq!(
        state
            .update(Msg::ImportChecked(Ok(ImportCheck {
                note: None,
                device_wide: false,
                writes: vec![WriteOp::Debounce(debounce(3))],
            })))
            .len(),
        1
    );
    assert!(state.update(Msg::PickProfile(profile(3))).is_empty());
    assert_eq!(state.prompt(), None);
    state.update(Msg::Imported {
        error: None,
        reached: false,
        written: vec![WriteOp::Debounce(debounce(3))],
        remembered: None,
        snapshot: Some(Ok(Snapshot {
            debounce: debounce(3),
            ..snapshot()
        })),
    });
    assert_eq!(
        state.notice(),
        Some(&Notice::Done("imported into profile 2".to_owned()))
    );
}

#[test]
fn a_pick_during_an_export_is_refused_even_with_pending_edits() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(
        state.update(Msg::Export(PathBuf::from("/x/p.toml"))).len(),
        1
    );
    assert!(state.update(Msg::PickProfile(profile(3))).is_empty());
    assert_eq!(state.prompt(), None);
    assert_eq!(state.edit_profile(), Some(profile(2)));
}

#[test]
fn a_plain_import_checked_under_a_question_runs_without_asking_once_it_closes() {
    let mut state = ready(snapshot());
    assert_eq!(state.update(Msg::Import(import_path())).len(), 1);
    edit(&mut state, Edit::Debounce(4));
    state.update(Msg::Select(Section::Lighting));
    assert!(state.prompt().is_some());
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: vec![WriteOp::PollingRate(PollingRate::Hz500)],
    })));
    let run = state.update(Msg::KeepEditing);
    assert_eq!(state.prompt(), None, "a plain import asks nothing");
    assert_eq!(run.len(), 1, "the import runs: {run:?}");
}

#[test]
fn an_edit_back_during_an_import_that_never_reached_the_mouse_ends_at_the_answer() {
    let mut state = ready(snapshot());
    assert_eq!(state.update(Msg::Import(import_path())).len(), 1);
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: vec![WriteOp::Debounce(debounce(3))],
    })));
    edit(&mut state, Edit::Debounce(5));
    edit(&mut state, Edit::Debounce(10));
    state.update(Msg::Imported {
        error: Some("no Glorious mouse found".to_owned()),
        reached: false,
        written: Vec::new(),
        remembered: None,
        snapshot: None,
    });
    assert!(!state.pending(Section::Performance));
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: debounce(7),
        ..snapshot()
    })));
    assert_eq!(state.debounce(), Some(7));
}

#[test]
fn sleep_fields_take_no_text_while_never_is_ticked() {
    let mut state = ready(Snapshot {
        sleep: SleepDelay::Never,
        ..snapshot()
    });
    edit(&mut state, Edit::SleepMinutes("5".to_owned()));
    assert!(!state.pending(Section::Performance));
    assert_eq!(state.sleep_minutes(), Some(String::new()));
    edit(&mut state, Edit::SleepNever(false));
    assert_eq!(state.sleep_minutes(), Some("0".to_owned()));
}

#[test]
fn text_typed_after_ticking_never_is_dropped() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::SleepNever(true));
    edit(&mut state, Edit::SleepMinutes("5".to_owned()));
    edit(&mut state, Edit::SleepSeconds("9".to_owned()));
    edit(&mut state, Edit::SleepNever(false));
    // back to the 2m0s the mouse holds, nothing of the dropped text
    assert_eq!(state.sleep_minutes(), Some("2".to_owned()));
    assert_eq!(state.sleep_seconds(), Some("0".to_owned()));
    assert!(!state.pending(Section::Performance));
}

// --- other writers (gui.md §Behaviour "other writers") ----------------------
// B: a value the mouse moved under a pending entry is a footer line in its own
//    slot beside the notice; the entry stays.
// C: an unpicked active stage follows the latest read into Apply.
// D: a readable value no write of this window explains raises the banner.

fn reload(state: &mut State, snap: Snapshot) {
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(snap)));
}

fn lines(items: &[&str]) -> Vec<String> {
    items.iter().map(|line| (*line).to_owned()).collect()
}

#[test]
fn an_outside_change_under_an_edit_keeps_the_edit_and_says_so() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    assert_eq!(
        state.changed(),
        lines(&["polling rate changed on the mouse to 250 Hz; your edit is kept"])
    );
    assert_eq!(state.polling_rate(), Some(PollingRate::Hz500));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::PollingRate(PollingRate::Hz500)])
    );
    // a later read that moves nothing leaves the notice, like any notice
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    assert_eq!(
        state.changed(),
        lines(&["polling rate changed on the mouse to 250 Hz; your edit is kept"])
    );
}

#[test]
fn every_moved_edited_field_gets_its_own_line_in_row_order() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    edit(&mut state, Edit::SleepMinutes("5".to_owned()));
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    edit(&mut state, Edit::DpiText(0, "500".to_owned()));
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz250,
            sleep: SleepDelay::Seconds(90),
            debounce: Debounce::new(7).unwrap(),
            dpi: stages(&[400, 800, 3200]),
            ..snapshot()
        },
    );
    assert_eq!(
        state.changed(),
        lines(&[
            "DPI stages changed on the mouse to 400 800 3200; your edit is kept",
            "polling rate changed on the mouse to 250 Hz; your edit is kept",
            "debounce changed on the mouse to 7 ms; your edit is kept",
            "sleep changed on the mouse to 1m30s; your edit is kept",
        ])
    );
    assert_eq!(
        state.stage_texts(),
        Some(vec!["500".to_owned(), "800".to_owned(), "1600".to_owned()])
    );
    assert_eq!(state.debounce(), Some(4));
}

#[test]
fn a_lift_off_moved_under_its_edit_names_its_millimetres() {
    let mut state = ready(Snapshot {
        lift_off: LiftOff::Mm2,
        ..snapshot()
    });
    edit(&mut state, Edit::LiftOff(LiftOff::Mm1));
    edit(&mut state, Edit::SleepNever(true));
    reload(
        &mut state,
        Snapshot {
            lift_off: LiftOff::Mm1,
            sleep: SleepDelay::Never,
            ..snapshot()
        },
    );
    assert_eq!(
        state.changed(),
        lines(&[
            "lift-off changed on the mouse to 1 mm; your edit is kept",
            "sleep changed on the mouse to never; your edit is kept",
        ])
    );
}

#[test]
fn the_mouse_moving_a_picked_active_stage_says_so() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::ActiveStage(3));
    reload(
        &mut state,
        Snapshot {
            dpi_stage: DpiStage::new(1).unwrap(),
            ..snapshot()
        },
    );
    assert_eq!(
        state.changed(),
        lines(&["active stage changed on the mouse to 1; your edit is kept"])
    );
    assert_eq!(state.active_stage(), Some(3));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![WriteOp::DpiStage(DpiStage::new(3).unwrap())])
    );
}

#[test]
fn a_moved_field_nobody_edited_is_no_notice() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    assert!(state.changed().is_empty());
}

#[test]
fn a_read_that_moves_nothing_under_an_edit_is_no_notice() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    reload(&mut state, snapshot());
    assert!(state.changed().is_empty());
}

#[test]
fn a_write_answer_showing_its_own_value_is_no_notice() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(Snapshot {
            polling_rate: PollingRate::Hz500,
            debounce: Debounce::new(4).unwrap(),
            ..snapshot()
        }),
    });
    assert!(state.changed().is_empty());
    assert!(!state.pending(Section::Performance));
}

#[test]
fn the_dpi_button_under_an_unpicked_stage_is_no_notice() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::DpiText(0, "500".to_owned()));
    reload(
        &mut state,
        Snapshot {
            dpi_stage: DpiStage::new(3).unwrap(),
            ..snapshot()
        },
    );
    assert!(state.changed().is_empty());
    assert_eq!(state.active_stage(), Some(3));
}

#[test]
fn a_stage_list_apply_writes_the_stage_the_dpi_button_picked() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::DpiText(0, "500".to_owned()));
    reload(
        &mut state,
        Snapshot {
            dpi_stage: DpiStage::new(3).unwrap(),
            ..snapshot()
        },
    );
    assert_eq!(
        state.update(Msg::Apply),
        vec![Request::Apply {
            profile: profile(2),
            ops: vec![
                WriteOp::Dpi(stages(&[500, 800, 1600])),
                WriteOp::DpiStage(DpiStage::new(3).unwrap()),
            ],
        }]
    );
}

#[test]
fn a_colour_apply_keeps_the_stage_the_dpi_button_picked() {
    let mut state = ready(Snapshot {
        dpi_colors: Some(vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")]),
        ..snapshot()
    });
    edit(&mut state, Edit::StageColor(0, rgb("123456")));
    reload(
        &mut state,
        Snapshot {
            dpi_stage: DpiStage::new(3).unwrap(),
            dpi_colors: Some(vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")]),
            ..snapshot()
        },
    );
    assert_eq!(state.active_stage(), Some(3));
    // the colours alone: no stage write, so the mouse stays on stage 3
    assert_eq!(
        state.update(Msg::Apply),
        vec![Request::Apply {
            profile: profile(2),
            ops: vec![WriteOp::DpiColors(vec![
                rgb("123456"),
                rgb("00ff00"),
                rgb("0000ff"),
            ])],
        }]
    );
}

#[test]
fn an_unpicked_stage_follows_the_mouse_after_the_list_shrinks() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::RemoveStage(0));
    // the mouse's stage 2 still fits the two-stage entry
    assert_eq!(state.active_stage(), Some(2));
    reload(
        &mut state,
        Snapshot {
            dpi_stage: DpiStage::new(1).unwrap(),
            ..snapshot()
        },
    );
    assert_eq!(state.active_stage(), Some(1));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![
            WriteOp::Dpi(stages(&[800, 1600])),
            WriteOp::DpiStage(DpiStage::new(1).unwrap()),
        ])
    );
}

#[test]
fn a_readable_value_another_program_moved_raises_the_banner() {
    let cases = [
        Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
        Snapshot {
            lift_off: LiftOff::Mm2,
            ..snapshot()
        },
        Snapshot {
            sleep: SleepDelay::Never,
            ..snapshot()
        },
        Snapshot {
            firmware: Firmware([0, 3, 9, 0]),
            ..snapshot()
        },
        Snapshot {
            dpi: stages(&[400, 800, 3200]),
            ..snapshot()
        },
        Snapshot {
            debounce: Debounce::new(7).unwrap(),
            ..snapshot()
        },
    ];
    for case in cases {
        let mut state = ready(snapshot());
        assert!(!state.outside_change(), "the first read compares nothing");
        reload(&mut state, case.clone());
        assert!(state.outside_change(), "{case:?}");
    }
}

#[test]
fn the_mouse_own_buttons_and_the_battery_never_raise_the_banner() {
    let mut state = ready(snapshot());
    reload(
        &mut state,
        Snapshot {
            dpi_stage: DpiStage::new(3).unwrap(),
            active: profile(1),
            battery: Battery {
                state: DeviceState::Awake,
                percent: Some(20),
            },
            ..snapshot()
        },
    );
    assert!(!state.outside_change());
}

#[test]
fn this_windows_own_write_never_raises_the_banner() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    edit(&mut state, Edit::DpiText(0, "500".to_owned()));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(Snapshot {
            polling_rate: PollingRate::Hz500,
            dpi: stages(&[500, 800, 1600]),
            ..snapshot()
        }),
    });
    assert!(!state.outside_change());
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz500,
            dpi: stages(&[500, 800, 1600]),
            ..snapshot()
        },
    );
    assert!(!state.outside_change());
}

#[test]
fn a_write_answer_showing_another_change_raises_the_banner() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(Snapshot {
            polling_rate: PollingRate::Hz500,
            lift_off: LiftOff::Mm2,
            ..snapshot()
        }),
    });
    assert!(state.outside_change());
}

#[test]
fn a_write_whose_reread_failed_is_explained_at_the_next_read() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Err(LoadError::Transport("gone".to_owned())),
    });
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz500,
            ..snapshot()
        },
    );
    assert!(!state.outside_change());
}

#[test]
fn a_partial_write_explains_the_ops_that_landed() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: vec![WriteOp::PollingRate(PollingRate::Hz500)],
            reached: true,
        }),
        remembered: None,
        snapshot: Ok(Snapshot {
            polling_rate: PollingRate::Hz500,
            ..snapshot()
        }),
    });
    assert!(!state.outside_change());
}

#[test]
fn an_import_never_raises_the_banner_for_what_it_wrote() {
    let mut state = ready(snapshot());
    state.update(Msg::Import(import_path()));
    let writes = vec![WriteOp::Debounce(Debounce::new(4).unwrap())];
    assert_eq!(
        state
            .update(Msg::ImportChecked(Ok(ImportCheck {
                note: None,
                device_wide: false,
                writes: writes.clone(),
            })))
            .len(),
        1
    );
    state.update(Msg::Imported {
        error: None,
        reached: false,
        written: writes,
        remembered: None,
        snapshot: Some(Ok(Snapshot {
            debounce: Debounce::new(4).unwrap(),
            ..snapshot()
        })),
    });
    assert!(!state.outside_change());
}

#[test]
fn a_change_made_while_the_mouse_was_away_raises_the_banner() {
    let mut state = ready(snapshot());
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Err(LoadError::NoMouse)));
    reload(
        &mut state,
        Snapshot {
            lift_off: LiftOff::Mm2,
            ..snapshot()
        },
    );
    assert!(state.outside_change());
}

#[test]
fn another_profile_compares_only_the_device_wide_values() {
    let mut state = ready(snapshot());
    assert_eq!(
        state.update(Msg::PickProfile(profile(1))),
        vec![Request::Load(Some(profile(1)))]
    );
    state.update(Msg::Loaded(Ok(Snapshot {
        profile: profile(1),
        dpi: stages(&[1200]),
        dpi_stage: DpiStage::new(1).unwrap(),
        debounce: Debounce::new(4).unwrap(),
        ..snapshot()
    })));
    assert!(!state.outside_change());

    let mut state = ready(snapshot());
    state.update(Msg::PickProfile(profile(1)));
    state.update(Msg::Loaded(Ok(Snapshot {
        profile: profile(1),
        polling_rate: PollingRate::Hz250,
        ..snapshot()
    })));
    assert!(state.outside_change());
}

#[test]
fn another_model_never_raises_the_banner() {
    let mut state = ready(snapshot());
    reload(
        &mut state,
        Snapshot {
            model: Model::from_pid(0x2012).unwrap(),
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    assert!(!state.outside_change());
}

#[test]
fn dismiss_clears_the_banner_until_the_next_unexplained_change() {
    let mut state = ready(snapshot());
    let moved_rate = Snapshot {
        polling_rate: PollingRate::Hz250,
        ..snapshot()
    };
    reload(&mut state, moved_rate.clone());
    assert!(state.outside_change());
    assert!(state.update(Msg::DismissOutsideChange).is_empty());
    assert!(!state.outside_change());
    reload(&mut state, moved_rate);
    assert!(!state.outside_change(), "the 250 Hz was already seen");
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz125,
            ..snapshot()
        },
    );
    assert!(state.outside_change());
}

#[test]
fn the_banner_stays_through_a_reconnect_writes_and_reads() {
    let mut state = ready(snapshot());
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Err(LoadError::NoMouse)));
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    assert!(state.outside_change());
    state.update(Msg::Select(Section::Lighting));
    edit(&mut state, Edit::Effect(EffectKind::Wave));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        }),
    });
    assert!(state.outside_change(), "only Dismiss clears it");
}

// --- other writers, review round 1 ----------------------------------------------

#[test]
fn a_failed_import_keeps_its_notice_beside_the_moved_lines() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    state.update(Msg::Import(import_path()));
    assert_eq!(
        state
            .update(Msg::ImportChecked(Ok(ImportCheck {
                note: None,
                device_wide: false,
                writes: vec![WriteOp::Debounce(Debounce::new(4).unwrap())],
            })))
            .len(),
        1
    );
    state.update(Msg::Imported {
        error: Some("HID transport failed: gone".to_owned()),
        reached: true,
        written: Vec::new(),
        remembered: None,
        snapshot: Some(Ok(Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        })),
    });
    assert_eq!(
        state.notice(),
        Some(&Notice::Failed("HID transport failed: gone".to_owned()))
    );
    assert_eq!(
        state.changed(),
        lines(&["polling rate changed on the mouse to 250 Hz; your edit is kept"])
    );
}

#[test]
fn a_successful_import_keeps_its_notice_beside_the_moved_lines() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    state.update(Msg::Import(import_path()));
    let writes = vec![WriteOp::Debounce(Debounce::new(4).unwrap())];
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: writes.clone(),
    })));
    state.update(Msg::Imported {
        error: None,
        reached: false,
        written: writes,
        remembered: None,
        snapshot: Some(Ok(Snapshot {
            polling_rate: PollingRate::Hz250,
            debounce: Debounce::new(4).unwrap(),
            ..snapshot()
        })),
    });
    assert_eq!(
        state.notice(),
        Some(&Notice::Done("imported into profile 2".to_owned()))
    );
    assert_eq!(
        state.changed(),
        lines(&["polling rate changed on the mouse to 250 Hz; your edit is kept"])
    );
}

#[test]
fn the_next_user_action_clears_the_moved_lines_like_a_notice() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    assert_eq!(state.changed().len(), 1);
    edit(&mut state, Edit::Debounce(4));
    assert!(state.changed().is_empty());
}

#[test]
fn a_dpi_write_that_failed_inside_the_list_owns_the_stage_reset() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::DpiText(0, "500".to_owned()));
    edit(&mut state, Edit::ActiveStage(3));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    // the failing `Dpi` op wrote active stage 1 before its list report failed
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: None,
        snapshot: Ok(Snapshot {
            dpi_stage: DpiStage::new(1).unwrap(),
            ..snapshot()
        }),
    });
    assert!(state.changed().is_empty());
    assert!(!state.outside_change());
}

#[test]
fn a_dpi_list_that_reached_the_mouse_before_its_failure_is_this_windows_own() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::DpiText(0, "500".to_owned()));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: None,
        snapshot: Ok(Snapshot {
            dpi: stages(&[500, 800, 1600]),
            dpi_stage: DpiStage::new(1).unwrap(),
            ..snapshot()
        }),
    });
    assert!(state.changed().is_empty());
    assert!(!state.outside_change());
}

#[test]
fn a_failed_write_whose_reread_failed_owns_its_fields_at_the_next_read_only() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: None,
        snapshot: Err(LoadError::Transport("gone".to_owned())),
    });
    // the failing report did reach the mouse
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz500,
            ..snapshot()
        },
    );
    assert!(!state.outside_change());
    // that read was good, so the next move is someone else's again
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    assert!(state.outside_change());
}

#[test]
fn a_failed_write_owns_only_the_fields_it_carried() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: None,
        snapshot: Ok(Snapshot {
            lift_off: LiftOff::Mm2,
            ..snapshot()
        }),
    });
    assert!(state.outside_change());
}

fn wired() -> Snapshot {
    Snapshot {
        model: Model::from_pid(0x2011).unwrap(),
        ..snapshot()
    }
}

#[test]
fn a_cable_or_dongle_switch_starts_a_fresh_baseline() {
    assert_eq!(snapshot().model.product_id, 0x2022);
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    reload(
        &mut state,
        Snapshot {
            firmware: Firmware([0, 3, 9, 0]),
            polling_rate: PollingRate::Hz250,
            ..wired()
        },
    );
    assert!(!state.outside_change());
    assert!(state.changed().is_empty());
    // the wired read is the new baseline
    reload(
        &mut state,
        Snapshot {
            firmware: Firmware([0, 3, 9, 0]),
            polling_rate: PollingRate::Hz125,
            ..wired()
        },
    );
    assert!(state.outside_change());
    assert_eq!(
        state.changed(),
        lines(&["polling rate changed on the mouse to 125 Hz; your edit is kept"])
    );
}

// --- other writers, review round 2: a moved line lives only with its entry ------

#[test]
fn a_section_discard_drops_the_lines_of_the_entries_it_drops() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    state.update(Msg::Select(Section::Lighting));
    assert!(state.prompt().is_some());
    // the background poll still reads under the discard question
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    assert_eq!(
        state.changed(),
        lines(&["polling rate changed on the mouse to 250 Hz; your edit is kept"])
    );
    state.update(Msg::Discard);
    assert_eq!(state.section(), Section::Lighting);
    assert_eq!(state.polling_rate(), Some(PollingRate::Hz250));
    assert!(state.changed().is_empty());
}

#[test]
fn a_profile_discard_drops_the_lines_of_the_entries_it_drops() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    state.update(Msg::PickProfile(profile(1)));
    assert!(state.prompt().is_some());
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    assert_eq!(state.changed().len(), 1);
    state.update(Msg::Discard);
    assert!(state.changed().is_empty());
}

#[test]
fn another_model_drops_the_lines_with_the_entries() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    assert_eq!(state.changed().len(), 1);
    reload(
        &mut state,
        Snapshot {
            model: Model::from_pid(0x2012).unwrap(),
            ..snapshot()
        },
    );
    assert!(!state.pending(Section::Performance));
    assert!(state.changed().is_empty());
}

#[test]
fn an_import_that_never_reached_the_mouse_owns_no_field() {
    let mut state = ready(snapshot());
    state.update(Msg::Import(import_path()));
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: vec![WriteOp::PollingRate(PollingRate::Hz500)],
    })));
    // `snapshot: None`: the import never opened the mouse, so wrote nothing
    state.update(Msg::Imported {
        error: Some("no Glorious mouse found".to_owned()),
        reached: false,
        written: Vec::new(),
        remembered: None,
        snapshot: None,
    });
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    assert!(state.outside_change());
}

// --- other writers, review round 3: every line ends with its own entry ----------

/// Edit, open the discard question, let the background read move the field,
/// then Discard: the line shows while the entry lives and ends with it.
fn discard_ends_the_line(edits: &[Edit], moved: Snapshot, expected: &[&str]) {
    let mut state = ready(snapshot());
    for e in edits {
        state.update(Msg::Edit(e.clone()));
    }
    state.update(Msg::Select(Section::Lighting));
    assert!(state.prompt().is_some(), "the discard question is open");
    reload(&mut state, moved);
    let want: Vec<String> = expected.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(
        state.changed(),
        want,
        "the line shows while its entry lives"
    );
    state.update(Msg::Discard);
    assert!(
        state.changed().is_empty(),
        "lines left after Discard: {:?}",
        state.changed()
    );
}

#[test]
fn the_active_stage_line_lives_with_the_dpi_entry() {
    discard_ends_the_line(
        &[Edit::ActiveStage(3)],
        Snapshot {
            dpi_stage: DpiStage::new(1).unwrap(),
            ..snapshot()
        },
        &["active stage changed on the mouse to 1; your edit is kept"],
    );
}

#[test]
fn the_dpi_list_line_lives_with_the_dpi_entry() {
    discard_ends_the_line(
        &[Edit::DpiText(0, "500".to_owned())],
        Snapshot {
            dpi: stages(&[400, 900, 1600]),
            ..snapshot()
        },
        &["DPI stages changed on the mouse to 400 900 1600; your edit is kept"],
    );
}

#[test]
fn the_lift_off_line_lives_with_its_entry() {
    discard_ends_the_line(
        &[Edit::LiftOff(LiftOff::Mm2)],
        Snapshot {
            lift_off: LiftOff::Mm2,
            ..snapshot()
        },
        &["lift-off changed on the mouse to 2 mm; your edit is kept"],
    );
}

#[test]
fn the_debounce_line_lives_with_its_entry() {
    discard_ends_the_line(
        &[Edit::Debounce(0)],
        Snapshot {
            debounce: Debounce::new(7).unwrap(),
            ..snapshot()
        },
        &["debounce changed on the mouse to 7 ms; your edit is kept"],
    );
}

#[test]
fn the_sleep_line_lives_with_its_entry() {
    discard_ends_the_line(
        &[Edit::SleepMinutes("5".to_owned())],
        Snapshot {
            sleep: SleepDelay::Seconds(60),
            ..snapshot()
        },
        &["sleep changed on the mouse to 1m; your edit is kept"],
    );
}

/// Every field at once, ended by a model change (no user message between).
#[test]
fn another_model_ends_every_line_at_once() {
    let other: &'static Model = MODELS.iter().find(|m| m.name != MODELS[0].name).unwrap();
    let mut state = ready(snapshot());
    for e in [
        Edit::DpiText(0, "500".to_owned()),
        Edit::ActiveStage(3),
        Edit::PollingRate(PollingRate::Hz500),
        Edit::LiftOff(LiftOff::Mm2),
        Edit::Debounce(0),
        Edit::SleepMinutes("5".to_owned()),
    ] {
        state.update(Msg::Edit(e));
    }
    reload(
        &mut state,
        Snapshot {
            dpi: stages(&[400, 900, 1600]),
            dpi_stage: DpiStage::new(1).unwrap(),
            polling_rate: PollingRate::Hz250,
            lift_off: LiftOff::Mm2,
            debounce: Debounce::new(7).unwrap(),
            sleep: SleepDelay::Seconds(60),
            ..snapshot()
        },
    );
    assert_eq!(state.changed().len(), 6, "{:?}", state.changed());
    reload(
        &mut state,
        Snapshot {
            model: other,
            ..snapshot()
        },
    );
    assert!(
        state.changed().is_empty(),
        "lines left: {:?}",
        state.changed()
    );
}

/// A write's answer ends the entry it carried and keeps the line of a live
/// entry it did not carry.
#[test]
fn a_write_answer_ends_its_own_entry_and_keeps_another_live_line() {
    let mut state = ready(snapshot());
    state.update(Msg::Edit(Edit::PollingRate(PollingRate::Hz500)));
    // the mouse moves to the edit's value: the entry lives, nothing pending
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz500,
            ..snapshot()
        },
    );
    assert!(!state.pending(Section::Performance));
    state.update(Msg::Edit(Edit::LiftOff(LiftOff::Mm2)));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: None,
        remembered: None,
        snapshot: Ok(Snapshot {
            polling_rate: PollingRate::Hz250,
            lift_off: LiftOff::Mm2,
            ..snapshot()
        }),
    });
    assert_eq!(
        state.changed(),
        vec!["polling rate changed on the mouse to 250 Hz; your edit is kept".to_owned()]
    );
    assert_eq!(state.lift_off(), Some(LiftOff::Mm2));
    assert!(state.outside_change());
}

/// An import that reached the mouse but whose re-read failed still owns the
/// fields its check carried until the next good read.
#[test]
fn an_import_whose_reread_failed_still_owns_its_fields() {
    let mut state = ready(snapshot());
    state.update(Msg::Import(import_path()));
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: vec![WriteOp::PollingRate(PollingRate::Hz500)],
    })));
    state.update(Msg::Imported {
        error: Some("HID transport failed: gone".to_owned()),
        reached: true,
        written: Vec::new(),
        remembered: None,
        snapshot: Some(Err(LoadError::Asleep)),
    });
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz500,
            ..snapshot()
        },
    );
    assert!(!state.outside_change());
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    assert!(state.outside_change());
}

/// The never-reached import keeps an earlier failed Apply's unsure fields: it
/// marks nothing and clears nothing.
#[test]
fn a_never_reached_import_keeps_an_earlier_failed_writes_fields() {
    let mut state = ready(snapshot());
    state.update(Msg::Edit(Edit::PollingRate(PollingRate::Hz500)));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: None,
        snapshot: Err(LoadError::Asleep),
    });
    state.update(Msg::Revert);
    state.update(Msg::Import(import_path()));
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: vec![WriteOp::Debounce(Debounce::new(4).unwrap())],
    })));
    state.update(Msg::Imported {
        error: Some("no Glorious mouse found".to_owned()),
        reached: false,
        written: Vec::new(),
        remembered: None,
        snapshot: None,
    });
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz500,
            ..snapshot()
        },
    );
    assert!(!state.outside_change());
}

/// A read that moves only a field nobody edited leaves the earlier line of a
/// live entry alone.
#[test]
fn a_move_under_no_entry_keeps_an_earlier_live_line() {
    let mut state = ready(snapshot());
    state.update(Msg::Edit(Edit::LiftOff(LiftOff::Mm2)));
    reload(
        &mut state,
        Snapshot {
            lift_off: LiftOff::Mm2,
            ..snapshot()
        },
    );
    let line = vec!["lift-off changed on the mouse to 2 mm; your edit is kept".to_owned()];
    assert_eq!(state.changed(), line);
    reload(
        &mut state,
        Snapshot {
            lift_off: LiftOff::Mm2,
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    assert_eq!(state.changed(), line);
}

/// A failed DPI write whose list report landed while the picked stage did not:
/// the entry lives (picked 3, mouse 1), and the list is this window's own.
#[test]
fn a_failed_list_write_that_landed_under_a_live_entry_is_no_line() {
    let mut state = ready(snapshot());
    state.update(Msg::Edit(Edit::DpiText(0, "500".to_owned())));
    state.update(Msg::Edit(Edit::ActiveStage(3)));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: None,
        snapshot: Ok(Snapshot {
            dpi: stages(&[500, 800, 1600]),
            dpi_stage: DpiStage::new(1).unwrap(),
            ..snapshot()
        }),
    });
    assert!(
        state.pending(Section::Performance),
        "the picked stage keeps the entry alive"
    );
    assert!(state.changed().is_empty(), "lines: {:?}", state.changed());
    assert!(!state.outside_change());
}

// --- other writers, review round 4: a write owns only what it may have sent -------

#[test]
fn an_apply_that_never_reached_the_mouse_owns_no_field() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::PollingRate(PollingRate::Hz500));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "no Glorious mouse found".to_owned(),
            landed: Vec::new(),
            reached: false,
        }),
        remembered: None,
        snapshot: Err(LoadError::NoMouse),
    });
    reload(
        &mut state,
        Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        },
    );
    assert!(state.outside_change());
    assert_eq!(
        state.changed(),
        lines(&["polling rate changed on the mouse to 250 Hz; your edit is kept"])
    );
}

#[test]
fn an_apply_refused_by_the_awake_check_owns_no_field() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "device is asleep".to_owned(),
            landed: Vec::new(),
            reached: false,
        }),
        remembered: None,
        snapshot: Err(LoadError::Asleep),
    });
    reload(
        &mut state,
        Snapshot {
            debounce: Debounce::new(7).unwrap(),
            ..snapshot()
        },
    );
    assert!(state.outside_change());
    assert_eq!(
        state.changed(),
        lines(&["debounce changed on the mouse to 7 ms; your edit is kept"])
    );
}

#[test]
fn an_apply_that_failed_on_its_first_report_owns_that_field() {
    let mut state = ready(snapshot());
    edit(&mut state, Edit::Debounce(4));
    assert_eq!(state.update(Msg::Apply).len(), 1);
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "HID transport failed: gone".to_owned(),
            landed: Vec::new(),
            reached: true,
        }),
        remembered: None,
        snapshot: Err(LoadError::Transport("gone".to_owned())),
    });
    reload(
        &mut state,
        Snapshot {
            debounce: Debounce::new(4).unwrap(),
            ..snapshot()
        },
    );
    assert!(!state.outside_change());
}

// --- other writers, review round 5: the mouse refused before the report ----------

#[test]
fn a_stage_pick_the_mouse_refused_before_its_report_shows_the_outside_shrink() {
    // Another program shrank the list to 2 stages after the last read; the
    // pick of stage 3 is refused before its report, and the answer's re-read
    // carries the shrink.
    let mut state = ready(snapshot());
    edit(&mut state, Edit::ActiveStage(3));
    assert_eq!(
        state.update(Msg::Apply),
        vec![Request::Apply {
            profile: profile(2),
            ops: vec![WriteOp::DpiStage(DpiStage::new(3).unwrap())],
        }]
    );
    state.update(Msg::Applied {
        error: Some(WriteFailure {
            message: "DPI stage 3 is out of range 1..=2".to_owned(),
            landed: Vec::new(),
            reached: false,
        }),
        snapshot: Ok(Snapshot {
            dpi: stages(&[400, 800]),
            dpi_stage: DpiStage::new(1).unwrap(),
            ..snapshot()
        }),
        remembered: None,
    });
    assert!(state.outside_change());
    assert_eq!(
        state.changed(),
        lines(&[
            "DPI stages changed on the mouse to 400 800; your edit is kept",
            "active stage changed on the mouse to 1; your edit is kept",
        ])
    );
}

// --- other writers, review rounds 6-7: an import's failing op and its re-read -----

const UNPLUGGED: &str = "HID transport failed: device unplugged";

/// A running import of `[PollingRate(500), Dpi(400 800)]` into profile 2.
fn import_running() -> State {
    let mut state = ready(snapshot());
    state.update(Msg::Import(import_path()));
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: true,
        writes: vec![
            WriteOp::PollingRate(PollingRate::Hz500),
            WriteOp::Dpi(stages(&[400, 800])),
        ],
    })));
    assert_eq!(state.update(Msg::ImportConfirm).len(), 1);
    state
}

/// The polling write landed, then the `Dpi` pre-read lost the mouse: the
/// failing op sent nothing, and the import's re-read fails the same way.
fn polling_landed_then_unplugged() -> State {
    let mut state = import_running();
    state.update(Msg::Imported {
        error: Some(UNPLUGGED.to_owned()),
        reached: false,
        written: vec![WriteOp::PollingRate(PollingRate::Hz500)],
        snapshot: Some(Err(LoadError::Transport(UNPLUGGED.to_owned()))),
        remembered: Some(Ok(Memory::default())),
    });
    state
}

#[test]
fn an_import_whose_dpi_pre_read_failed_after_its_polling_write_landed_keeps_that_write() {
    let mut state = polling_landed_then_unplugged();
    assert_eq!(
        state.snapshot().map(|snap| snap.polling_rate),
        Some(PollingRate::Hz500),
        "the landed write applies to the snapshot at once"
    );
    assert!(state.reopen(), "a transport error re-opens the mouse");
    // the next poll reads this window's own polling write and nothing else
    state.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz500,
        ..snapshot()
    })));
    assert!(!state.outside_change());
}

#[test]
fn an_import_whose_memory_save_failed_after_every_op_landed_keeps_its_writes() {
    let mut state = import_running();
    let read = Snapshot {
        polling_rate: PollingRate::Hz500,
        dpi: stages(&[400, 800]),
        dpi_stage: DpiStage::new(1).unwrap(),
        ..snapshot()
    };
    state.update(Msg::Imported {
        error: Some("cannot save memory".to_owned()),
        reached: false,
        written: vec![
            WriteOp::PollingRate(PollingRate::Hz500),
            WriteOp::Dpi(stages(&[400, 800])),
        ],
        snapshot: Some(Ok(read.clone())),
        remembered: None,
    });
    assert!(!state.outside_change());
    state.update(Msg::Loaded(Ok(read)));
    assert!(!state.outside_change());
}

#[test]
fn an_outside_move_after_an_import_that_sent_its_failing_op_nothing_still_raises() {
    let mut state = polling_landed_then_unplugged();
    state.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz250,
        ..snapshot()
    })));
    assert!(state.outside_change());
}

/// The polling write landed, the `Dpi` op failed, and the re-read shows a DPI
/// list another program wrote meanwhile.
fn polling_landed_then_dpi_failed(reached: bool) -> State {
    let mut state = import_running();
    state.update(Msg::Imported {
        error: Some(UNPLUGGED.to_owned()),
        reached,
        written: vec![WriteOp::PollingRate(PollingRate::Hz500)],
        snapshot: Some(Ok(Snapshot {
            polling_rate: PollingRate::Hz500,
            dpi: stages(&[400, 3200]),
            ..snapshot()
        })),
        remembered: None,
    });
    state
}

#[test]
fn an_import_whose_failing_op_sent_nothing_owns_no_field_on_its_re_read() {
    let state = polling_landed_then_dpi_failed(false);
    assert!(
        state.outside_change(),
        "the DPI list moved and no report of this import explains it"
    );
}

#[test]
fn an_import_whose_failing_op_sent_a_report_owns_its_fields_on_its_re_read() {
    let state = polling_landed_then_dpi_failed(true);
    assert!(
        !state.outside_change(),
        "the failing DPI report may have landed"
    );
}

/// A pick of profile 3 whose read failed keeps profile 2's snapshot hidden;
/// an import into profile 3 lands `written`, and its re-read fails too.
fn import_into_a_picked_profile_whose_read_failed(written: Vec<WriteOp>) -> State {
    let mut state = ready(snapshot());
    assert_eq!(state.update(Msg::Tick).len(), 1);
    assert!(state.update(Msg::PickProfile(profile(3))).is_empty());
    assert_eq!(state.update(Msg::Loaded(Ok(snapshot()))).len(), 1);
    state.update(Msg::Loaded(Err(LoadError::Failed(
        "HID transport failed: no response".to_owned(),
    ))));
    state.update(Msg::Import(import_path()));
    let requests = state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: written.clone(),
    })));
    assert!(
        matches!(requests.as_slice(), [Request::Import { profile: p, .. }] if *p == profile(3)),
        "the import writes profile 3: {requests:?}"
    );
    state.update(Msg::Imported {
        error: None,
        reached: false,
        written,
        snapshot: Some(Err(LoadError::Failed(
            "HID transport failed: no response".to_owned(),
        ))),
        remembered: Some(Ok(Memory::default())),
    });
    state
}

#[test]
fn an_import_into_another_profile_never_patches_the_hidden_profiles_dpi() {
    let mut state =
        import_into_a_picked_profile_whose_read_failed(vec![WriteOp::Dpi(stages(&[400, 800]))]);
    assert_eq!(state.update(Msg::PickProfile(profile(2))).len(), 1);
    state.update(Msg::Loaded(Ok(snapshot())));
    assert!(
        !state.outside_change(),
        "profile 3's DPI write was patched into profile 2's baseline"
    );
}

#[test]
fn an_import_into_another_profile_still_patches_its_device_wide_writes() {
    let mut state = import_into_a_picked_profile_whose_read_failed(vec![WriteOp::PollingRate(
        PollingRate::Hz500,
    )]);
    assert_eq!(state.update(Msg::PickProfile(profile(2))).len(), 1);
    state.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz500,
        ..snapshot()
    })));
    assert!(
        !state.outside_change(),
        "the polling rate is every profile's: this window's own write explains it"
    );
}

#[test]
fn an_import_into_another_profile_never_patches_the_hidden_profiles_debounce() {
    let mut state = import_into_a_picked_profile_whose_read_failed(vec![WriteOp::Debounce(
        Debounce::new(4).unwrap(),
    )]);
    assert_eq!(state.update(Msg::PickProfile(profile(2))).len(), 1);
    state.update(Msg::Loaded(Ok(snapshot())));
    assert!(
        !state.outside_change(),
        "profile 3's debounce write was patched into profile 2's baseline"
    );
}

#[test]
fn picking_back_after_an_import_into_another_profile_shows_only_its_own_values() {
    let mut state = import_into_a_picked_profile_whose_read_failed(vec![
        WriteOp::Dpi(stages(&[400, 800])),
        WriteOp::DpiStage(DpiStage::new(1).unwrap()),
        WriteOp::Debounce(Debounce::new(4).unwrap()),
    ]);
    assert_eq!(state.update(Msg::PickProfile(profile(2))).len(), 1);
    // profile 2's read has not landed yet: the panes show its kept snapshot
    assert_eq!(
        state.stage_texts(),
        Some(vec!["400".to_owned(), "800".to_owned(), "1600".to_owned()])
    );
    assert_eq!(
        state.active_stage(),
        Some(2),
        "profile 3's stage shows on profile 2"
    );
    assert_eq!(
        state.debounce(),
        Some(10),
        "profile 3's debounce shows on profile 2"
    );
}
