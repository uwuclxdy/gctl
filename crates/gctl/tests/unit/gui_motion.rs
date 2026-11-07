//! Motion, measured on rendered frames: a frame past every animation's end is
//! the at-rest frame, and a frame inside one is not, so a motion that never
//! settles and a change that never moves both fail here.
//!
//! The shell's clock moves on `Message::Frame` and never backwards; every
//! animation a message starts begins at the clock as that message lands. A
//! dropdown keeps its open state and its fold in its own widget state, timed by
//! the redraws it is handed, the way iced's own picker keeps its open state.

use std::time::Duration;

use iced::time::Instant;

use super::*;

/// One rendered frame of `shell` in its own scheme.
fn frame(shell: &Shell) -> iced_test::simulator::Snapshot {
    let mut ui = simulator(shell.view());
    ui.snapshot(&shell.theme()).unwrap()
}

/// Whether two frames hold the same pixels: the first writes its hash to a
/// fresh temp dir, the second compares against it.
fn same(a: &iced_test::simulator::Snapshot, b: &iced_test::simulator::Snapshot) -> bool {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("frame");
    assert!(a.matches_hash(&path).unwrap(), "the first hash is written");
    b.matches_hash(&path).unwrap()
}

#[test]
fn same_tells_equal_frames_from_different_ones() {
    let performance = shell(snapshot());
    let lighting = on_section(snapshot(), Section::Lighting);
    assert!(same(&frame(&performance), &frame(&performance)));
    assert!(!same(&frame(&performance), &frame(&lighting)));
}

/// Far past every duration the design uses (the longest, a slide, is 440 ms).
const SETTLED: Duration = Duration::from_secs(10);

/// Inside every animation a message starts: the shortest entrance is 120 ms.
const INSIDE: Duration = Duration::from_millis(40);

/// The window on Performance, then Lighting picked through the shell, the
/// clock `offset` past the pick.
fn selected_lighting_at(offset: Duration) -> Shell {
    let mut shell = shell(snapshot());
    let start = Instant::now();
    let _ = shell.update(Message::Frame(start));
    let _ = shell.update(Message::Machine(Msg::Select(Section::Lighting)));
    let _ = shell.update(Message::Frame(start + offset));
    shell
}

/// Opening a section raises and fades its cards in and slides the sidebar's
/// dot to its row; once every animation ends the frame is the one a window
/// opened on that section draws.
#[test]
fn a_section_change_moves_then_settles_on_the_at_rest_frame() {
    let rest = frame(&on_section(snapshot(), Section::Lighting));
    assert!(
        same(&frame(&selected_lighting_at(SETTLED)), &rest),
        "settled frame differs from the at-rest frame"
    );
    assert!(
        !same(&frame(&selected_lighting_at(INSIDE)), &rest),
        "nothing moved 40 ms into the section change"
    );
}

/// The window on profile 2, then profile 3 picked through the shell and its
/// read answered, the clock `offset` past the pick.
fn picked_three_at(offset: Duration) -> Shell {
    let mut shell = shell(snapshot());
    let start = Instant::now();
    let _ = shell.update(Message::Frame(start));
    let _ = shell.update(Message::Machine(Msg::PickProfile(profile(3))));
    let _ = shell.update(Message::Loaded {
        device: None,
        result: Ok(Snapshot {
            profile: profile(3),
            ..snapshot()
        }),
    });
    let _ = shell.update(Message::Frame(start + offset));
    shell
}

/// Picking a profile slides the selected segment to its digit; settled, the
/// frame is the one a window already on that profile draws.
#[test]
fn a_profile_pick_slides_then_settles_on_the_at_rest_frame() {
    let mut machine = State::new().0;
    machine.update(Msg::Loaded(Ok(snapshot())));
    machine.update(Msg::PickProfile(profile(3)));
    machine.update(Msg::Loaded(Ok(Snapshot {
        profile: profile(3),
        ..snapshot()
    })));
    let rest = frame(&shell_with_machine(machine));
    assert!(
        same(&frame(&picked_three_at(SETTLED)), &rest),
        "settled frame differs from the at-rest frame"
    );
    assert!(
        !same(&frame(&picked_three_at(INSIDE)), &rest),
        "nothing moved 40 ms into the profile pick"
    );
}

/// The clock only moves forward: a frame stamped before the clock, inside the
/// section change's run, leaves the change settled. Every instant derives from
/// the clock the change started at, so a clock that winds back to the older
/// frame lands inside the change.
#[test]
fn an_older_frame_never_winds_the_clock_back() {
    let mut shell = shell(snapshot());
    let _ = shell.update(Message::Frame(Instant::now()));
    let _ = shell.update(Message::Machine(Msg::Select(Section::Lighting)));
    let started = shell.motion.now();
    let _ = shell.update(Message::Frame(started + SETTLED));
    assert!(!shell.motion.animating(), "the change never settled");
    let _ = shell.update(Message::Frame(started + Duration::from_millis(20)));
    assert!(
        !shell.motion.animating(),
        "a frame 20 ms into the change wound the clock back"
    );
    assert!((shell.motion.section_alpha.at(shell.motion.now()) - 1.0).abs() < f32::EPSILON);
}

fn redraw(at: Instant) -> iced::Event {
    iced::Event::Window(iced::window::Event::RedrawRequested(at))
}

/// Hands a simulator a redraw at `at`, then one past every animation that
/// redraw started: whatever the cursor or a key changed is settled after it.
fn settle(ui: &mut iced_test::Simulator<'_, Message, iced::Theme, iced::Renderer>, at: Instant) {
    let _ = ui.simulate([redraw(at), redraw(at + SETTLED)]);
}

/// The polling dropdown's trigger centre.
fn polling_trigger(
    ui: &mut iced_test::Simulator<'_, Message, iced::Theme, iced::Renderer>,
) -> iced::Point {
    let picker = ui
        .find(iced::widget::Id::new("polling-picker"))
        .expect("the polling dropdown is on the pane");
    let bounds = picker.visible_bounds().expect("the dropdown is visible");
    iced::Point::new(
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    )
}

/// A dropdown folds open from its trigger: the first frame after the click is
/// still the closed body (the fold starts at that frame), and once settled the
/// menu is open. Escape folds it shut the same way: the first frame after it
/// is still the open menu, and once settled it is the closed body again. A
/// menu that pops open or shut in one frame fails both halves. The cursor
/// rests on the trigger throughout, so any hover fade settles alike in the
/// frames compared.
#[test]
fn a_dropdown_folds_open_and_shut_instead_of_popping() {
    let shell = shell(snapshot());
    let theme = shell.theme();
    let base = Instant::now() + Duration::from_secs(3600);

    let mut idle = simulator(shell.view());
    let at = polling_trigger(&mut idle);
    idle.point_at(at);
    let closed_first = idle.snapshot(&theme).unwrap();
    settle(&mut idle, base);
    let closed = idle.snapshot(&theme).unwrap();

    let mut ui = simulator(shell.view());
    ui.point_at(at);
    ui.simulate(click());
    let first = ui.snapshot(&theme).unwrap();
    assert!(
        same(&first, &closed_first),
        "the menu drew open in the frame of its click"
    );
    settle(&mut ui, base);
    let open = ui.snapshot(&theme).unwrap();
    assert!(!same(&open, &closed), "the menu never opened");

    let _ = ui.tap_key(iced::keyboard::key::Named::Escape);
    let closing = ui.snapshot(&theme).unwrap();
    assert!(
        same(&closing, &open),
        "the menu vanished in the frame of its Escape"
    );
    settle(&mut ui, base + SETTLED + SETTLED);
    let shut = ui.snapshot(&theme).unwrap();
    assert!(same(&shut, &closed), "the folded menu left a trace");
}

/// The edits an open polling dropdown emits for `keys`, tapped in order after
/// the click that opens it.
fn polling_keys(keys: &[iced::keyboard::key::Named]) -> Vec<Edit> {
    let shell = shell(snapshot());
    let mut ui = simulator(shell.view());
    let at = polling_trigger(&mut ui);
    ui.point_at(at);
    ui.simulate(click());
    let _ = ui.simulate([redraw(Instant::now() + SETTLED)]);
    for key in keys {
        let _ = ui.tap_key(*key);
    }
    ui.into_messages()
        .filter_map(|message| match message {
            Message::Machine(Msg::Edit(edit)) => Some(edit),
            _ => None,
        })
        .collect()
}

/// An open dropdown takes the keyboard: the highlight starts on the current
/// value (1000 Hz, the last option), the arrows move it, Enter picks it, and
/// Escape closes with no edit.
#[test]
fn an_open_dropdown_takes_arrows_enter_and_escape() {
    use iced::keyboard::key::Named;
    assert_eq!(
        polling_keys(&[Named::ArrowUp, Named::Enter]),
        vec![Edit::PollingRate(PollingRate::Hz500)]
    );
    assert_eq!(
        polling_keys(&[
            Named::ArrowUp,
            Named::ArrowUp,
            Named::ArrowDown,
            Named::Enter
        ]),
        vec![Edit::PollingRate(PollingRate::Hz500)]
    );
    assert_eq!(
        polling_keys(&[Named::Enter]),
        vec![Edit::PollingRate(PollingRate::Hz1000)]
    );
    assert_eq!(polling_keys(&[Named::Escape]), Vec::new());
}

/// A click outside an open dropdown folds it shut and picks nothing.
#[test]
fn a_click_outside_an_open_dropdown_closes_it_with_no_edit() {
    let shell = shell(snapshot());
    let theme = shell.theme();
    let base = Instant::now() + Duration::from_secs(3600);
    let mut idle = simulator(shell.view());
    let at = polling_trigger(&mut idle);
    let away = iced::Point::new(at.x, 8.0);
    idle.point_at(away);
    settle(&mut idle, base);
    let closed = idle.snapshot(&theme).unwrap();

    let mut ui = simulator(shell.view());
    ui.point_at(at);
    ui.simulate(click());
    settle(&mut ui, base);
    ui.point_at(away);
    ui.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
        position: away,
    })]);
    ui.simulate(click());
    settle(&mut ui, base + SETTLED + SETTLED);
    let shut = ui.snapshot(&theme).unwrap();
    assert!(same(&shut, &closed), "the menu stayed open");
    assert_eq!(edits(ui), Vec::new());
}

fn edits(ui: iced_test::Simulator<'_, Message, iced::Theme, iced::Renderer>) -> Vec<Edit> {
    ui.into_messages()
        .filter_map(|message| match message {
            Message::Machine(Msg::Edit(edit)) => Some(edit),
            _ => None,
        })
        .collect()
}

// --- the open list: closing, keys, scroll, sinks ------------------------------

/// A dropdown trigger's centre and bounds, by its id.
fn trigger(
    ui: &mut iced_test::Simulator<'_, Message, iced::Theme, iced::Renderer>,
    id: &'static str,
) -> (iced::Point, iced::Rectangle) {
    let bounds = ui
        .find(iced::widget::Id::new(id))
        .unwrap_or_else(|_| panic!("no {id} on the pane"))
        .visible_bounds()
        .unwrap_or_else(|| panic!("{id} is not visible"));
    (
        iced::Point::new(
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        ),
        bounds,
    )
}

/// A click where a folding-shut panel's option sat picks nothing: the
/// panel takes no click once it is closing.
#[test]
fn a_click_on_a_closing_panel_picks_nothing() {
    let shell = shell(snapshot());
    let base = Instant::now() + Duration::from_secs(3600);
    let mut ui = simulator(shell.view());
    let (at, bounds) = trigger(&mut ui, "polling-picker");
    ui.point_at(at);
    ui.simulate(click());
    settle(&mut ui, base);
    let _ = ui.tap_key(iced::keyboard::key::Named::Escape);
    // the first option's row, wherever the panel opened
    let y = menu_row_y(
        bounds,
        POLLING_RATES.len(),
        0,
        window_settings().size.height,
    );
    let option = iced::Point::new(at.x, y);
    ui.point_at(option);
    ui.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
        position: option,
    })]);
    ui.simulate(click());
    assert_eq!(edits(ui), Vec::new(), "a click on the folding panel picked");
}

/// Whether a held press on the polling dropdown, settled, draws the same
/// frame as no press, under the given motion preference.
fn pressed_looks_at_rest(reduce: bool) -> bool {
    let mut shell = shell(snapshot());
    shell.motion.prefs = crate::gui::motion::Prefs {
        factor: 1.0,
        reduce,
    };
    let theme = shell.theme();
    let base = Instant::now() + Duration::from_secs(3600);
    let mut idle = simulator(shell.view());
    let (at, _) = trigger(&mut idle, "polling-picker");
    idle.point_at(at);
    settle(&mut idle, base);
    let rest = idle.snapshot(&theme).unwrap();
    let mut ui = simulator(shell.view());
    ui.point_at(at);
    ui.simulate([iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
        iced::mouse::Button::Left,
    ))]);
    settle(&mut ui, base);
    let pressed = ui.snapshot(&theme).unwrap();
    same(&pressed, &rest)
}

/// With motion on, a held press sinks the dropdown's field: the control for
/// the reduced-motion check below.
#[test]
fn a_held_press_sinks_the_dropdown_with_motion_on() {
    assert!(
        !pressed_looks_at_rest(false),
        "no sink drawn with motion on"
    );
}

/// Reduced motion drops every sink, the dropdown's included.
#[test]
fn reduced_motion_drops_the_dropdowns_sink() {
    assert!(
        pressed_looks_at_rest(true),
        "the dropdown sank under reduced motion"
    );
}

/// ArrowDown from no highlight lands on the first option: the effect
/// picker with nothing remembered has no current value.
#[test]
fn arrow_down_from_no_value_lands_on_the_first_option() {
    let shell = on_section(snapshot(), Section::Lighting);
    let base = Instant::now() + Duration::from_secs(3600);
    let mut ui = simulator(shell.view());
    let (at, _) = trigger(&mut ui, "effect-picker");
    ui.point_at(at);
    ui.simulate(click());
    settle(&mut ui, base);
    let _ = ui.tap_key(iced::keyboard::key::Named::ArrowDown);
    let _ = ui.tap_key(iced::keyboard::key::Named::Enter);
    assert_eq!(
        edits(ui),
        vec![Edit::Effect(crate::gui::state::EffectKind::ALL[0])]
    );
}

/// The window on Buttons with the left button bound to `key:KeyA`.
fn left_bound_to_key_a() -> Shell {
    let mut snap = snapshot();
    snap.binds[0] = (
        Button::Left,
        Some(crate::parse::binding("key:KeyA").unwrap()),
    );
    on_section(snap, Section::Buttons)
}

/// The edit that binds the left button to the key list's last key, built from
/// the CLI grammar.
fn left_to_last_key() -> Edit {
    let last = gctl_proto::Key::all().last().unwrap();
    Edit::Bind(
        Button::Left,
        crate::parse::binding(&format!("key:{}", last.name())).unwrap(),
    )
}

/// The key picker's last key is reachable by keyboard: the arrows walk
/// the whole list, scrolling it, and Enter picks the last key.
#[test]
fn the_key_pickers_last_key_is_reachable_by_keyboard() {
    let shell = left_bound_to_key_a();
    let base = Instant::now() + Duration::from_secs(3600);
    let mut ui = simulator(shell.view());
    let (at, _) = trigger(&mut ui, "key-picker-left");
    ui.point_at(at);
    ui.simulate(click());
    settle(&mut ui, base);
    for _ in 0..200 {
        let _ = ui.tap_key(iced::keyboard::key::Named::ArrowDown);
    }
    let _ = ui.tap_key(iced::keyboard::key::Named::Enter);
    assert_eq!(edits(ui).last(), Some(&left_to_last_key()));
}

/// The open panel is at most 240 px tall and its rows scroll: scrolled to
/// its end with the wheel, a click on the panel's last 36 px row, inside the
/// window, picks the last key.
#[test]
fn the_key_pickers_last_key_is_reachable_by_pointer() {
    let shell = left_bound_to_key_a();
    let base = Instant::now() + Duration::from_secs(3600);
    let height = window_settings().size.height;
    let mut ui = simulator(shell.view());
    let (at, bounds) = trigger(&mut ui, "key-picker-left");
    ui.point_at(at);
    ui.simulate(click());
    settle(&mut ui, base);
    let (top, panel) = menu_panel(bounds, gctl_proto::Key::all().count(), height);
    // scrolled to its end, the last row sits right above the bottom gutter
    let (inside, last_row) = (
        top + panel / 2.0,
        top + panel - MENU_GUTTER - MENU_ROW / 2.0,
    );
    assert!(
        last_row > 0.0 && last_row < height,
        "the last row sits at y {last_row}"
    );
    let over = iced::Point::new(at.x, inside);
    ui.point_at(over);
    ui.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
        position: over,
    })]);
    ui.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
        delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -1000.0 },
    })]);
    settle(&mut ui, base + SETTLED + SETTLED);
    let row = iced::Point::new(at.x, last_row);
    ui.point_at(row);
    ui.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
        position: row,
    })]);
    ui.simulate(click());
    assert_eq!(edits(ui).last(), Some(&left_to_last_key()));
}

/// The first profile slide after a real boot starts where the selected
/// segment is drawn: the mouse's first read reaches the motion through
/// the shell's message path, so the segment never starts from profile 1.
#[test]
fn the_first_slide_after_boot_starts_at_the_drawn_segment() {
    let mut shell = Shell::new(State::new().0);
    let _ = shell.update(Message::Loaded {
        device: None,
        result: Ok(snapshot()),
    });
    let _ = shell.update(Message::Frame(Instant::now()));
    let drawn = segment_offset(profile_index(&shell.machine));
    let _ = shell.update(Message::Machine(Msg::PickProfile(profile(3))));
    let origin = shell.motion.segment.at(shell.motion.now());
    assert!(
        (origin - drawn).abs() < 0.5,
        "the slide starts at {origin}, the segment was drawn at {drawn}"
    );
}

// --- every part settles; marks under a prompt -----------------------------------

/// Shells covering every kind of part the window draws: panes, the sidebar
/// with Make N active, both modals, the banners and the footer lines.
fn every_part() -> Vec<(&'static str, Shell)> {
    let mut discard = State::new().0;
    discard.update(Msg::Loaded(Ok(snapshot())));
    discard.update(Msg::Edit(Edit::Debounce(0)));
    discard.update(Msg::Select(Section::Lighting));
    let mut import = shell(snapshot());
    import
        .machine
        .update(Msg::Import(std::path::PathBuf::from("/o.toml")));
    import
        .machine
        .update(Msg::ImportChecked(Ok(crate::gui::state::ImportCheck {
        note: Some(
            "\"/o.toml\" holds settings for \"Model D Wireless\"; writing them to Model O Wireless"
                .to_owned(),
        ),
        device_wide: true,
        writes: Vec::new(),
    })));
    let mut outside = State::new().0;
    outside.update(Msg::Loaded(Ok(snapshot())));
    outside.update(Msg::Edit(Edit::PollingRate(PollingRate::Hz500)));
    outside.update(Msg::Tick);
    outside.update(Msg::Loaded(Ok(Snapshot {
        polling_rate: PollingRate::Hz250,
        debounce: gctl_proto::Debounce::new(7).unwrap(),
        ..snapshot()
    })));
    let mut make_active = shell(snapshot());
    make_active.machine.update(Msg::PickProfile(profile(3)));
    make_active.machine.update(Msg::Loaded(Ok(Snapshot {
        profile: profile(3),
        ..snapshot()
    })));
    vec![
        ("performance", shell(snapshot())),
        ("lighting", on_section(snapshot(), Section::Lighting)),
        ("buttons", on_section(snapshot(), Section::Buttons)),
        ("device", on_section(snapshot(), Section::Device)),
        ("make active", make_active),
        ("discard modal", shell_with_machine(discard)),
        ("import modal", import),
        ("outside banner and lines", shell_with_machine(outside)),
    ]
}

/// `render(scheme)` draws every part in the scheme it is handed, whatever the
/// shell's own scheme: the two-theme frames the tests write and the live
/// window share that one path, so a part reading the shell's scheme instead
/// (the round-3 modals drew dark text on the light modal) fails here.
#[test]
fn render_draws_every_part_in_the_scheme_it_is_handed() {
    let mut wrong = Vec::new();
    for (case, mut shell) in every_part() {
        for (scheme, other) in [(Scheme::Light, Scheme::Dark), (Scheme::Dark, Scheme::Light)] {
            shell.scheme = other;
            let handed = simulator(shell.render(scheme))
                .snapshot(&scheme.theme())
                .unwrap();
            shell.scheme = scheme;
            let own = frame(&shell);
            if !same(&handed, &own) {
                wrong.push(format!("{case}: render({scheme:?}) on a {other:?} shell"));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// The dropdown trigger fades its hover surface in: the
/// first redraw that sees the cursor starts the fade, so that frame is not yet
/// the settled hover frame.
#[test]
fn the_dropdown_trigger_fades_its_hover_in() {
    let shell = shell(snapshot());
    let theme = shell.theme();
    let base = Instant::now() + Duration::from_secs(3600);
    let mut ui = simulator(shell.view());
    let (at, _) = trigger(&mut ui, "polling-picker");
    let away = iced::Point::new(at.x, 8.0);
    ui.point_at(away);
    let _ = ui.simulate([redraw(base)]);
    let rest = ui.snapshot(&theme).unwrap();
    ui.point_at(at);
    ui.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
        position: at,
    })]);
    let _ = ui.simulate([redraw(base + Duration::from_millis(1))]);
    let first = ui.snapshot(&theme).unwrap();
    let _ = ui.simulate([redraw(base + SETTLED)]);
    let settled = ui.snapshot(&theme).unwrap();
    assert!(!same(&rest, &settled), "the trigger has no hover surface");
    assert!(
        !same(&first, &settled),
        "the trigger's hover snapped on in one frame"
    );
}

/// A background read under the discard prompt that moves the active profile
/// slides the active dot without drawing it over the scrim.
#[test]
fn the_active_dot_never_draws_over_the_discard_prompt() {
    let mut shell = shell(snapshot());
    let base = Instant::now() + Duration::from_secs(3600);
    let _ = shell.update(Message::Frame(base));
    let _ = shell.update(Message::Machine(Msg::Edit(Edit::Debounce(0))));
    let _ = shell.update(Message::Machine(Msg::Select(Section::Lighting)));
    assert!(shell.machine.prompt().is_some(), "no discard prompt");
    let _ = shell.update(Message::Machine(Msg::Tick));
    let _ = shell.update(Message::Loaded {
        device: None,
        result: Ok(Snapshot {
            active: profile(3),
            ..snapshot()
        }),
    });
    assert!(
        shell.machine.prompt().is_some(),
        "the read closed the prompt"
    );
    let _ = shell.update(Message::Frame(base + Duration::from_millis(100)));
    let mid = frame(&shell);
    let _ = shell.update(Message::Frame(base + SETTLED));
    assert!(
        same(&mid, &frame(&shell)),
        "the sliding dot changed the modal frame"
    );
}

/// A mark that moved under a prompt is at its new place when the prompt
/// closes: the active dot slid under a discard prompt, the prompt
/// closes 80 ms into the slide, and 20 ms later the frame is the settled one.
#[test]
fn a_mark_moved_under_a_prompt_stays_put_when_it_closes() {
    let mut shell = shell(snapshot());
    let base = Instant::now() + Duration::from_secs(3600);
    let _ = shell.update(Message::Frame(base));
    let _ = shell.update(Message::Machine(Msg::Edit(Edit::Debounce(0))));
    let _ = shell.update(Message::Machine(Msg::Select(Section::Lighting)));
    assert!(shell.machine.prompt().is_some(), "no discard prompt");
    let _ = shell.update(Message::Machine(Msg::Tick));
    let _ = shell.update(Message::Loaded {
        device: None,
        result: Ok(Snapshot {
            active: profile(3),
            ..snapshot()
        }),
    });
    let _ = shell.update(Message::Frame(base + Duration::from_millis(80)));
    let _ = shell.update(Message::Machine(Msg::KeepEditing));
    assert!(shell.machine.prompt().is_none(), "the prompt stayed");
    let _ = shell.update(Message::Frame(base + Duration::from_millis(100)));
    let just_closed = frame(&shell);
    let _ = shell.update(Message::Frame(base + SETTLED));
    assert!(
        same(&just_closed, &frame(&shell)),
        "after the prompt closed the active dot is back inside its slide"
    );
}

/// In a short window the key picker's panel never covers its trigger:
/// a click on the trigger of the open picker closes it and picks nothing.
#[test]
fn a_click_on_an_open_pickers_trigger_in_a_short_window_picks_nothing() {
    let shell = left_bound_to_key_a();
    let base = Instant::now() + Duration::from_secs(3600);
    let mut ui = simulator_sized(shell.view(), iced::Size::new(1024.0, 420.0));
    let (at, _) = trigger(&mut ui, "key-picker-left");
    ui.point_at(at);
    ui.simulate(click());
    settle(&mut ui, base);
    ui.simulate(click());
    assert_eq!(edits(ui), Vec::new(), "the trigger's click picked a key");
}
