//! The setting grid, measured: every pane lays its rows out as
//! label | control | readout | tags, with the tags pinned to the card's right
//! edge. Positions come from the widget bounds the simulator reports, so a
//! drifting column fails here instead of in a render.

use iced::Rectangle;

use super::*;

const TAGS: [&str; 5] = [
    "all profiles",
    "per profile",
    "last set by gctl",
    "Unknown",
    "Colour unknown",
];

/// Everything left of this x is the sidebar, which is not part of the grid.
const PANE_LEFT: f32 = 230.0;

/// Positions agree when they differ by less than this many logical pixels.
const SLACK: f32 = 1.0;

#[derive(Debug, Clone)]
struct Placed {
    content: String,
    bounds: Rectangle,
}

fn texts(shell: &Shell) -> Vec<Placed> {
    let mut ui = simulator(shell.view());
    let mut found = Vec::new();
    let _ = ui.find(|candidate: iced_test::selector::Candidate<'_>| {
        if let iced_test::selector::Candidate::Text {
            content, bounds, ..
        } = candidate
            && bounds.x >= PANE_LEFT
        {
            found.push(Placed {
                content: content.to_owned(),
                bounds,
            });
        }
        None::<()>
    });
    found
}

fn inputs(shell: &Shell) -> Vec<Rectangle> {
    let mut ui = simulator(shell.view());
    let mut found = Vec::new();
    let _ = ui.find(|candidate: iced_test::selector::Candidate<'_>| {
        if let iced_test::selector::Candidate::TextInput { bounds, .. } = candidate
            && bounds.x >= PANE_LEFT
        {
            found.push(bounds);
        }
        None::<()>
    });
    found
}

fn centre_y(bounds: &Rectangle) -> f32 {
    bounds.y + bounds.height / 2.0
}

/// The tags grouped by row (centres within 6 px), each row's rightmost edge.
fn tag_row_ends(placed: &[Placed]) -> Vec<(f32, f32)> {
    let mut rows: Vec<(f32, f32)> = Vec::new();
    for tag in placed.iter().filter(|p| TAGS.contains(&p.content.as_str())) {
        let y = centre_y(&tag.bounds);
        let right = tag.bounds.x + tag.bounds.width;
        match rows.iter_mut().find(|(row_y, _)| (row_y - y).abs() < 6.0) {
            Some((_, end)) => *end = end.max(right),
            None => rows.push((y, right)),
        }
    }
    rows
}

fn all_agree(name: &str, values: &[(String, f32)]) {
    let Some((first_name, first)) = values.first() else {
        panic!("{name}: nothing measured");
    };
    for (what, value) in values {
        assert!(
            (value - first).abs() < SLACK,
            "{name}: {what} at {value}, {first_name} at {first}"
        );
    }
}

/// A snapshot where every write-only value is remembered, so every mark a row
/// can carry is on screen.
fn remembered() -> Snapshot {
    let mut snap = snapshot();
    snap.effect = Some(Effect::Wave {
        rate: Rate::new(40).unwrap(),
    });
    snap.brightness = Some(gctl_proto::Brightness {
        wired: 200,
        wireless: 50,
    });
    snap.dpi_colors = Some(vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")]);
    for (index, button) in BUTTONS.iter().enumerate() {
        snap.binds[index] = (*button, Some(Binding::Mouse(MouseFn::Forward)));
    }
    snap
}

/// The widest rows the panes can show: key binds carrying modifiers (the
/// longest modifier, `meta-right`, included) and a six-colour pulse.
fn widest() -> Snapshot {
    let mut snap = remembered();
    snap.effect = Some(Effect::Pulse {
        rate: Rate::new(40).unwrap(),
        colors: ["ff0000", "00ff00", "0000ff", "ffff00", "00ffff", "ff00ff"]
            .into_iter()
            .map(rgb)
            .collect(),
    });
    for (index, (button, text)) in BUTTONS
        .iter()
        .zip([
            "key:ctrl+KeyA",
            "key:ctrl+shift+KeyB",
            "key:meta-right+KeyC",
            "key:ctrl+shift+alt+meta+F12",
            "key:KeyE",
            "mouse:forward",
            "media:volume-up",
            "dpi:stage-up",
        ])
        .enumerate()
    {
        snap.binds[index] = (*button, Some(crate::parse::binding(text).unwrap()));
    }
    snap
}

const SECTIONS: [Section; 4] = [
    Section::Performance,
    Section::Lighting,
    Section::Buttons,
    Section::Device,
];

/// Every pane: nothing remembered, everything remembered, and the widest rows.
fn panes() -> Vec<(String, Section, Shell)> {
    let mut panes = Vec::new();
    for (state, snap) in [
        ("empty", snapshot()),
        ("remembered", remembered()),
        ("widest", widest()),
    ] {
        for section in SECTIONS {
            panes.push((
                format!("{section:?} {state}"),
                section,
                on_section(snap.clone(), section),
            ));
        }
    }
    panes
}

/// Row labels only some states show: the effect's speed and colour list.
const OPTIONAL_LABELS: [&str; 2] = ["Speed", "Colours"];

/// The row labels each pane shows in every state. Card titles are caps
/// eyebrows, so `Effect` and `Scroll` match their row alone.
fn labels(section: Section) -> &'static [&'static str] {
    match section {
        Section::Performance => &[
            "Stage 1",
            "Stage 2",
            "Stage 3",
            "Active stage",
            "Polling rate",
            "Lift-off",
            "Debounce",
            "Sleep",
        ],
        Section::Lighting => &["Effect", "Wired", "Wireless"],
        Section::Buttons => &[
            "Scroll",
            "Left",
            "Right",
            "Wheel click",
            "Back",
            "Forward",
            "Scroll up",
            "Scroll down",
            "DPI button",
        ],
        Section::Device => &[
            "Model",
            "Link",
            "Firmware",
            "Battery",
            "Export profile",
            "Import profile",
        ],
    }
}

#[test]
fn every_row_label_starts_in_one_column_on_every_pane() {
    let mut lefts = Vec::new();
    for (pane, section, shell) in panes() {
        let placed = texts(&shell);
        for label in labels(section) {
            let found: Vec<&Placed> = placed.iter().filter(|p| p.content == *label).collect();
            assert!(!found.is_empty(), "{pane}: no {label:?} label");
            for p in found {
                lefts.push((format!("{pane} {label:?}"), p.bounds.x));
            }
        }
        // rows only some effects show
        for p in placed
            .iter()
            .filter(|p| OPTIONAL_LABELS.contains(&p.content.as_str()))
        {
            lefts.push((format!("{pane} {:?}", p.content), p.bounds.x));
        }
    }
    // the pulse effect's colour list is a row of its own
    let pulse = on_section(widest(), Section::Lighting);
    let Some(colours) = texts(&pulse)
        .into_iter()
        .find(|placed| placed.content == "Colours")
    else {
        panic!("Lighting widest: no \"Colours\" label");
    };
    lefts.push(("Lighting widest \"Colours\"".to_owned(), colours.bounds.x));
    all_agree("label column", &lefts);
}

fn is_row_label(section: Section, content: &str) -> bool {
    labels(section).contains(&content) || OPTIONAL_LABELS.contains(&content)
}

/// The texts sharing a labelled row: centres within 6 px of the label's.
fn row_of<'a>(placed: &'a [Placed], label: &Placed) -> Vec<&'a Placed> {
    let y = centre_y(&label.bounds);
    let mut row: Vec<&Placed> = placed
        .iter()
        .filter(|p| (centre_y(&p.bounds) - y).abs() < 6.0)
        .collect();
    row.sort_by(|a, b| a.bounds.x.total_cmp(&b.bounds.x));
    row
}

#[test]
fn no_short_text_on_a_pane_wraps() {
    let measured: Vec<(String, Vec<Placed>)> = panes()
        .into_iter()
        .map(|(pane, _, shell)| (pane, texts(&shell)))
        .collect();
    // one line of a tag (the smallest text size) sets the bar: a wrapped text
    // is at least two such lines tall, while a card title (the largest
    // single-line text) stays under 1.8 of them. every short text counts,
    // wherever its row puts it (a key bind's modifier line included); the
    // legend and notices are long sentences that may wrap.
    let line = measured
        .iter()
        .flat_map(|(_, placed)| placed)
        .filter(|p| TAGS.contains(&p.content.as_str()))
        .map(|p| p.bounds.height)
        .fold(f32::INFINITY, f32::min);
    assert!(line.is_finite(), "no tag to measure a line by");
    for (pane, placed) in &measured {
        for text in placed.iter().filter(|p| p.content.chars().count() <= 32) {
            assert!(
                text.bounds.height < line * 1.8,
                "{pane}: {:?} is {} tall, one line is {line}",
                text.content,
                text.bounds.height
            );
        }
    }
}

#[test]
fn no_two_texts_in_a_setting_row_overlap() {
    for (pane, section, shell) in panes() {
        let placed = texts(&shell);
        for label in placed.iter().filter(|p| is_row_label(section, &p.content)) {
            let row = row_of(&placed, label);
            for pair in row.windows(2) {
                let (left, right) = (pair[0], pair[1]);
                assert!(
                    left.bounds.x + left.bounds.width <= right.bounds.x + 0.5,
                    "{pane}: {:?} runs into {:?} on the {:?} row",
                    left.content,
                    right.content,
                    label.content
                );
            }
        }
    }
}

#[test]
fn every_tag_row_ends_at_the_cards_right_edge_on_every_pane() {
    let mut ends = Vec::new();
    for (pane, _, shell) in panes() {
        for (y, end) in tag_row_ends(&texts(&shell)) {
            ends.push((format!("{pane} row at y {y}"), end));
        }
    }
    assert!(ends.len() >= 20, "only {} tag rows found", ends.len());
    all_agree("tag column right edge", &ends);
}

#[test]
fn every_slider_readout_starts_in_one_column() {
    let mut lefts = Vec::new();
    for (pane, _, shell) in panes() {
        for placed in texts(&shell) {
            if ["10 ms", "40", "200", "50"].contains(&placed.content.as_str()) {
                lefts.push((format!("{pane} {:?}", placed.content), placed.bounds.x));
            }
        }
    }
    assert!(lefts.len() >= 5, "only {} readouts found", lefts.len());
    all_agree("readout column", &lefts);
}

/// Rows whose first control is a text input, on the panes that show them.
const INPUT_ROWS: [(Section, &str); 5] = [
    (Section::Performance, "Stage 1"),
    (Section::Performance, "Stage 2"),
    (Section::Performance, "Stage 3"),
    (Section::Device, "Export profile"),
    (Section::Device, "Import profile"),
];

#[test]
fn every_rows_first_input_starts_in_the_control_column() {
    let mut firsts = Vec::new();
    for (state, snap) in [
        ("empty", snapshot()),
        ("remembered", remembered()),
        ("widest", widest()),
    ] {
        for (section, label) in INPUT_ROWS {
            let shell = on_section(snap.clone(), section);
            let Some(row) = texts(&shell)
                .into_iter()
                .find(|placed| placed.content == label)
            else {
                panic!("{section:?} {state}: no {label:?} label");
            };
            let y = centre_y(&row.bounds);
            let first = inputs(&shell)
                .into_iter()
                .filter(|bounds| (centre_y(bounds) - y).abs() < 12.0)
                .map(|bounds| bounds.x)
                .fold(f32::INFINITY, f32::min);
            assert!(
                first.is_finite(),
                "{section:?} {state}: no input on the {label:?} row"
            );
            firsts.push((format!("{section:?} {state} {label:?}"), first));
        }
    }
    all_agree("control column", &firsts);
}

/// A bind picker's horizontal padding (12 each side) plus its arrow (9.1 px at
/// text size 14, rounded up): what a `KEY_PICKER_WIDTH` picker keeps from text.
const PICKER_CHROME: f32 = 24.0 + 10.0;

/// Pick lists report no text to the selector, so a name too wide for its
/// picker cannot show up in the grid checks above: measure every name a bind
/// picker offers, at the picker's text size, in the UI font and in the
/// default font it falls back to.
#[test]
fn every_bind_picker_name_fits_the_picker() {
    let room = KEY_PICKER_WIDTH - PICKER_CHROME;
    let mut names: Vec<&str> = Vec::new();
    names.extend(crate::parse::MOUSE_FN_NAMES);
    names.extend(crate::parse::MEDIA_FN_NAMES);
    names.extend(crate::parse::DPI_FN_NAMES);
    names.extend(crate::parse::KEYBOARD_FN_NAMES);
    names.extend(gctl_proto::Key::all().map(gctl_proto::Key::name));
    assert!(names.len() > 100, "only {} names", names.len());
    for name in names {
        let mut ui = simulator(iced::widget::text(name).size(14).font(UI_FONT).into());
        let mut width = 0.0f32;
        let _ = ui.find(|candidate: iced_test::selector::Candidate<'_>| {
            if let iced_test::selector::Candidate::Text { bounds, .. } = candidate {
                width = width.max(bounds.width);
            }
            None::<()>
        });
        assert!(width > 0.0, "{name:?}: nothing measured");
        assert!(
            width <= room,
            "{name:?} is {width} wide in {UI_FONT:?}, a picker keeps {room}"
        );
    }
}

/// How far a button's label may sit from the edge its button starts at: the
/// button's own padding plus a little.
const BUTTON_INSET: f32 = 24.0;

fn one<'a>(placed: &'a [Placed], content: &str) -> &'a Placed {
    let found: Vec<&Placed> = placed.iter().filter(|p| p.content == content).collect();
    assert_eq!(found.len(), 1, "{content:?} found {} times", found.len());
    found[0]
}

/// An action button is as wide as its label plus padding, never as wide as the
/// space around it: each label sits next to the edge its button is anchored
/// to, not centred in a stretched button.
#[test]
fn action_buttons_hug_their_labels() {
    for (state, snap) in [("empty", snapshot()), ("remembered", remembered())] {
        let shell = on_section(snap, Section::Performance);
        let placed = texts(&shell);
        let card_right = tag_row_ends(&placed)
            .into_iter()
            .map(|(_, end)| end)
            .fold(f32::NEG_INFINITY, f32::max);
        let label_x = one(&placed, "Stage 1").bounds.x;
        let stage_y = centre_y(&one(&placed, "Stage 1").bounds);
        let control_x = inputs(&shell)
            .into_iter()
            .filter(|bounds| (centre_y(bounds) - stage_y).abs() < 12.0)
            .map(|bounds| bounds.x)
            .fold(f32::INFINITY, f32::min);

        let apply = one(&placed, "Apply").bounds;
        let apply_gap = (card_right - (apply.x + apply.width)).abs();
        assert!(
            apply_gap <= BUTTON_INSET,
            "{state}: Apply's label ends {apply_gap} from the card's right edge"
        );

        let revert_gap = (one(&placed, "Revert").bounds.x - label_x).abs();
        assert!(
            revert_gap <= BUTTON_INSET,
            "{state}: Revert's label starts {revert_gap} from the label column"
        );

        let add_gap = one(&placed, "Add stage").bounds.x - control_x;
        assert!(
            (0.0..=BUTTON_INSET).contains(&add_gap),
            "{state}: Add stage's label starts {add_gap} past the control column"
        );

        // each Remove follows the last input on its stage row
        for remove in placed.iter().filter(|p| p.content == "Remove") {
            let y = centre_y(&remove.bounds);
            let input_end = inputs(&shell)
                .into_iter()
                .filter(|bounds| (centre_y(bounds) - y).abs() < 12.0)
                .map(|bounds| bounds.x + bounds.width)
                .fold(f32::NEG_INFINITY, f32::max);
            let gap = remove.bounds.x - input_end;
            assert!(
                (0.0..=BUTTON_INSET + 8.0).contains(&gap),
                "{state}: Remove's label starts {gap} past its row's last input"
            );
        }
    }
}

fn encloses(outer: &Rectangle, inner: &Rectangle) -> bool {
    outer.x <= inner.x + 0.5
        && outer.y <= inner.y + 0.5
        && outer.x + outer.width + 0.5 >= inner.x + inner.width
        && outer.y + outer.height + 0.5 >= inner.y + inner.height
}

/// Each `label`'s text bounds and its button: the smallest enclosing container
/// 30-40 px tall and at least 10 px wider than the label.
fn buttons_around(
    case: &str,
    shell: &Shell,
    labels: &[&str],
) -> Vec<(String, Rectangle, Rectangle)> {
    let mut ui = simulator(shell.view());
    let mut texts: Vec<(String, Rectangle)> = Vec::new();
    let mut boxes: Vec<Rectangle> = Vec::new();
    let _ = ui.find(|candidate: iced_test::selector::Candidate<'_>| {
        if let iced_test::selector::Candidate::Text {
            content, bounds, ..
        } = candidate
        {
            texts.push((content.to_owned(), bounds));
        } else if let iced_test::selector::Candidate::Container { bounds, .. } = candidate {
            boxes.push(bounds);
        }
        None::<()>
    });
    let mut found = Vec::new();
    for label in labels {
        let mut any = false;
        for (content, text) in texts.iter().filter(|(c, _)| c == label) {
            let button = boxes
                .iter()
                .filter(|b| {
                    encloses(b, text)
                        && b.width > text.width + 10.0
                        && (30.0..=40.0).contains(&b.height)
                })
                .min_by(|a, b| a.width.total_cmp(&b.width))
                .unwrap_or_else(|| panic!("{case}: no button around {content:?}"));
            found.push((content.clone(), *text, *button));
            any = true;
        }
        assert!(any, "{case}: no {label:?} label");
    }
    found
}

/// Every action button on every pane and modal is its label plus padding wide,
/// and the two fixed-width file buttons centre their label: a stretch at any
/// one button fails here, not only one at the shared label helper.
#[test]
fn every_action_button_hugs_its_label() {
    let mut cases: Vec<(&str, Shell, Vec<&str>)> = vec![(
        "performance",
        shell(snapshot()),
        vec!["Remove", "Add stage", "Apply", "Revert"],
    )];
    let mut pulse = snapshot();
    pulse.effect = Some(Effect::Pulse {
        rate: Rate::new(40).unwrap(),
        colors: vec![rgb("ff0000"), rgb("00ff00"), rgb("0000ff")],
    });
    cases.push((
        "lighting pulse",
        on_section(pulse, Section::Lighting),
        vec!["Add colour", "Remove"],
    ));
    let mut make_active = shell(snapshot());
    make_active.machine.update(Msg::PickProfile(profile(3)));
    make_active.machine.update(Msg::Loaded(Ok(Snapshot {
        profile: profile(3),
        ..snapshot()
    })));
    cases.push(("make active", make_active, vec!["Make 3 active"]));
    let mut machine = State::new().0;
    machine.update(Msg::Loaded(Ok(snapshot())));
    machine.update(Msg::Edit(Edit::Debounce(0)));
    machine.update(Msg::Select(Section::Lighting));
    cases.push((
        "discard modal",
        shell_with_machine(machine),
        vec!["Keep editing", "Discard"],
    ));
    let mut import = shell(snapshot());
    import
        .machine
        .update(Msg::Import(std::path::PathBuf::from("/o.toml")));
    import
        .machine
        .update(Msg::ImportChecked(Ok(crate::gui::state::ImportCheck {
            note: None,
            device_wide: true,
            writes: Vec::new(),
        })));
    cases.push(("import modal", import, vec!["Cancel", "Import"]));

    let mut wrong = Vec::new();
    for (case, shell, labels) in &cases {
        for (content, text, button) in buttons_around(case, shell, labels) {
            if button.width - text.width > 22.0 {
                wrong.push(format!(
                    "{case} {content:?}: button {:.1} for label {:.1}",
                    button.width, text.width
                ));
            }
        }
    }
    let mut three = snapshot();
    three.profile = profile(3);
    three.active = profile(3);
    for (case, snap) in [("device p2", snapshot()), ("device p3", three)] {
        let shell = on_section(snap, Section::Device);
        let id = shell.machine.live_snapshot().unwrap().profile.get();
        let labels = [
            format!("Export profile {id}"),
            format!("Import into profile {id}"),
        ];
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        for (content, text, button) in buttons_around(case, &shell, &refs) {
            let dx = (text.x + text.width / 2.0) - (button.x + button.width / 2.0);
            let dy = (text.y + text.height / 2.0) - (button.y + button.height / 2.0);
            if (button.width - FILE_BUTTON_WIDTH).abs() > 0.5
                || dx.abs() > 1.0
                || dy.abs() > 1.0
                || text.height > 20.0
            {
                wrong.push(format!(
                    "{case} {content:?}: width {:.1}, off centre {dx:.2},{dy:.2}, label {:.1} tall",
                    button.width, text.height
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// The footer's last text (its notice lines, else Revert and Apply) ends this
/// close to the window's bottom edge when the footer is pinned there.
const FOOTER_REACH: f32 = 40.0;

/// Performance with the outside-change banner and two moved lines under it:
/// the tallest footer and the tallest chrome the window draws.
fn banner_and_lines() -> Shell {
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
    assert!(machine.outside_change());
    assert_eq!(machine.changed().len(), 2);
    shell_with_machine(machine)
}

/// At the window's shipped size, every pane with a footer keeps Revert, Apply
/// and the moved lines inside the window, pinned to its bottom edge: the cards
/// scroll, the footer never does (gui.md §Behaviour).
#[test]
fn the_footer_stays_pinned_inside_the_shipped_window_on_every_pane() {
    let height = window_settings().size.height;
    let mut cases: Vec<(String, Shell)> = panes()
        .into_iter()
        .filter(|(_, section, _)| *section != Section::Device)
        .map(|(name, _, shell)| (name, shell))
        .collect();
    cases.push((
        "Performance banner and lines".to_owned(),
        banner_and_lines(),
    ));
    let mut wrong = Vec::new();
    for (case, shell) in &cases {
        let placed = texts(shell);
        let footer: Vec<&Placed> = placed
            .iter()
            .filter(|p| {
                p.content == "Revert"
                    || p.content == "Apply"
                    || p.content.contains("changed on the mouse to")
            })
            .collect();
        let has = |content: &str| footer.iter().any(|p| p.content == content);
        if !has("Revert") || !has("Apply") {
            wrong.push(format!("{case}: no Revert or Apply"));
            continue;
        }
        let bottom = footer
            .iter()
            .map(|p| p.bounds.y + p.bounds.height)
            .fold(f32::NEG_INFINITY, f32::max);
        if bottom > height + SLACK || bottom < height - FOOTER_REACH {
            wrong.push(format!(
                "{case}: the footer ends at {bottom:.1} in a {height} tall window"
            ));
        }
    }
    let lines = texts(&cases[cases.len() - 1].1)
        .into_iter()
        .filter(|p| p.content.contains("changed on the mouse to"))
        .count();
    assert!(lines >= 1, "the moved lines are drawn");
    assert!(wrong.is_empty(), "{wrong:#?}");
}

// --- the sidebar and the controls ----------------------------------------------

/// Everything the simulator reports left of the pane: texts, and containers
/// with the id they carry (`None` for buttons and unnamed containers).
fn sidebar(shell: &Shell) -> (Vec<Placed>, Vec<(Option<iced::widget::Id>, Rectangle)>) {
    let mut ui = simulator(shell.view());
    let mut texts = Vec::new();
    let mut boxes = Vec::new();
    let _ = ui.find(|candidate: iced_test::selector::Candidate<'_>| {
        match candidate {
            iced_test::selector::Candidate::Text {
                content, bounds, ..
            } if bounds.x < PANE_LEFT => texts.push(Placed {
                content: content.to_owned(),
                bounds,
            }),
            iced_test::selector::Candidate::Container { id, bounds, .. }
                if bounds.x < PANE_LEFT =>
            {
                boxes.push((id.cloned(), bounds));
            }
            _ => {}
        }
        None::<()>
    });
    (texts, boxes)
}

fn by_id(boxes: &[(Option<iced::widget::Id>, Rectangle)], id: &'static str) -> Vec<Rectangle> {
    let want = iced::widget::Id::new(id);
    boxes
        .iter()
        .filter(|(found, _)| found.as_ref() == Some(&want))
        .map(|(_, bounds)| *bounds)
        .collect()
}

/// The window on profile 3 while profile 2 stays the active one.
fn editing_three() -> Shell {
    let mut shell = shell(snapshot());
    shell.machine.update(Msg::PickProfile(profile(3)));
    shell.machine.update(Msg::Loaded(Ok(Snapshot {
        profile: profile(3),
        ..snapshot()
    })));
    shell
}

/// The profile picker is one segmented control: three equal segments side by
/// side, each digit's glyph box centred in its segment (the session-8 frame
/// had the selected digit ~3 px left of centre).
#[test]
fn each_profile_digit_sits_centred_in_one_segmented_control() {
    let mut wrong = Vec::new();
    for (case, shell) in [
        ("edited 2", shell(snapshot())),
        ("edited 3", editing_three()),
    ] {
        let (texts, boxes) = sidebar(&shell);
        let mut segments: Vec<Rectangle> = [
            "edited-profile",
            "profile-pick-1",
            "profile-pick-2",
            "profile-pick-3",
        ]
        .into_iter()
        .flat_map(|id| by_id(&boxes, id))
        .collect();
        assert_eq!(segments.len(), 3, "{case}: segments {segments:?}");
        segments.sort_by(|a, b| a.x.total_cmp(&b.x));
        for (index, segment) in segments.iter().enumerate() {
            let digit = (index + 1).to_string();
            let inside: Vec<&Placed> = texts
                .iter()
                .filter(|p| p.content == digit && encloses(segment, &p.bounds))
                .collect();
            assert_eq!(inside.len(), 1, "{case}: digit {digit} in {segment:?}");
            let glyphs = inside[0].bounds;
            if glyphs.width >= segment.width / 2.0 {
                wrong.push(format!(
                    "{case} {digit}: text box {:.1} wide in a {:.1} segment, not its glyphs",
                    glyphs.width, segment.width
                ));
            }
            let dx = (glyphs.x + glyphs.width / 2.0) - (segment.x + segment.width / 2.0);
            let dy = centre_y(&glyphs) - centre_y(segment);
            if dx.abs() > 0.5 || dy.abs() > 0.5 {
                wrong.push(format!("{case} {digit}: off centre by {dx:.2},{dy:.2}"));
            }
        }
        for pair in segments.windows(2) {
            let (left, right) = (pair[0], pair[1]);
            let gap = right.x - (left.x + left.width);
            if !(-0.5..=2.0).contains(&gap)
                || (left.width - right.width).abs() > 0.5
                || (left.height - right.height).abs() > 0.5
                || (left.y - right.y).abs() > 0.5
            {
                wrong.push(format!(
                    "{case}: segments {left:?} and {right:?} are not one control"
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// The sidebar's header, its eyebrows and its section rows share one left
/// edge (the session-8 frame had the header at x 16 and the rows' text at
/// x 50): the model name, the link eyebrow, the `Profile` and `Settings`
/// eyebrows and the section rows' dot start at one x, and every row's label
/// starts just right of the dot.
#[test]
fn the_sidebar_header_eyebrows_and_section_rows_share_one_left_edge() {
    for (case, shell) in [
        ("edited 2", shell(snapshot())),
        ("edited 3", editing_three()),
    ] {
        let (texts, boxes) = sidebar(&shell);
        let mut lefts = Vec::new();
        for content in [
            "Model O Wireless".to_owned(),
            eyebrow("Wireless"),
            eyebrow("Profile"),
            eyebrow("Settings"),
        ] {
            let found: Vec<&Placed> = texts.iter().filter(|p| p.content == content).collect();
            assert_eq!(
                found.len(),
                1,
                "{case}: {content:?} found {} times",
                found.len()
            );
            lefts.push((format!("{case} {content:?}"), found[0].bounds.x));
        }
        let dot = by_id(&boxes, "section-dot");
        assert_eq!(dot.len(), 1, "{case}: section dots {dot:?}");
        lefts.push((format!("{case} section dot"), dot[0].x));
        all_agree("sidebar left edge", &lefts);

        let dot_right = dot[0].x + dot[0].width;
        let mut labels = Vec::new();
        for section in SECTIONS {
            let label = match section {
                Section::Performance => "Performance",
                Section::Lighting => "Lighting",
                Section::Buttons => "Buttons",
                Section::Device => "Device",
            };
            let found: Vec<&Placed> = texts.iter().filter(|p| p.content == label).collect();
            assert_eq!(
                found.len(),
                1,
                "{case}: {label:?} found {} times",
                found.len()
            );
            let x = found[0].bounds.x;
            assert!(
                (dot_right..=dot_right + 16.0).contains(&x),
                "{case}: {label:?} starts at {x}, the dot ends at {dot_right}"
            );
            labels.push((format!("{case} {label:?}"), x));
        }
        all_agree("section label column", &labels);
    }
}

/// Each pane's card titles, as eyebrows.
fn card_titles(section: Section) -> [&'static str; 2] {
    match section {
        Section::Performance => ["DPI", "Sensor"],
        Section::Lighting => ["Effect", "Brightness"],
        Section::Buttons => ["Scroll", "Buttons"],
        Section::Device => ["Mouse", "Profile file"],
    }
}

/// Every card title is a caps eyebrow starting in the label column, and the
/// DPI card has no `DPI stages` row repeating its title: the stage list's
/// `per profile` tag rides the title's row instead.
#[test]
fn card_titles_are_eyebrows_in_the_label_column() {
    let mut lefts = Vec::new();
    for (pane, section, shell) in panes() {
        let placed = texts(&shell);
        for title in card_titles(section) {
            let title = eyebrow(title);
            let found: Vec<&Placed> = placed.iter().filter(|p| p.content == title).collect();
            assert_eq!(
                found.len(),
                1,
                "{pane}: {title:?} found {} times",
                found.len()
            );
            lefts.push((format!("{pane} {title:?}"), found[0].bounds.x));
        }
        let first = labels(section)[0];
        lefts.push((format!("{pane} {first:?}"), one(&placed, first).bounds.x));
        if section == Section::Performance {
            assert!(
                placed.iter().all(|p| p.content != "DPI stages"),
                "{pane}: a DPI stages row"
            );
            let dpi = one(&placed, &eyebrow("DPI"));
            let y = centre_y(&dpi.bounds);
            assert!(
                placed
                    .iter()
                    .any(|p| p.content == "per profile" && (centre_y(&p.bounds) - y).abs() < 6.0),
                "{pane}: no per profile tag on the DPI title's row"
            );
        }
    }
    all_agree("card title column", &lefts);
}

/// The Performance pane's single-value controls share one width and start in
/// the control column: each stage's DPI input, the active stage, polling and
/// lift-off pickers (the session-8 frame had them at 110 / 150 / 108 / 85 px).
#[test]
fn the_performance_value_controls_share_one_width() {
    for (state, snap) in [("empty", snapshot()), ("remembered", remembered())] {
        let shell = on_section(snap, Section::Performance);
        let placed = texts(&shell);
        let mut ui = simulator(shell.view());
        let mut boxes: Vec<(Option<iced::widget::Id>, Rectangle)> = Vec::new();
        let _ = ui.find(|candidate: iced_test::selector::Candidate<'_>| {
            if let iced_test::selector::Candidate::Container { id, bounds, .. } = candidate {
                boxes.push((id.cloned(), bounds));
            }
            None::<()>
        });
        let mut controls: Vec<(String, Rectangle)> = Vec::new();
        for stage in ["Stage 1", "Stage 2", "Stage 3"] {
            let y = centre_y(&one(&placed, stage).bounds);
            let first = inputs(&shell)
                .into_iter()
                .filter(|bounds| (centre_y(bounds) - y).abs() < 12.0)
                .min_by(|a, b| a.x.total_cmp(&b.x));
            let Some(first) = first else {
                panic!("{state}: no input on the {stage:?} row");
            };
            controls.push((format!("{state} {stage:?} input"), first));
        }
        for id in ["active-stage-picker", "polling-picker", "lift-off-picker"] {
            let found = by_id(&boxes, id);
            assert_eq!(found.len(), 1, "{state}: {id} found {} times", found.len());
            controls.push((format!("{state} {id}"), found[0]));
        }
        let widths: Vec<(String, f32)> = controls
            .iter()
            .map(|(name, bounds)| (name.clone(), bounds.width))
            .collect();
        all_agree("value control width", &widths);
        let lefts: Vec<(String, f32)> = controls
            .iter()
            .map(|(name, bounds)| (name.clone(), bounds.x))
            .collect();
        all_agree("value control left edge", &lefts);
    }
}
