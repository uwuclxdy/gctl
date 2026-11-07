#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The open panel's geometry and its own motion: the side is one decision from
//! screen-space bounds, the panel stands clear of its trigger and never covers
//! it, its height is capped and its rows scroll inside a gutter with a rail,
//! and the mark, the row costume and the shadow each stay their own colour.

use std::time::Duration;

use iced::time::Instant;
use iced::touch;
use iced::widget::{container, text};

use super::*;
use crate::gui::control::Control;
use crate::gui::motion::Prefs;
use crate::gui::state::{Edit, Msg, Section};
use crate::gui::testutil::{
    SETTLED, click, on_section, redraw, settle, simulator, simulator_sized, snapshot,
};
use crate::gui::view::Message;

use gctl_proto::Button;

fn trigger(y: f32) -> Rectangle {
    Rectangle {
        x: 100.0,
        y,
        width: 150.0,
        height: ROW_HEIGHT,
    }
}

const WINDOW: Size = Size::new(1024.0, 680.0);

#[test]
fn a_panel_opens_below_when_the_room_below_holds_it() {
    assert!(opens_below_for(trigger(100.0), 90, WINDOW));
    assert!(!opens_below_for(trigger(600.0), 90, WINDOW));
    // A short panel fits where the tallest one would not: the layout decides
    // on the panel's own height.
    let low = trigger(480.0);
    assert!(opens_below_for(low, 4, WINDOW));
    assert!(!opens_below_for(low, 90, WINDOW));
}

#[test]
fn a_long_list_is_capped_at_the_selects_height() {
    assert_eq!(
        panel_size(trigger(100.0), 90, WINDOW).height,
        MAX_PANEL_HEIGHT
    );
    // A short list's panel is its rows plus the gutter, so no row is clipped.
    assert_eq!(
        panel_size(trigger(100.0), 4, WINDOW).height,
        4.0 * ROW_HEIGHT + 2.0 * PANEL_GUTTER
    );
    assert_eq!(panel_size(trigger(100.0), 90, WINDOW).width, 150.0);
    // a window shorter than the select's height still holds the panel
    assert_eq!(
        panel_size(trigger(0.0), 90, Size::new(1024.0, 100.0)).height,
        100.0
    );
}

/// The panel is its own surface: it starts [`PANEL_GAP`] past its trigger on
/// the side it opens to, and its place stays inside the window.
#[test]
fn the_panel_stands_clear_of_its_trigger() {
    let below = panel_at(trigger(100.0), 90, true, WINDOW);
    assert_eq!(below.y, 100.0 + ROW_HEIGHT + PANEL_GAP);
    assert_eq!(below.height, MAX_PANEL_HEIGHT);
    let above = panel_at(trigger(500.0), 90, false, WINDOW);
    assert_eq!(above.y + above.height, 500.0 - PANEL_GAP);
    assert_eq!(above.height, MAX_PANEL_HEIGHT);
    // However short the window, the panel stays clear and inside it.
    for (height, y) in [(400.0, 150.0), (420.0, 40.0), (420.0, 340.0)] {
        let window = Size::new(1024.0, height);
        let t = trigger(y);
        let placed = panel_at(t, 90, opens_below_for(t, 90, window), window);
        let overlap = placed.intersection(&t).map_or(0.0, |r| r.height);
        assert!(
            overlap <= 0.0,
            "window {height}, trigger y {y}: covers its trigger"
        );
        assert!(
            placed.y >= 0.0 && placed.y + placed.height <= height + 0.5,
            "window {height}, trigger y {y}: the panel leaves the window"
        );
    }
}

#[test]
fn the_panel_is_clamped_inside_the_window() {
    let wide = Rectangle {
        x: 950.0,
        width: 200.0,
        ..trigger(100.0)
    };
    assert_eq!(panel_rect(wide, 4, true, WINDOW).x, 1024.0 - 200.0);

    let short = Size::new(1024.0, 100.0);
    let panel = panel_rect(trigger(0.0), 90, false, short);
    assert_eq!(panel.y, 0.0);
    assert!(panel.y + panel.height <= short.height);
    // A panel below is capped by the room below it, never across the field.
    let capped = panel_rect(trigger(250.0), 90, true, short);
    assert_eq!(capped.y, 250.0 + ROW_HEIGHT);
    assert_eq!(capped.height, 0.0);
}

#[test]
fn only_a_list_taller_than_the_panel_scrolls() {
    let panel = panel_at(trigger(100.0), 90, true, WINDOW);
    assert_eq!(
        max_scroll(90, panel),
        90.0 * ROW_HEIGHT - content_rect(panel).height
    );
    assert!(max_scroll(90, panel) > 0.0);
    assert_eq!(
        max_scroll(4, panel_at(trigger(100.0), 4, true, WINDOW)),
        0.0
    );
}

#[test]
fn the_highlight_is_scrolled_just_into_view() {
    let panel = panel_at(trigger(100.0), 90, true, WINDOW);
    let view = content_rect(panel).height;
    // already visible: no movement
    assert_eq!(scroll_into_view(panel, 90, 5, 0.0), 0.0);
    // the first row past the bottom edge lands on the edge
    assert_eq!(scroll_into_view(panel, 90, 6, 0.0), 7.0 * ROW_HEIGHT - view);
    // the last row lands at the panel's bottom
    assert_eq!(
        scroll_into_view(panel, 90, 89, 0.0),
        90.0 * ROW_HEIGHT - view
    );
    // moving up to a row above the window brings its top to the edge
    assert_eq!(scroll_into_view(panel, 90, 2, 1000.0), 2.0 * ROW_HEIGHT);
}

#[test]
fn the_thumb_is_drawn_only_while_the_list_overflows() {
    let panel = panel_at(trigger(100.0), 90, true, WINDOW);
    assert!(thumb_rect(panel, 4, 0.0).is_none());
    let content = content_rect(panel);
    let max = max_scroll(90, panel);
    let top = thumb_rect(panel, 90, 0.0).unwrap();
    let end = thumb_rect(panel, 90, max).unwrap();
    assert!(top.y >= content.y && end.y + end.height <= content.y + content.height + 0.5);
    assert!(top.x >= content.x && top.x + top.width <= content.x + content.width);
    assert!(end.y > top.y, "the thumb never moved with the offset");
    assert!(
        (top.height - (content.height * content.height / (90.0 * ROW_HEIGHT)).max(THUMB_MIN)).abs()
            < 0.5,
        "the thumb is not proportional"
    );
}

/// The scroll rail is a lane of its own, the list's full height whenever the
/// list scrolls: a press anywhere in it picks nothing, while the row beside it
/// at the same height picks its own option.
#[test]
fn the_thumb_rail_is_not_a_rows_hit_area() {
    let panel = panel_at(trigger(100.0), 90, true, WINDOW);
    let content = content_rect(panel);
    let mid = max_scroll(90, panel) / 2.0;
    let thumb = thumb_rect(panel, 90, mid).unwrap();
    let lane = rail_lane(panel, 90, mid).unwrap();
    assert_eq!(lane.x, thumb.x);
    assert_eq!(lane.width, thumb.width);
    assert_eq!(lane.y, content.y);
    assert_eq!(lane.height, content.height);
    for y in [thumb.y - 4.0, thumb.y + thumb.height + 4.0] {
        let on_rail = Point::new(thumb.center_x(), y);
        assert!(content.contains(on_rail), "{on_rail:?} misses the list");
        assert_eq!(
            row_at(panel, 90, mid, on_rail),
            None,
            "a press on the rail beside the thumb picked a row"
        );
        let beside = Point::new(thumb.x - 8.0, y);
        assert!(
            row_at(panel, 90, mid, beside).is_some(),
            "the row beside the rail at {y} picked nothing"
        );
    }
    for scroll in [0.0, max_scroll(90, panel)] {
        let thumb = thumb_rect(panel, 90, scroll).unwrap();
        assert_eq!(row_at(panel, 90, scroll, thumb.center()), None);
    }
}

/// A list that fits the panel has no rail: its rows keep their full width and
/// pick along all of it.
#[test]
fn a_list_that_fits_has_no_rail_lane() {
    let panel = panel_at(trigger(100.0), 4, true, WINDOW);
    let content = content_rect(panel);
    assert!(rail_lane(panel, 4, 0.0).is_none());
    let row = Rectangle {
        y: content.y,
        height: ROW_HEIGHT,
        ..content
    };
    let far = Point::new(content.x + content.width - 6.0, row.center_y());
    assert_eq!(row_at(panel, 4, 0.0, far), Some(0));
    assert_eq!(row_fill(row, false).width, row.width - 8.0);
}

/// A scrolling list ends its row fill before the rail, so the thumb never
/// draws over a row's fill.
#[test]
fn a_rows_fill_ends_before_the_rail() {
    let panel = panel_at(trigger(100.0), 90, true, WINDOW);
    let content = content_rect(panel);
    let lane = rail_lane(panel, 90, 0.0).unwrap();
    let row = Rectangle {
        y: content.y,
        height: ROW_HEIGHT,
        ..content
    };
    let filled = row_fill(row, true);
    assert_eq!(filled.x + filled.width, lane.x);
    assert_eq!(
        row_fill(row, false).x + row_fill(row, false).width,
        row.x + row.width - 4.0
    );
}

/// A press on a row's boundary is the row below it, as the list has no gaps,
/// and a press outside the list picks nothing.
#[test]
fn a_press_outside_the_list_picks_nothing() {
    let panel = panel_at(trigger(100.0), 90, true, WINDOW);
    let content = content_rect(panel);
    for index in [0usize, 3, 5] {
        assert_eq!(
            row_at(
                panel,
                90,
                0.0,
                iced::Point::new(content.center_x(), content.y + index as f32 * ROW_HEIGHT)
            ),
            Some(index),
            "a press on the boundary above row {index}"
        );
    }
    assert_eq!(
        row_at(
            panel,
            90,
            0.0,
            iced::Point::new(content.x - 1.0, content.center_y())
        ),
        None
    );
    assert_eq!(
        row_at(
            panel,
            90,
            0.0,
            iced::Point::new(content.center_x(), content.y + content.height + 1.0)
        ),
        None
    );
    assert_eq!(
        row_at(
            panel,
            4,
            0.0,
            iced::Point::new(content.center_x(), content.y + 4.5 * ROW_HEIGHT)
        ),
        None,
        "a press below the last row picked one"
    );
}

/// The thumb rests in the pane scrollable's rail colours, one tone below the
/// draggable scroller, and inside the panel the rows travel in.
#[test]
fn the_thumb_rests_in_the_rails_quiet_colour() {
    let t = *Scheme::Dark.tokens();
    assert_eq!(thumb_fill(t), t.surface_2);
    assert_ne!(thumb_fill(t), t.surface_3);
    // Drawn in the panel's own coordinates: the panel's slide carries it.
    let panel = panel_at(trigger(100.0), 90, true, WINDOW);
    let thumb = thumb_rect(panel, 90, 0.0).unwrap();
    assert!(content_rect(panel).contains(thumb.center()));
}

/// The current value keeps its accent-quiet mark whatever the pointer does: the
/// hover costume never paints on the selected row.
#[test]
fn the_selected_row_keeps_its_mark_under_the_pointer() {
    let t = *Scheme::Dark.tokens();
    for hover in [0.0, 0.4, 1.0] {
        assert_eq!(
            row_paint(t, true, hover),
            (Some(t.accent_quiet), t.accent_text),
            "selected row at hover {hover}"
        );
    }
    assert_eq!(row_paint(t, false, 0.0), (None, t.text));
    assert_eq!(
        row_paint(t, false, 1.0),
        (Some(t.surface_2), t.text),
        "the pointer's costume on another row"
    );
}

/// The panel's shadow always falls downward, whichever side it opened to: the
/// light direction never flips for one surface.
#[test]
fn the_panel_shadow_falls_downward() {
    let t = *Scheme::Dark.tokens();
    let shadow = panel_shadow(t);
    assert_eq!(shadow.offset.y, SHADOW_OFFSET);
    assert!(shadow.offset.y > 0.0);
    assert_eq!(shadow.offset.x, 0.0);
    assert_eq!(shadow.blur_radius, SHADOW_BLUR);
    assert_eq!(shadow.color, t.shadow);
}

#[test]
fn the_caret_is_quiet_at_rest_and_one_step_up_on_hover() {
    let t = *Scheme::Dark.tokens();
    assert_eq!(caret_colour(t, 0.0), t.text_4);
    assert_eq!(caret_colour(t, 1.0), t.text_3);
    let mid = caret_colour(t, 0.5);
    assert!(mid.r < t.text_3.r && mid.r > t.text_4.r);
}

/// One redraw of the open list: the costume follows the highlight, then every
/// row's fade takes the instant. The panel's own update does exactly this.
fn tick(state: &mut State, options: usize, prefs: Prefs, at: Instant) {
    state.follow_rows(options, prefs);
    for fade in state.rows.values_mut() {
        fade.observe(at);
    }
}

/// A row the highlight leaves fades out on the exit length while the row it
/// takes fades in on the tint: at `exit(TINT)` the leaving row's costume is
/// gone, where a shared tint-length cross-fade would still draw it.
#[test]
fn a_leaving_row_fades_out_on_the_exit_length() {
    let prefs = Prefs::default();
    let at = Instant::now() + Duration::from_secs(60);
    let mut state = State::new();
    state.highlight = Some(1);
    tick(&mut state, 4, prefs, at);
    tick(
        &mut state,
        4,
        prefs,
        at + motion::TINT + Duration::from_millis(1),
    );
    assert_eq!(state.row_alpha(1), 1.0);

    let moved = at + 10 * motion::TINT;
    state.highlight = Some(2);
    tick(&mut state, 4, prefs, moved);
    assert_eq!(state.row_alpha(1), 1.0, "the old row jumped off its mark");
    assert_eq!(state.row_alpha(2), 0.0, "the new row jumped on");
    let exit = motion::exit(motion::TINT);
    tick(&mut state, 4, prefs, moved + Duration::from_millis(40));
    assert!(state.row_alpha(2) > 0.0, "the new row never faded in");
    tick(
        &mut state,
        4,
        prefs,
        moved + exit + Duration::from_millis(1),
    );
    assert_eq!(state.row_alpha(1), 0.0, "the costume outlived its exit");
    tick(
        &mut state,
        4,
        prefs,
        moved + motion::TINT + Duration::from_millis(1),
    );
    assert_eq!(state.row_alpha(2), 1.0);
}

/// An open prompt settles every fade at rest, so the panel never draws above
/// the scrim and comes back shut.
#[test]
fn a_prompt_settles_the_panel_shut() {
    let prefs = Prefs::default();
    let at = Instant::now() + Duration::from_secs(60);
    let mut state = State::new();
    state.fold.open(motion::NEAR, Curve::EaseOut);
    state.fold.observe(at);
    state.highlight = Some(1);
    tick(&mut state, 4, prefs, at);
    tick(
        &mut state,
        4,
        prefs,
        at + motion::TINT + Duration::from_millis(1),
    );
    assert_eq!(state.row_alpha(1), 1.0);
    state.settle();
    assert_eq!(state.fold.value(), 1.0, "the panel came back open");
    assert!(!state.fold.animating());
    assert_eq!(state.edge.value(), 0.0);
    assert_eq!(state.row_alpha(1), 0.0);
}

/// The open panel shows the pointer only over a row it can pick, and the idle
/// cursor over its gutter and rail: claimed, so nothing under the panel draws
/// as if the pointer were its own.
#[test]
fn the_panel_claims_the_pointer_only_over_a_row() {
    let panel = panel_at(trigger(100.0), 90, true, WINDOW);
    let content = content_rect(panel);
    let lane = rail_lane(panel, 90, 0.0).unwrap();
    let row = Point::new(content.center_x(), content.y + ROW_HEIGHT / 2.0);
    let gutter = Point::new(panel.x + 2.0, panel.center_y());
    let over = |at: Point| mouse::Cursor::Available(at);
    assert_eq!(
        panel_cursor(panel, 90, 0.0, true, over(row)),
        mouse::Interaction::Pointer
    );
    assert_eq!(
        panel_cursor(panel, 90, 0.0, true, over(gutter)),
        mouse::Interaction::Idle
    );
    assert_eq!(
        panel_cursor(panel, 90, 0.0, true, over(lane.center())),
        mouse::Interaction::Idle
    );
    assert_eq!(
        panel_cursor(
            panel,
            90,
            0.0,
            true,
            over(Point::new(panel.x - 2.0, panel.center_y()))
        ),
        mouse::Interaction::None
    );
    assert_eq!(
        panel_cursor(panel, 90, 0.0, false, over(row)),
        mouse::Interaction::None
    );
}

/// The open panel's frame: the row it first shows, its trigger and the widths
/// the panel opens over.
const PICKER: Size = Size::new(180.0, 36.0);
const WINDOW_SPACE: Size = Size::new(300.0, 400.0);

fn picker_view() -> Element<'static, Message> {
    let prefs = Prefs::default();
    let face = container(text("pick").size(14))
        .padding([0.0, 12.0])
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);
    let trigger = Control::new(face, theme::field_surface(*Scheme::Dark.tokens()), prefs)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(0);
    container(Dropdown::new(
        Paint {
            scheme: Scheme::Dark,
            alpha: 1.0,
            frozen: false,
        },
        prefs,
        trigger,
        vec!["one".to_owned(), "two".to_owned(), "three".to_owned()],
        Some(0),
        Font::DEFAULT,
        |_| Some(Message::Machine(Msg::Tick)),
    ))
    .width(PICKER.width)
    .height(PICKER.height)
    .into()
}

fn finger(event: touch::Event) -> iced::Event {
    iced::Event::Touch(event)
}

fn cursor_moved(at: Point) -> iced::Event {
    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: at })
}

/// A tap opens the list like a click, and a tap on one of its rows picks it:
/// only the open panel can publish a pick, so the message proves the tap
/// opened the list.
#[test]
fn a_tap_opens_the_list_and_picks_a_row() {
    let base = Instant::now() + Duration::from_secs(3600);
    let mut ui = simulator_sized(picker_view(), WINDOW_SPACE);
    let at = Point::new(PICKER.width / 2.0, PICKER.height / 2.0);
    ui.point_at(at);
    let _ = ui.simulate([cursor_moved(at)]);
    let _ = ui.simulate([finger(touch::Event::FingerPressed {
        id: touch::Finger(0),
        position: at,
    })]);
    let _ = ui.simulate([finger(touch::Event::FingerLifted {
        id: touch::Finger(0),
        position: at,
    })]);
    let _ = ui.simulate([redraw(base), redraw(base + SETTLED)]);

    let row = Point::new(
        at.x,
        PICKER.height + PANEL_GAP + PANEL_GUTTER + ROW_HEIGHT / 2.0,
    );
    ui.point_at(row);
    let _ = ui.simulate([finger(touch::Event::FingerPressed {
        id: touch::Finger(0),
        position: row,
    })]);
    let _ = ui.simulate([finger(touch::Event::FingerLifted {
        id: touch::Finger(0),
        position: row,
    })]);
    let picks = ui
        .into_messages()
        .filter(|message| matches!(message, Message::Machine(Msg::Tick)))
        .count();
    assert_eq!(picks, 1, "a tap on a row picked nothing");
}

/// A list reopened while its close still draws keeps the costume on screen and
/// fades it; one opened after the panel had gone starts every row at rest.
#[test]
fn a_reopened_list_keeps_the_costume_a_fresh_open_clears_it() {
    let prefs = Prefs::default();
    let at = Instant::now() + Duration::from_secs(60);
    let mut state = State::new();
    state.highlight = Some(1);
    tick(&mut state, 4, prefs, at);
    tick(
        &mut state,
        4,
        prefs,
        at + motion::TINT + Duration::from_millis(1),
    );
    assert_eq!(state.row_alpha(1), 1.0);
    state.fold.close(motion::NEAR, Curve::Exit);
    state.fold.observe(at + 2 * motion::NEAR);
    assert!(state.fold.needed(), "the close no longer draws");
    state.open(Some(0), motion::NEAR, motion::TINT);
    assert_eq!(
        state.row_alpha(1),
        1.0,
        "the closing panel's costume was dropped in one frame"
    );

    let mut fresh = State::new();
    fresh.highlight = Some(1);
    tick(&mut fresh, 4, prefs, at);
    tick(
        &mut fresh,
        4,
        prefs,
        at + motion::TINT + Duration::from_millis(1),
    );
    assert_eq!(fresh.row_alpha(1), 1.0);
    fresh.open(Some(0), motion::NEAR, motion::TINT);
    assert_eq!(
        fresh.row_alpha(1),
        0.0,
        "a costume from an earlier open survived"
    );
}

/// After a wheel step the highlight follows the row under the pointer, so
/// Enter picks what the pointer points at, not what it pointed at before the
/// list moved.
#[test]
fn a_wheel_step_moves_the_highlight_with_the_row_under_the_pointer() {
    let mut snap = snapshot();
    snap.binds[0] = (
        Button::Left,
        Some(crate::parse::binding("key:KeyA").unwrap()),
    );
    let shell = on_section(snap, Section::Buttons);
    let base = Instant::now() + Duration::from_secs(3600);
    let mut ui = simulator(shell.view());
    let trigger = ui
        .find(iced::widget::Id::new("key-picker-left"))
        .expect("no key picker")
        .visible_bounds()
        .expect("the key picker is not visible");
    let at = iced::Point::new(trigger.center_x(), trigger.center_y());
    ui.point_at(at);
    ui.simulate(click());
    settle(&mut ui, base);

    // The open panel: 90 rows capped to 240 px, held the panel gap clear of
    // the trigger below it, with its 4 px gutter.
    let panel = iced::Rectangle {
        x: trigger.x,
        y: trigger.y + trigger.height + PANEL_GAP,
        width: trigger.width,
        height: 240.0,
    };
    // Row 3 (KeyD) under the pointer, one wheel line down: row 4 (KeyE).
    let row = iced::Point::new(
        panel.x + panel.width / 2.0,
        panel.y + 4.0 + 3.5 * ROW_HEIGHT,
    );
    ui.point_at(row);
    ui.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
        position: row,
    })]);
    ui.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
        delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
    })]);
    let _ = ui.tap_key(iced::keyboard::key::Named::Enter);
    let expected = Edit::Bind(Button::Left, crate::parse::binding("key:KeyE").unwrap());
    let edits: Vec<Edit> = ui
        .into_messages()
        .filter_map(|message| match message {
            Message::Machine(Msg::Edit(edit)) => Some(edit),
            _ => None,
        })
        .collect();
    assert_eq!(
        edits.last(),
        Some(&expected),
        "Enter picked the row the pointer left, not the one under it"
    );
}

/// Writes the open list's frames for a by-eye check when `GCTL_PROBE_RENDER`
/// names a directory: the fade and slide at the start of an open, the settled
/// panel, and the close. Nothing runs without the variable.
#[test]
fn probe_render_fold_frames() {
    use crate::gui::state::Section;
    use crate::gui::testutil::{click, on_section, redraw, shell, simulator, snapshot};
    use crate::gui::view::Message;

    let Ok(dir) = std::env::var("GCTL_PROBE_RENDER") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let base = Instant::now() + Duration::from_secs(3600);
    for scheme in [Scheme::Dark, Scheme::Light] {
        for (picker, build, options) in
            [("polling-picker", 0u8, 4usize), ("key-picker-left", 1, 90)]
        {
            let mut shell = if build == 0 {
                shell(snapshot())
            } else {
                let mut snap = snapshot();
                snap.binds[0] = (
                    gctl_proto::Button::Left,
                    Some(crate::parse::binding("key:KeyA").unwrap()),
                );
                on_section(snap, Section::Buttons)
            };
            shell.scheme = scheme;
            let theme = scheme.theme();
            let mut ui = simulator(shell.view());
            let bounds = ui
                .find(iced::widget::Id::new(picker))
                .expect("the picker is on the pane")
                .visible_bounds()
                .expect("the picker is visible");
            let at = iced::Point::new(bounds.center_x(), bounds.center_y());
            ui.point_at(at);
            ui.simulate(click());
            let frame =
                |ui: &mut iced_test::Simulator<'_, Message, iced::Theme, iced::Renderer>,
                 name: &str,
                 at: Instant| {
                    let _ = ui.simulate([redraw(at)]);
                    let label = format!("{picker}-{name}-{scheme:?}");
                    let _ = ui.snapshot(&theme).unwrap().matches_image(dir.join(label));
                };
            for (name, offset) in [
                ("open-2ms", 2u64),
                ("open-40ms", 40),
                ("open-90ms", 90),
                ("settled", 2000),
            ] {
                frame(&mut ui, name, base + Duration::from_millis(offset));
            }
            // The close starts at the first redraw after the ask, so these
            // frames show the exit from its start through 84 of its 108 ms.
            let close = base + Duration::from_millis(2000);
            let _ = ui.tap_key(iced::keyboard::key::Named::Escape);
            for (name, offset) in [
                ("close-20ms", 20u64),
                ("close-60ms", 60),
                ("close-104ms", 104),
            ] {
                frame(&mut ui, name, close + Duration::from_millis(offset));
            }
            // A reopen while the close still draws: the costume the closing
            // panel showed fades out from where it stands.
            let again = close + Duration::from_millis(400);
            ui.point_at(at);
            ui.simulate(click());
            frame(&mut ui, "second-open", again);
            let window = crate::gui::view::window_settings().size;
            let panel = panel_at(
                bounds,
                options,
                opens_below_for(bounds, options, window),
                window,
            );
            let row =
                iced::Point::new(bounds.center_x(), panel.y + PANEL_GUTTER + 1.5 * ROW_HEIGHT);
            let opened = again + Duration::from_millis(400);
            frame(&mut ui, "second-open-settled", opened);
            ui.point_at(row);
            ui.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
                position: row,
            })]);
            let hovered = opened + Duration::from_millis(200);
            frame(&mut ui, "costume", hovered);
            let _ = ui.tap_key(iced::keyboard::key::Named::Escape);
            frame(
                &mut ui,
                "reclosing-20ms",
                hovered + Duration::from_millis(20),
            );
            ui.point_at(at);
            ui.simulate(click());
            frame(
                &mut ui,
                "reopened-40ms",
                hovered + Duration::from_millis(40),
            );
            frame(
                &mut ui,
                "reopened-100ms",
                hovered + Duration::from_millis(100),
            );
        }
    }
}
