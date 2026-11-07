//! The window driven the way the winit runtime drives it: one user interface
//! whose widget tree survives every rebuild through its cache, every message
//! routed back into the shell, the redraw request of each event batch read,
//! and frames drawn from that same tree. The simulator rebuilds its tree per
//! view and drops redraw requests, so hover and press state kept in widgets,
//! and a widget that asks for no redraw, are only visible here.

use std::time::Duration;

use iced::theme::Base as _;
use iced::time::Instant;
use iced_test::runtime::user_interface::{self, UserInterface};

use super::*;

/// Far past every duration the design uses.
const SETTLED: Duration = Duration::from_secs(10);

fn redraw(at: Instant) -> iced::Event {
    iced::Event::Window(iced::window::Event::RedrawRequested(at))
}

fn moved(position: iced::Point) -> iced::Event {
    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position })
}

/// A window's widget tree across rebuilds, its renderer and its cursor.
struct Live {
    renderer: iced::Renderer,
    cache: Option<user_interface::Cache>,
    cursor: iced::mouse::Cursor,
    /// The latest redraw instant handed to the tree.
    last: Option<Instant>,
}

impl Live {
    fn new() -> Live {
        // the guarded helper checks the backend and loads the embedded fonts
        let _ = simulator(iced::widget::space().into());
        let renderer = iced::futures::executor::block_on(
            <iced::Renderer as iced::advanced::renderer::Headless>::new(
                UI_FONT,
                iced::Pixels(16.0),
                Some("tiny-skia"),
            ),
        )
        .unwrap();
        Live {
            renderer,
            cache: None,
            cursor: iced::mouse::Cursor::Unavailable,
            last: None,
        }
    }

    /// One event batch against the shell's current view: the redraw it asks
    /// for, and every message it produced, already routed into the shell.
    fn send(&mut self, shell: &mut Shell, events: &[iced::Event]) -> iced::window::RedrawRequest {
        for event in events {
            if let iced::Event::Window(iced::window::Event::RedrawRequested(at)) = event {
                self.last = Some(self.last.map_or(*at, |last| last.max(*at)));
            }
        }
        let mut messages = Vec::new();
        let request = {
            let mut ui = UserInterface::build(
                shell.view(),
                window_settings().size,
                self.cache.take().unwrap_or_default(),
                &mut self.renderer,
            );
            let (state, _) = ui.update(
                events,
                self.cursor,
                &mut self.renderer,
                &mut iced_test::core::clipboard::Null,
                &mut messages,
            );
            self.cache = Some(ui.into_cache());
            match state {
                user_interface::State::Updated { redraw_request, .. } => redraw_request,
                user_interface::State::Outdated => iced::window::RedrawRequest::NextFrame,
            }
        };
        for message in messages {
            let _ = shell.update(message);
        }
        request
    }

    /// Moves the pointer to `at`.
    fn point(&mut self, shell: &mut Shell, at: iced::Point) {
        self.cursor = iced::mouse::Cursor::Available(at);
        let _ = self.send(shell, &[moved(at)]);
    }

    /// A redraw at `at` and one past every animation it started, with the
    /// shell's frame clock moved the same way, as the runtime's frame
    /// subscription moves it while anything animates.
    fn settle(&mut self, shell: &mut Shell, at: Instant) {
        let _ = shell.update(Message::Frame(at));
        let _ = self.send(shell, &[redraw(at)]);
        let _ = shell.update(Message::Frame(at + SETTLED));
        let _ = self.send(shell, &[redraw(at + SETTLED)]);
    }

    /// The pixels of the shell's current view, drawn from the kept tree after
    /// a redraw at the latest redraw instant, as the runtime and the simulator
    /// both hand every frame's widgets a redraw before drawing them.
    fn frame(&mut self, shell: &Shell) -> Vec<u8> {
        let theme = shell.theme();
        let base = theme.base();
        let mut ui = UserInterface::build(
            shell.view(),
            window_settings().size,
            self.cache.take().unwrap_or_default(),
            &mut self.renderer,
        );
        let at = self.last.unwrap_or_else(Instant::now);
        let _ = ui.update(
            &[redraw(at)],
            self.cursor,
            &mut self.renderer,
            &mut iced_test::core::clipboard::Null,
            &mut Vec::new(),
        );
        ui.draw(
            &mut self.renderer,
            &theme,
            &iced::advanced::renderer::Style {
                text_color: base.text_color,
            },
            self.cursor,
        );
        self.cache = Some(ui.into_cache());
        let size = window_settings().size;
        iced::advanced::renderer::Headless::screenshot(
            &mut self.renderer,
            iced::Size::new((size.width * 2.0) as u32, (size.height * 2.0) as u32),
            2.0,
            base.background_color,
        )
    }
}

/// The centre of the first text showing `label` in the shell's current view.
fn centre_of(shell: &Shell, label: &str) -> iced::Point {
    let bounds = simulator(shell.view())
        .find(label)
        .unwrap_or_else(|_| panic!("no {label:?} in the view"))
        .visible_bounds()
        .unwrap();
    iced::Point::new(bounds.center_x(), bounds.center_y())
}

/// The frame `fresh` (a shell built straight to the same visible state, so it
/// carries no pointer history of its own) draws in a window that never saw the
/// earlier pointer history, pointer at `at`, everything settled at `time`.
fn fresh_frame(mut fresh: Shell, at: iced::Point, time: Instant) -> Vec<u8> {
    let mut live = Live::new();
    live.point(&mut fresh, at);
    live.settle(&mut fresh, time);
    live.frame(&fresh)
}

/// `live`'s frame of `shell`, everything settled at `time`.
fn kept_frame(live: &mut Live, shell: &mut Shell, time: Instant) -> Vec<u8> {
    live.settle(shell, time);
    live.frame(shell)
}

/// A shell on Performance with a pending edit, so Revert and Apply are live
/// and leaving the section asks first.
fn pending() -> Shell {
    let mut shell = shell(snapshot());
    let _ = shell.update(Message::Machine(Msg::Edit(Edit::Debounce(0))));
    shell
}

/// Leaving a dropdown's trigger asks the runtime for the redraw its hover
/// fade-out needs: without it the fade waits on an unrelated redraw.
#[test]
fn leaving_a_dropdown_trigger_asks_for_a_redraw() {
    let mut shell = shell(snapshot());
    let base = Instant::now() + Duration::from_secs(3600);
    let mut ui = simulator(shell.view());
    let bounds = ui
        .find(iced::widget::Id::new("polling-picker"))
        .unwrap()
        .visible_bounds()
        .unwrap();
    drop(ui);
    let at = iced::Point::new(bounds.center_x(), bounds.center_y());
    let away = iced::Point::new(bounds.x - 120.0, bounds.center_y());
    let mut live = Live::new();
    live.cursor = iced::mouse::Cursor::Available(at);
    let enter = live.send(&mut shell, &[moved(at)]);
    assert_ne!(
        enter,
        iced::window::RedrawRequest::Wait,
        "control: entering asked for none"
    );
    live.settle(&mut shell, base);
    live.cursor = iced::mouse::Cursor::Available(away);
    let leave = live.send(&mut shell, &[moved(away)]);
    assert_ne!(
        leave,
        iced::window::RedrawRequest::Wait,
        "leaving asked for no redraw"
    );
}

/// Moving across the rows of a settled open list asks for a redraw, so the
/// row costume follows the pointer.
#[test]
fn moving_across_an_open_lists_rows_asks_for_a_redraw() {
    let mut shell = shell(snapshot());
    let base = Instant::now() + Duration::from_secs(3600);
    let mut ui = simulator(shell.view());
    let bounds = ui
        .find(iced::widget::Id::new("polling-picker"))
        .unwrap()
        .visible_bounds()
        .unwrap();
    drop(ui);
    let at = iced::Point::new(bounds.center_x(), bounds.center_y());
    let height = window_settings().size.height;
    let mut live = Live::new();
    live.point(&mut shell, at);
    let open = live.send(&mut shell, &click().collect::<Vec<_>>());
    assert_ne!(
        open,
        iced::window::RedrawRequest::Wait,
        "control: the click asked for none"
    );
    live.settle(&mut shell, base);
    let row_1 = iced::Point::new(at.x, menu_row_y(bounds, POLLING_RATES.len(), 1, height));
    live.cursor = iced::mouse::Cursor::Available(row_1);
    let _ = live.send(&mut shell, &[moved(row_1)]);
    live.settle(&mut shell, base + SETTLED + SETTLED);
    let row_2 = iced::Point::new(at.x, menu_row_y(bounds, POLLING_RATES.len(), 2, height));
    live.cursor = iced::mouse::Cursor::Available(row_2);
    let over = live.send(&mut shell, &[moved(row_2)]);
    assert_ne!(
        over,
        iced::window::RedrawRequest::Wait,
        "a row move asked for no redraw"
    );
}

/// A button disabled under the pointer and enabled again after the pointer
/// left draws no hover: click Revert, move away, edit again.
#[test]
fn a_disabled_buttons_hover_does_not_outlive_the_pointer() {
    let mut shell = pending();
    let base = Instant::now() + Duration::from_secs(3600);
    let revert = centre_of(&shell, "Revert");
    let away = iced::Point::new(revert.x, 300.0);
    let mut live = Live::new();
    live.point(&mut shell, revert);
    live.settle(&mut shell, base);
    let _ = live.send(&mut shell, &click().collect::<Vec<_>>());
    assert!(
        !shell.machine.pending(Section::Performance),
        "Revert kept the edit"
    );
    live.point(&mut shell, away);
    live.settle(&mut shell, base + 2 * SETTLED);
    let _ = shell.update(Message::Machine(Msg::Edit(Edit::Debounce(5))));
    let time = base + 4 * SETTLED;
    let kept = kept_frame(&mut live, &mut shell, time);
    let mut fresh = Shell::new(State::new().0);
    let _ = fresh.update(Message::Loaded {
        device: None,
        result: Ok(snapshot()),
    });
    let _ = fresh.update(Message::Machine(Msg::Edit(Edit::Debounce(5))));
    let mut on_revert = Shell::new(State::new().0);
    let _ = on_revert.update(Message::Loaded {
        device: None,
        result: Ok(snapshot()),
    });
    let _ = on_revert.update(Message::Machine(Msg::Edit(Edit::Debounce(5))));
    let at_rest = fresh_frame(fresh, away, time);
    assert!(
        fresh_frame(on_revert, revert, time) != at_rest,
        "control: a hovered Revert draws like a resting one"
    );
    assert!(
        kept == at_rest,
        "Revert draws hovered with the pointer elsewhere"
    );
}

/// A section row clicked into the discard prompt draws no hover once Keep
/// editing closes the prompt with the pointer away from the row.
#[test]
fn a_section_rows_hover_does_not_outlive_the_discard_prompt() {
    let mut shell = pending();
    let base = Instant::now() + Duration::from_secs(3600);
    let row = centre_of(&shell, "Lighting");
    let mut live = Live::new();
    live.point(&mut shell, row);
    live.settle(&mut shell, base);
    let _ = live.send(&mut shell, &click().collect::<Vec<_>>());
    assert!(shell.machine.prompt().is_some(), "no discard prompt");
    live.settle(&mut shell, base + 2 * SETTLED);
    let keep = centre_of(&shell, "Keep editing");
    live.point(&mut shell, keep);
    live.settle(&mut shell, base + 4 * SETTLED);
    let _ = live.send(&mut shell, &click().collect::<Vec<_>>());
    assert!(shell.machine.prompt().is_none(), "the prompt stayed");
    let time = base + 6 * SETTLED;
    let kept = kept_frame(&mut live, &mut shell, time);
    assert!(
        kept == fresh_frame(pending(), keep, time),
        "the section row (or Keep editing's spot) draws a stale hover"
    );
}

/// A section row released into the discard prompt never draws above the
/// scrim: 20 ms into its sink's return the frame is the settled modal
/// frame.
#[test]
fn a_pressed_row_never_draws_over_the_discard_prompt() {
    for scheme in [Scheme::Dark, Scheme::Light] {
        let mut shell = pending();
        shell.scheme = scheme;
        let base = Instant::now() + Duration::from_secs(3600);
        let row = centre_of(&shell, "Lighting");
        let mut live = Live::new();
        live.point(&mut shell, row);
        live.settle(&mut shell, base);
        let press = base + 2 * SETTLED;
        let _ = live.send(
            &mut shell,
            &[iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Left,
            ))],
        );
        let _ = live.send(
            &mut shell,
            &[redraw(press), redraw(press + Duration::from_millis(200))],
        );
        let _ = live.send(
            &mut shell,
            &[iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                iced::mouse::Button::Left,
            ))],
        );
        assert!(
            shell.machine.prompt().is_some(),
            "{scheme:?}: no discard prompt"
        );
        let at = press + Duration::from_millis(220);
        let _ = live.send(&mut shell, &[redraw(at)]);
        let returning = live.frame(&shell);
        let mut fresh = pending();
        fresh.scheme = scheme;
        let _ = fresh.update(Message::Machine(Msg::Select(Section::Lighting)));
        assert!(
            returning == fresh_frame(fresh, row, at),
            "{scheme:?}: the returning row drew over the modal"
        );
    }
}

/// A modal button whose click closes its own modal keeps no hover for the
/// next modal: the second discard prompt draws Discard at rest.
#[test]
fn a_closed_modals_button_keeps_no_hover() {
    let mut shell = pending();
    let base = Instant::now() + Duration::from_secs(3600);
    let _ = shell.update(Message::Machine(Msg::Select(Section::Lighting)));
    assert!(shell.machine.prompt().is_some());
    let discard = centre_of(&shell, "Discard");
    let mut live = Live::new();
    live.point(&mut shell, discard);
    live.settle(&mut shell, base);
    let _ = live.send(&mut shell, &click().collect::<Vec<_>>());
    assert!(
        shell.machine.prompt().is_none(),
        "the click did not discard"
    );
    let away = iced::Point::new(8.0, 8.0);
    live.point(&mut shell, away);
    live.settle(&mut shell, base + 2 * SETTLED);
    let _ = shell.update(Message::Machine(Msg::Edit(Edit::Rate(5))));
    let _ = shell.update(Message::Machine(Msg::Edit(Edit::BrightnessWired(9))));
    let _ = shell.update(Message::Machine(Msg::Select(Section::Performance)));
    assert!(shell.machine.prompt().is_some(), "no second prompt");
    let time = base + 4 * SETTLED;
    let kept = kept_frame(&mut live, &mut shell, time);
    let mut fresh = on_section(snapshot(), Section::Performance);
    let _ = fresh.update(Message::Machine(Msg::Select(Section::Lighting)));
    let _ = fresh.update(Message::Machine(Msg::Edit(Edit::Rate(5))));
    let _ = fresh.update(Message::Machine(Msg::Edit(Edit::BrightnessWired(9))));
    let _ = fresh.update(Message::Machine(Msg::Select(Section::Performance)));
    assert!(
        fresh.machine.prompt().is_some(),
        "no prompt on the fresh shell"
    );
    let mut on_discard = on_section(snapshot(), Section::Performance);
    let _ = on_discard.update(Message::Machine(Msg::Select(Section::Lighting)));
    let _ = on_discard.update(Message::Machine(Msg::Edit(Edit::Rate(5))));
    let _ = on_discard.update(Message::Machine(Msg::Edit(Edit::BrightnessWired(9))));
    let _ = on_discard.update(Message::Machine(Msg::Select(Section::Performance)));
    let second_discard = centre_of(&on_discard, "Discard");
    let at_rest = fresh_frame(fresh, away, time);
    assert!(
        fresh_frame(on_discard, second_discard, time) != at_rest,
        "control: a hovered Discard draws like a resting one"
    );
    assert!(
        kept == at_rest,
        "the second prompt's Discard draws a stale hover"
    );
}

// --- pixels, presses and taps on the open list ---------------------------------

/// Device pixels per logical pixel in every `Live::frame`.
const PX: f32 = 2.0;

fn frame_width() -> usize {
    (window_settings().size.width * PX) as usize
}

/// The RGBA pixel of `frame` at the logical point `at`.
fn pixel(frame: &[u8], at: iced::Point) -> [u8; 4] {
    let i = (((at.y * PX) as usize) * frame_width() + (at.x * PX) as usize) * 4;
    [frame[i], frame[i + 1], frame[i + 2], frame[i + 3]]
}

/// Whether two pixels match within anti-aliasing noise.
fn near(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 6)
}

/// The device pixels of the logical rect `r`, clipped to the frame.
fn device_pixels(r: iced::Rectangle) -> impl Iterator<Item = usize> {
    let size = window_settings().size;
    let (w, h) = (frame_width(), (size.height * PX) as usize);
    let x0 = (r.x.max(0.0) * PX) as usize;
    let y0 = (r.y.max(0.0) * PX) as usize;
    let x1 = (((r.x + r.width) * PX).max(0.0) as usize).min(w);
    let y1 = (((r.y + r.height) * PX).max(0.0) as usize).min(h);
    (y0..y1).flat_map(move |y| (x0..x1).map(move |x| y * w + x))
}

/// How many device pixels of `r` differ between two frames.
fn differing(a: &[u8], b: &[u8], r: iced::Rectangle) -> usize {
    device_pixels(r)
        .filter(|i| a[i * 4..i * 4 + 4] != b[i * 4..i * 4 + 4])
        .count()
}

fn luma(frame: &[u8], i: usize) -> i32 {
    let p = &frame[i * 4..i * 4 + 3];
    (299 * i32::from(p[0]) + 587 * i32::from(p[1]) + 114 * i32::from(p[2])) / 1000
}

/// The device pixels, inside `within` and outside every rect of `keep`, that
/// `open` draws clearly brighter than `shut`: in the dark scheme a glyph is
/// the only thing an open list can add that brightens, its shadow darkens.
fn brightened(
    open: &[u8],
    shut: &[u8],
    within: iced::Rectangle,
    keep: &[iced::Rectangle],
) -> usize {
    let w = frame_width();
    device_pixels(within)
        .filter(|&i| {
            let at = iced::Point::new((i % w) as f32 / PX, (i / w) as f32 / PX);
            !keep.iter().any(|r| r.contains(at)) && luma(open, i) > luma(shut, i) + 40
        })
        .count()
}

fn grown(r: iced::Rectangle, by: f32) -> iced::Rectangle {
    iced::Rectangle {
        x: r.x - by,
        y: r.y - by,
        width: r.width + 2.0 * by,
        height: r.height + 2.0 * by,
    }
}

fn whole_window() -> iced::Rectangle {
    let size = window_settings().size;
    iced::Rectangle::new(iced::Point::ORIGIN, size)
}

/// The visible bounds of the widget `id` in the shell's current view.
fn bounds_of(shell: &Shell, id: &'static str) -> iced::Rectangle {
    simulator(shell.view())
        .find(iced::widget::Id::new(id))
        .unwrap_or_else(|_| panic!("no {id:?} in the view"))
        .visible_bounds()
        .unwrap()
}

/// The at-rest rect of the open panel of the dropdown at `bounds`, per gui.md.
fn panel_of(bounds: iced::Rectangle, option_count: usize) -> iced::Rectangle {
    let (top, height) = menu_panel(bounds, option_count, window_settings().size.height);
    iced::Rectangle {
        x: bounds.x,
        y: top,
        width: bounds.width,
        height,
    }
}

fn tap(named: iced::keyboard::key::Named) -> Vec<iced::Event> {
    iced_test::simulator::tap_key(named, None).collect()
}

fn clicked(live: &mut Live, shell: &mut Shell) {
    let _ = live.send(shell, &click().collect::<Vec<_>>());
}

/// Opens the dropdown `id` with a click on its centre and settles it at `base`.
fn opened(live: &mut Live, shell: &mut Shell, id: &'static str, base: Instant) -> iced::Rectangle {
    let bounds = bounds_of(shell, id);
    live.point(
        shell,
        iced::Point::new(bounds.center_x(), bounds.center_y()),
    );
    clicked(live, shell);
    live.settle(shell, base);
    bounds
}

/// A shell on Buttons whose first button carries a key bind, so its row shows
/// the key picker.
fn with_key_bind() -> Shell {
    let mut snap = snapshot();
    snap.binds[0] = (
        gctl_proto::Button::Left,
        Some(crate::parse::binding("key:KeyA").unwrap()),
    );
    on_section(snap, Section::Buttons)
}

/// A dropdown field keeps its rounded corners at rest: the pixel just inside
/// its bounds' corner shows the card, not the well.
#[test]
fn a_dropdown_field_keeps_its_rounded_corners() {
    let mut square = Vec::new();
    for scheme in [Scheme::Dark, Scheme::Light] {
        for id in ["polling-picker", "lift-off-picker", "active-stage-picker"] {
            let mut shell = shell(snapshot());
            shell.scheme = scheme;
            let b = bounds_of(&shell, id);
            let mut live = Live::new();
            live.point(&mut shell, iced::Point::new(2.0, 2.0));
            let f = kept_frame(&mut live, &mut shell, Instant::now() + SETTLED);
            let card = pixel(&f, iced::Point::new(b.x - 3.0, b.center_y()));
            let well = pixel(&f, iced::Point::new(b.x + 4.0, b.center_y()));
            assert!(
                !near(card, well),
                "control, {scheme:?} {id}: the well {well:?} draws like the card {card:?}"
            );
            for corner in [
                iced::Point::new(b.x + 1.0, b.y + 1.0),
                iced::Point::new(b.x + b.width - 1.5, b.y + b.height - 1.5),
            ] {
                let got = pixel(&f, corner);
                if !near(got, card) {
                    square.push(format!(
                        "{scheme:?} {id}: the corner at {corner:?} draws {got:?}, the card is {card:?}"
                    ));
                }
            }
        }
    }
    assert!(square.is_empty(), "square corners: {square:#?}");
}

/// A hovered profile segment keeps its rounded corners: its corner pixel is
/// the track's, as at rest. Exact pixels, since the two frames differ
/// only in the hover fill and the light hover step is smaller than `near`'s
/// tolerance.
#[test]
fn a_hovered_profile_segment_keeps_its_rounded_corners() {
    for scheme in [Scheme::Dark, Scheme::Light] {
        let time = Instant::now() + SETTLED;
        let in_scheme = || {
            let mut shell = shell(snapshot());
            shell.scheme = scheme;
            shell
        };
        let s = bounds_of(&in_scheme(), "profile-pick-1");
        let away = iced::Point::new(s.center_x(), s.y + 200.0);
        let resting = fresh_frame(in_scheme(), away, time);
        let hovered = fresh_frame(
            in_scheme(),
            iced::Point::new(s.center_x(), s.center_y()),
            time,
        );
        let inside = iced::Point::new(s.x + 6.0, s.center_y());
        assert_ne!(
            pixel(&hovered, inside),
            pixel(&resting, inside),
            "control, {scheme:?}: a hovered segment draws like a resting one"
        );
        for corner in [
            iced::Point::new(s.x + 1.0, s.y + 1.0),
            iced::Point::new(s.x + 1.0, s.y + s.height - 1.5),
        ] {
            assert_eq!(
                pixel(&hovered, corner),
                pixel(&resting, corner),
                "{scheme:?}: the hovered segment fills its square corner at {corner:?}"
            );
        }
    }
}

/// An open list left settled with no redraw for seconds (the runtime asks for
/// none once nothing moves) still folds shut on Escape: 30 ms after the
/// key the panel still draws, as it does when a redraw came just before.
#[test]
fn an_idle_open_list_folds_shut_on_escape() {
    let mut closing_vs_shut = Vec::new();
    for fresh_redraw in [true, false] {
        let mut shell = shell(snapshot());
        let base = Instant::now() + Duration::from_secs(3600);
        let mut live = Live::new();
        let bounds = opened(&mut live, &mut shell, "polling-picker", base);
        let open = live.frame(&shell);
        let idle = base + SETTLED + Duration::from_secs(5);
        if fresh_redraw {
            let _ = live.send(&mut shell, &[redraw(idle)]);
        }
        let _ = live.send(&mut shell, &tap(iced::keyboard::key::Named::Escape));
        let _ = live.send(&mut shell, &[redraw(idle + Duration::from_millis(30))]);
        let closing = live.frame(&shell);
        live.settle(&mut shell, idle + Duration::from_secs(1));
        let shut = live.frame(&shell);
        let panel = panel_of(bounds, POLLING_RATES.len());
        assert!(
            differing(&open, &shut, panel) > 0,
            "control: the open panel draws like the shut window"
        );
        closing_vs_shut.push(differing(&closing, &shut, panel));
    }
    assert!(
        closing_vs_shut[0] > 0,
        "control: with a redraw just before Escape the close shows no fold"
    );
    assert!(
        closing_vs_shut[1] > 0,
        "an idle open list vanished on Escape with no fold"
    );
}

/// Reopening a list after the pointer hovered a row draws no costume on that
/// row: 40 ms into the reopen it draws like a list never hovered.
#[test]
fn a_reopened_list_draws_no_stale_row_costume() {
    let height = window_settings().size.height;
    let mut settled = Vec::new();
    let mut reopened = Vec::new();
    let mut row_0 = iced::Rectangle::default();
    for hovered in [false, true] {
        let mut shell = shell(snapshot());
        let base = Instant::now() + Duration::from_secs(3600);
        let mut live = Live::new();
        let bounds = opened(&mut live, &mut shell, "polling-picker", base);
        let trigger = iced::Point::new(bounds.center_x(), bounds.center_y());
        let panel = panel_of(bounds, POLLING_RATES.len());
        row_0 = iced::Rectangle {
            y: panel.y + MENU_GUTTER,
            height: MENU_ROW,
            ..panel
        };
        if hovered {
            let over = iced::Point::new(
                trigger.x,
                menu_row_y(bounds, POLLING_RATES.len(), 0, height),
            );
            live.point(&mut shell, over);
        }
        live.settle(&mut shell, base + SETTLED * 2);
        settled.push(live.frame(&shell));
        live.point(&mut shell, trigger);
        let t = base + SETTLED * 4;
        let _ = live.send(&mut shell, &[redraw(t)]);
        let _ = live.send(&mut shell, &tap(iced::keyboard::key::Named::Escape));
        live.settle(&mut shell, t + Duration::from_millis(1));
        let reopen = t + SETTLED * 2;
        let _ = live.send(&mut shell, &[redraw(reopen)]);
        clicked(&mut live, &mut shell);
        let _ = live.send(&mut shell, &[redraw(reopen + Duration::from_millis(16))]);
        let _ = live.send(&mut shell, &[redraw(reopen + Duration::from_millis(40))]);
        reopened.push(live.frame(&shell));
    }
    assert!(
        differing(&settled[0], &settled[1], row_0) > 0,
        "control: a hovered row draws like a resting one"
    );
    assert_eq!(
        differing(&reopened[0], &reopened[1], row_0),
        0,
        "the reopened list drew the old row's costume"
    );
}

/// A press on the scroll thumb of the key list picks no key and keeps the
/// list open; a press on a row at the same height picks one.
#[test]
fn a_press_on_the_scroll_thumb_picks_nothing() {
    let n = key_names().len();
    let content = MENU_MAX - 2.0 * MENU_GUTTER;
    let total = n as f32 * MENU_ROW;
    let scrolled = 3.0 * MENU_ROW;
    let mut outcomes = Vec::new();
    for on_thumb in [false, true] {
        let mut shell = with_key_bind();
        let base = Instant::now() + Duration::from_secs(3600);
        let mut live = Live::new();
        let shut = live.frame(&shell);
        let bounds = opened(&mut live, &mut shell, "key-picker-left", base);
        let panel = panel_of(bounds, n);
        live.point(
            &mut shell,
            iced::Point::new(panel.center_x(), panel.center_y()),
        );
        let _ = live.send(
            &mut shell,
            &[iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 },
            })],
        );
        live.settle(&mut shell, base + SETTLED * 2);
        // the thumb's span along the rail, its length the visible share of the rows
        let thumb_h = (content * content / total).max(24.0).min(content);
        let thumb_y = panel.y + MENU_GUTTER + scrolled / (total - content) * (content - thumb_h);
        let x = if on_thumb {
            panel.x + panel.width - 10.0
        } else {
            panel.center_x()
        };
        live.point(&mut shell, iced::Point::new(x, thumb_y + thumb_h / 2.0));
        clicked(&mut live, &mut shell);
        live.settle(&mut shell, base + SETTLED * 4);
        let open = differing(&live.frame(&shell), &shut, panel) > 0;
        outcomes.push((shell.machine.pending(Section::Buttons), open));
    }
    assert!(
        outcomes[0].0,
        "control: a press on a row at the thumb's height picked nothing"
    );
    assert!(!outcomes[1].0, "a press on the scroll thumb picked a key");
    assert!(outcomes[1].1, "a press on the scroll thumb closed the list");
}

/// A press in the open panel's gutter, over the lift-off trigger the panel
/// covers, changes nothing on screen and reaches nothing under the panel
/// a press on a row at the same height picks and closes.
#[test]
fn a_press_in_the_panels_gutter_stays_in_the_panel() {
    for in_gutter in [false, true] {
        let mut shell = shell(snapshot());
        let base = Instant::now() + Duration::from_secs(3600);
        let lift = bounds_of(&shell, "lift-off-picker");
        let mut live = Live::new();
        let bounds = opened(&mut live, &mut shell, "polling-picker", base);
        let panel = panel_of(bounds, POLLING_RATES.len());
        let x = if in_gutter {
            panel.x + MENU_GUTTER / 2.0
        } else {
            panel.center_x()
        };
        let p = iced::Point::new(x, lift.center_y());
        assert!(
            lift.contains(p) && panel.contains(p),
            "the press at {p:?} misses the lift-off trigger {lift:?} or the panel {panel:?}"
        );
        live.point(&mut shell, p);
        let before = kept_frame(&mut live, &mut shell, base + SETTLED * 2);
        clicked(&mut live, &mut shell);
        let after = kept_frame(&mut live, &mut shell, base + SETTLED * 4);
        let moved = differing(&before, &after, whole_window());
        if in_gutter {
            assert_eq!(moved, 0, "a press in the panel's gutter changed the window");
            assert!(
                !shell.machine.pending(Section::Performance),
                "a press in the panel's gutter made an edit"
            );
        } else {
            assert!(moved > 0, "control: a press on a row changed nothing");
        }
    }
}

/// An open list draws nothing outside its panel and its trigger: no row label
/// of a partly shown row and no caret spills over the controls or the footer
/// around it. Dark scheme, where a glyph is the only thing that
/// brightens a pixel.
#[test]
fn an_open_list_draws_nothing_outside_its_panel() {
    let mut low_keys = snapshot();
    for bind in &mut low_keys.binds {
        bind.1 = Some(crate::parse::binding("key:F12").unwrap());
    }
    let cases: [(&str, Shell, &'static str); 3] = [
        (
            "the key list below its trigger",
            with_key_bind(),
            "key-picker-left",
        ),
        (
            "the key list above a trigger the pane's edge cuts",
            on_section(low_keys, Section::Buttons),
            "key-picker-scroll-down",
        ),
        ("the polling list", shell(snapshot()), "polling-picker"),
    ];
    let mut spills = Vec::new();
    for (case, mut shell, id) in cases {
        let base = Instant::now() + Duration::from_secs(3600);
        let mut live = Live::new();
        let trigger = bounds_of(&shell, id);
        live.point(
            &mut shell,
            iced::Point::new(trigger.center_x(), trigger.center_y()),
        );
        let shut = kept_frame(&mut live, &mut shell, base);
        let options = if id == "polling-picker" {
            POLLING_RATES.len()
        } else {
            key_names().len()
        };
        let _ = opened(&mut live, &mut shell, id, base + SETTLED * 2);
        let open = live.frame(&shell);
        let panel = panel_of(trigger, options);
        assert!(
            brightened(&open, &shut, panel, &[]) > 0,
            "control, {case}: the panel's rows add no glyph"
        );
        let spilled = brightened(
            &open,
            &shut,
            whole_window(),
            &[grown(panel, 1.0), grown(trigger, 1.0)],
        );
        spills.push((case, spilled));
    }
    assert!(
        spills.iter().all(|(_, spilled)| *spilled == 0),
        "glyph pixels outside the panel and the trigger: {spills:?}"
    );
}

/// A tap at `at`: the finger lands and lifts there, the pointer following it
/// as the winit runtime moves it with a touch.
fn tapped(live: &mut Live, shell: &mut Shell, at: iced::Point) {
    live.cursor = iced::mouse::Cursor::Available(at);
    let finger = iced::touch::Finger(0);
    let _ = live.send(
        shell,
        &[
            iced::Event::Touch(iced::touch::Event::FingerPressed {
                id: finger,
                position: at,
            }),
            iced::Event::Touch(iced::touch::Event::FingerLifted {
                id: finger,
                position: at,
            }),
        ],
    );
}

/// A press on the scroll rail beside the thumb, below it, picks no key and
/// keeps the list open; a press on a row at the same height picks one.
#[test]
fn a_press_on_the_scroll_rail_beside_the_thumb_picks_nothing() {
    let n = key_names().len();
    let content = MENU_MAX - 2.0 * MENU_GUTTER;
    let total = n as f32 * MENU_ROW;
    let scrolled = 3.0 * MENU_ROW;
    let mut outcomes = Vec::new();
    for on_rail in [false, true] {
        let mut shell = with_key_bind();
        let base = Instant::now() + Duration::from_secs(3600);
        let mut live = Live::new();
        let shut = live.frame(&shell);
        let bounds = opened(&mut live, &mut shell, "key-picker-left", base);
        let panel = panel_of(bounds, n);
        live.point(
            &mut shell,
            iced::Point::new(panel.center_x(), panel.center_y()),
        );
        let _ = live.send(
            &mut shell,
            &[iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 },
            })],
        );
        live.settle(&mut shell, base + SETTLED * 2);
        // the thumb's span along the rail, its length the visible share of the rows
        let thumb_h = (content * content / total).max(24.0).min(content);
        let thumb_y = panel.y + MENU_GUTTER + scrolled / (total - content) * (content - thumb_h);
        let y = (thumb_y + thumb_h + 40.0).min(panel.y + panel.height - MENU_GUTTER - 4.0);
        assert!(
            y > thumb_y + thumb_h + 4.0,
            "no rail below the thumb at {y}"
        );
        let x = if on_rail {
            panel.x + panel.width - 10.0
        } else {
            panel.center_x()
        };
        live.point(&mut shell, iced::Point::new(x, y));
        clicked(&mut live, &mut shell);
        live.settle(&mut shell, base + SETTLED * 4);
        let open = differing(&live.frame(&shell), &shut, panel) > 0;
        outcomes.push((shell.machine.pending(Section::Buttons), open));
    }
    assert!(
        outcomes[0].0,
        "control: a press on a row beside the rail picked nothing"
    );
    assert!(
        !outcomes[1].0,
        "a press on the rail beside the thumb picked a key"
    );
    assert!(outcomes[1].1, "a press on the rail closed the list");
}

/// A tap acts like a click on every kind of control the window draws: a
/// button, a section row, a dropdown's trigger and a row of its open list.
#[test]
fn a_touch_tap_acts_like_a_click() {
    let mut failed = Vec::new();
    for touch in [false, true] {
        let press = |live: &mut Live, shell: &mut Shell, at: iced::Point| {
            if touch {
                tapped(live, shell, at);
            } else {
                live.point(shell, at);
                clicked(live, shell);
            }
        };
        let base = Instant::now() + Duration::from_secs(3600);

        let mut edited = pending();
        let mut live = Live::new();
        live.settle(&mut edited, base);
        let revert = centre_of(&edited, "Revert");
        press(&mut live, &mut edited, revert);
        if edited.machine.pending(Section::Performance) {
            failed.push(format!("touch {touch}: Revert kept the edit"));
        }

        let mut on_rows = shell(snapshot());
        let mut live = Live::new();
        live.settle(&mut on_rows, base);
        let lighting = centre_of(&on_rows, "Lighting");
        press(&mut live, &mut on_rows, lighting);
        if simulator(on_rows.view()).find("Effect").is_err() {
            failed.push(format!(
                "touch {touch}: the Lighting row did not open its pane"
            ));
        }

        let mut on_list = shell(snapshot());
        let mut live = Live::new();
        let b = bounds_of(&on_list, "polling-picker");
        let shut = kept_frame(&mut live, &mut on_list, base);
        press(
            &mut live,
            &mut on_list,
            iced::Point::new(b.center_x(), b.center_y()),
        );
        live.settle(&mut on_list, base + SETTLED * 2);
        let panel = panel_of(b, POLLING_RATES.len());
        if differing(&live.frame(&on_list), &shut, panel) == 0 {
            failed.push(format!("touch {touch}: the polling list did not open"));
        }
        let height = window_settings().size.height;
        let row_0 = iced::Point::new(b.center_x(), menu_row_y(b, POLLING_RATES.len(), 0, height));
        press(&mut live, &mut on_list, row_0);
        if !on_list.machine.pending(Section::Performance) {
            failed.push(format!(
                "touch {touch}: a row of the open list picked nothing"
            ));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A list reopened while its close still draws keeps the costume the closing
/// panel shows and fades it, never dropping it in one frame.
#[test]
fn a_list_reopened_while_closing_fades_the_old_costume() {
    let height = window_settings().size.height;
    let mut closing = Vec::new();
    let mut reopened = Vec::new();
    let mut row_0 = iced::Rectangle::default();
    for hovered in [false, true] {
        let mut shell = shell(snapshot());
        let base = Instant::now() + Duration::from_secs(3600);
        let mut live = Live::new();
        let bounds = opened(&mut live, &mut shell, "polling-picker", base);
        let trigger = iced::Point::new(bounds.center_x(), bounds.center_y());
        let panel = panel_of(bounds, POLLING_RATES.len());
        row_0 = iced::Rectangle {
            y: panel.y + MENU_GUTTER,
            height: MENU_ROW,
            ..panel
        };
        if hovered {
            let over = iced::Point::new(
                trigger.x,
                menu_row_y(bounds, POLLING_RATES.len(), 0, height),
            );
            live.point(&mut shell, over);
        }
        live.settle(&mut shell, base + SETTLED * 2);
        live.point(&mut shell, trigger);
        let t = base + SETTLED * 4;
        let _ = live.send(&mut shell, &[redraw(t)]);
        let _ = live.send(&mut shell, &tap(iced::keyboard::key::Named::Escape));
        let _ = live.send(&mut shell, &[redraw(t + Duration::from_millis(1))]);
        let _ = live.send(&mut shell, &[redraw(t + Duration::from_millis(20))]);
        closing.push(live.frame(&shell));
        clicked(&mut live, &mut shell);
        let _ = live.send(&mut shell, &[redraw(t + Duration::from_millis(30))]);
        reopened.push(live.frame(&shell));
    }
    assert!(
        differing(&closing[0], &closing[1], row_0) > 0,
        "control: the closing panel shows no costume"
    );
    assert!(
        differing(&reopened[0], &reopened[1], row_0) > 0,
        "the costume the closing panel showed vanished in one frame on reopen"
    );
}
